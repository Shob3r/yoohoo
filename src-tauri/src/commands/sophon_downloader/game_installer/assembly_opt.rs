//! Optimized assembly helpers with direct offset writes and kernel-side copies.

use std::cell::RefCell;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::os::unix::fs::FileExt;
use std::os::unix::io::AsRawFd;
use std::path::Path;

use super::FILE_WRITE_BUFFER_SIZE;
use super::error::{SophonError, SophonResult};
use super::sysio;

const ASSEMBLY_BUFFER_SIZE: usize = 64 * 1024;

const ONESHOT_MAX_SIZE: usize = 16 * 1024 * 1024;

const EMPTY_MD5: &str = "00000000000000000000000000000000";

pub(crate) const MAX_WINDOW_LOG: u32 = 26;

/// Compute the minimum `WindowLogMax` that fits a zstd frame of
/// `decompressed_size`. The zstd window must be a power of 2 and at least
/// as large as the decompressed size, so we round up to the next power of
/// two and use its log2. Clamped to libzstd's `[10, 26]` valid range.
#[inline]
pub(crate) fn window_log_for_size(decompressed_size: u64) -> u32 {
    const MIN_WINDOW_LOG: u32 = 10;
    let pow2 = decompressed_size.max(1).next_power_of_two();
    let log = pow2.trailing_zeros();
    log.clamp(MIN_WINDOW_LOG, MAX_WINDOW_LOG)
}

/// MD5 hasher backed by OpenSSL EVP. Hardware-accelerated on x86_64 via
/// libcrypto, ~28% higher throughput than the md-5 crate.
pub(crate) struct Md5 {
    inner: openssl::hash::Hasher,
}

impl Md5 {
    pub fn new() -> SophonResult<Self> {
        Ok(Self {
            inner: openssl::hash::Hasher::new(openssl::hash::MessageDigest::md5())
                .map_err(|e| SophonError::Io(io::Error::other(e.to_string())))?,
        })
    }

    #[inline]
    pub fn update(&mut self, data: &[u8]) -> io::Result<()> {
        self.inner
            .update(data)
            .map_err(|e| io::Error::other(e.to_string()))
    }

    pub fn finish(&mut self) -> io::Result<[u8; 16]> {
        let digest = self
            .inner
            .finish()
            .map_err(|e| io::Error::other(e.to_string()))?;
        let mut out = [0u8; 16];
        let n = digest.len().min(16);
        out[..n].copy_from_slice(&digest[..n]);
        Ok(out)
    }
}

thread_local! {
    static MD5_POOL: RefCell<Option<Md5>> = const { RefCell::new(None) };
}

pub(crate) fn take_md5() -> SophonResult<Md5> {
    MD5_POOL.with(|cell| cell.borrow_mut().take().map(Ok).unwrap_or_else(Md5::new))
}

pub(crate) fn return_md5(md5: Md5) {
    MD5_POOL.with(|cell| cell.borrow_mut().replace(md5));
}

/// Compare an MD5 digest against an expected lowercase hex string without
/// allocating. Returns false on length mismatch.
#[inline]
pub(crate) fn md5_hex_eq(digest: &[u8; 16], expected: &str) -> bool {
    if expected.len() != 32 {
        return false;
    }
    let bytes = expected.as_bytes();
    let mut i = 0;
    while i < 16 {
        let hi = hex_nibble(bytes[i * 2]);
        let lo = hex_nibble(bytes[i * 2 + 1]);
        match (hi, lo) {
            (Some(h), Some(l)) if (h << 4) | l == digest[i] => i += 1,
            _ => return false,
        }
    }
    true
}

/// Compare a u64 value against a 16-char lowercase hex string without
/// allocating. Used for XXH64 hash verification.
#[inline]
pub(crate) fn xxh64_hex_eq(value: u64, expected: &str) -> bool {
    if expected.len() != 16 {
        return false;
    }
    let bytes = expected.as_bytes();
    let mut acc: u64 = 0;
    let mut i = 0;
    while i < 16 {
        let nibble = match bytes[i] {
            b'0'..=b'9' => bytes[i] - b'0',
            b'a'..=b'f' => bytes[i] - b'a' + 10,
            b'A'..=b'F' => bytes[i] - b'A' + 10,
            _ => return false,
        };
        acc = (acc << 4) | nibble as u64;
        i += 1;
    }
    acc == value
}

/// Encode an MD5 digest to a lowercase hex String. Only for error paths.
#[inline]
pub(crate) fn md5_to_hex(digest: &[u8; 16]) -> String {
    let mut s = String::with_capacity(32);
    for b in digest {
        s.push(hex_digit(b >> 4));
        s.push(hex_digit(b & 0xf));
    }
    s
}

#[inline]
fn hex_digit(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        _ => (b'a' + n - 10) as char,
    }
}

#[inline]
fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

thread_local! {
    static OPT_BUFFER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static ONESHOT_BUF: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static ZSTD_DCTX: RefCell<Option<zstd::zstd_safe::DCtx<'static>>> = const { RefCell::new(None) };
}

pub(crate) fn take_dctx() -> zstd::zstd_safe::DCtx<'static> {
    ZSTD_DCTX.with(|cell| {
        cell.borrow_mut()
            .take()
            .unwrap_or_else(zstd::zstd_safe::DCtx::create)
    })
}

pub(crate) fn return_dctx(ctx: zstd::zstd_safe::DCtx<'static>) {
    ZSTD_DCTX.with(|cell| cell.borrow_mut().replace(ctx));
}

pub(crate) struct MmapGuard {
    ptr: *mut libc::c_void,
    len: usize,
}

impl MmapGuard {
    pub(crate) fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr as *const u8, self.len) }
    }
}

impl Drop for MmapGuard {
    fn drop(&mut self) {
        if self.ptr != libc::MAP_FAILED && !self.ptr.is_null() {
            unsafe { libc::munmap(self.ptr, self.len) };
        }
    }
}

pub(crate) fn mmap_read_only(file: &File) -> io::Result<MmapGuard> {
    let len = file.metadata()?.len() as usize;
    if len == 0 {
        return Err(io::Error::other("empty file"));
    }
    unsafe {
        let ptr = libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            file.as_raw_fd(),
            0,
        );
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        libc::madvise(ptr, len, libc::MADV_SEQUENTIAL);
        Ok(MmapGuard { ptr, len })
    }
}

fn decompress_chunk_oneshot(
    chunk_path: &Path,
    out_file: &File,
    offset: u64,
    expected_size: u64,
    file_hasher: &mut Option<&mut Md5>,
    chunk_decompressed_hash_md5: &str,
) -> SophonResult<u64> {
    if expected_size == 0 {
        return Ok(0);
    }
    let expected_usize = expected_size as usize;

    let f = File::open(chunk_path)?;
    let mmap = mmap_read_only(&f)?;
    let compressed = mmap.as_slice();

    let mut output = ONESHOT_BUF.with(|cell| {
        let mut buf = cell.take();
        if buf.capacity() < expected_usize {
            buf = Vec::with_capacity(expected_usize);
        }
        unsafe { buf.set_len(expected_usize) };
        buf
    });

    let result = (|| {
        let mut ctx = take_dctx();
        ctx.reset(zstd::zstd_safe::ResetDirective::SessionAndParameters)
            .map_err(|_| SophonError::Io(io::Error::other("zstd DCtx reset")))?;

        let written = ctx
            .decompress(&mut output, compressed)
            .map_err(|e| SophonError::Io(io::Error::other(e.to_string())))?;
        return_dctx(ctx);

        if written != expected_usize {
            return Err(SophonError::SizeMismatch {
                item: chunk_path.display().to_string(),
                expected: expected_size,
                actual: written as u64,
            });
        }

        let need_chunk_hash = chunk_hash_required(chunk_decompressed_hash_md5);
        if need_chunk_hash {
            let mut chunk_hasher = take_md5()?;
            chunk_hasher.update(&output)?;
            let digest = chunk_hasher.finish()?;
            return_md5(chunk_hasher);
            if !md5_hex_eq(&digest, chunk_decompressed_hash_md5) {
                return Err(SophonError::Md5Mismatch {
                    item: chunk_path.display().to_string(),
                    expected: chunk_decompressed_hash_md5.to_string(),
                    actual: md5_to_hex(&digest),
                });
            }
        }

        if let Some(hasher) = file_hasher.as_deref_mut() {
            hasher.update(&output)?;
        }

        out_file.write_all_at(&output, offset)?;
        Ok(expected_size)
    })();

    output.clear();
    ONESHOT_BUF.with(|cell| cell.replace(output));
    result
}

/// Hint the kernel about access to `fd`: SEQUENTIAL enables read-ahead for
/// streaming reads; DONTNEED evicts pages from the page cache.
#[inline]
pub(crate) fn posix_advise(
    fd: std::os::unix::io::RawFd,
    offset: u64,
    len: u64,
    advice: libc::c_int,
) {
    let _ = unsafe { libc::posix_fadvise(fd, offset as libc::off_t, len as libc::off_t, advice) };
}

/// Flush a range of the output file to disk, then evict its pages from the
/// page cache. Reduces peak resident memory during assembly of large multi-
/// chunk files. Schedules async writeback without waiting.
#[inline]
pub(crate) fn sync_and_evict_range(fd: std::os::unix::io::RawFd, offset: u64, len: u64) {
    let _ = unsafe {
        libc::sync_file_range(
            fd,
            offset as libc::off64_t,
            len as libc::off64_t,
            libc::SYNC_FILE_RANGE_WRITE,
        )
    };
    posix_advise(fd, offset, len, libc::POSIX_FADV_DONTNEED);
}

/// Returns true when the chunk hash must be verified against the decompressed
/// bytes.
#[inline]
pub(crate) fn chunk_hash_required(chunk_decompressed_hash_md5: &str) -> bool {
    chunk_decompressed_hash_md5.len() == 32 && chunk_decompressed_hash_md5 != EMPTY_MD5
}

/// Copy a chunk from an old file into `out_file` at `new_offset`. Uses the
/// kernel `copy_file_range` syscall when no per-chunk or per-file hashing is
/// required, avoiding any in-user-space copy of the bytes. Otherwise streams
/// the region through a thread-local buffer to compute MD5 while writing.
#[allow(clippy::too_many_arguments)]
pub fn write_chunk_from_mmap(
    old_file: &File,
    old_file_path: &Path,
    out_file: &File,
    new_offset: u64,
    old_offset: u64,
    expected_size: u64,
    mut file_hasher: Option<&mut Md5>,
    chunk_decompressed_hash_md5: &str,
) -> SophonResult<u64> {
    let needs_chunk_hash = chunk_hash_required(chunk_decompressed_hash_md5);
    if file_hasher.is_none() && !needs_chunk_hash {
        let copied =
            sysio::copy_file_from(old_file, old_offset, out_file, new_offset, expected_size)?;
        if copied != expected_size {
            return Err(SophonError::SizeMismatch {
                item: old_file_path.display().to_string(),
                expected: expected_size,
                actual: copied,
            });
        }
        return Ok(copied);
    }

    let file_len = old_file.metadata().map_err(SophonError::Io)?.len();
    posix_advise(
        old_file.as_raw_fd(),
        old_offset,
        expected_size,
        libc::POSIX_FADV_SEQUENTIAL,
    );

    if old_offset + expected_size > file_len {
        return Err(SophonError::SizeMismatch {
            item: old_file_path.display().to_string(),
            expected: expected_size,
            actual: file_len.saturating_sub(old_offset),
        });
    }

    let expected_size_u = expected_size as usize;

    let mut chunk_hasher: Option<Md5> = if needs_chunk_hash {
        Some(take_md5()?)
    } else {
        None
    };
    let mut buf = OPT_BUFFER.with(|cell| {
        let mut buf = cell.take();
        if buf.capacity() < ASSEMBLY_BUFFER_SIZE {
            buf = Vec::with_capacity(ASSEMBLY_BUFFER_SIZE);
        }
        unsafe { buf.set_len(ASSEMBLY_BUFFER_SIZE) };
        buf
    });

    let mut remaining = expected_size_u;
    let mut write_offset = new_offset;
    while remaining > 0 {
        let to_read = remaining.min(buf.len());
        let read_at = old_offset + (expected_size_u - remaining) as u64;
        let n = old_file
            .read_at(&mut buf[..to_read], read_at)
            .map_err(SophonError::Io)?;
        if n == 0 {
            break;
        }
        if let Some(ref mut ch) = chunk_hasher {
            ch.update(&buf[..n])?;
        }
        if let Some(hasher) = file_hasher.as_deref_mut() {
            hasher.update(&buf[..n])?;
        }
        out_file.write_all_at(&buf[..n], write_offset)?;
        write_offset += n as u64;
        remaining -= n;
    }

    posix_advise(
        old_file.as_raw_fd(),
        old_offset,
        expected_size,
        libc::POSIX_FADV_DONTNEED,
    );

    buf.clear();
    OPT_BUFFER.with(|cell| cell.replace(buf));

    let bytes_written = (expected_size_u - remaining) as u64;
    if bytes_written != expected_size {
        return Err(SophonError::SizeMismatch {
            item: old_file_path.display().to_string(),
            expected: expected_size,
            actual: bytes_written,
        });
    }

    if let Some(ref mut ch) = chunk_hasher {
        let digest = ch.finish()?;
        return_md5(chunk_hasher.take().unwrap());
        if !md5_hex_eq(&digest, chunk_decompressed_hash_md5) {
            return Err(SophonError::Md5Mismatch {
                item: old_file_path.display().to_string(),
                expected: chunk_decompressed_hash_md5.to_string(),
                actual: md5_to_hex(&digest),
            });
        }
    }

    Ok(bytes_written)
}

/// Decompress a chunk file and write the bytes to `out_file` at `offset`,
/// computing the chunk and file MD5 values in the same pass.
///
/// Tries a tight `WindowLogMax` first to shrink the DStream's pre-allocated
/// window buffer. If the frame's actual window exceeds the guess (the error
/// surfaces on the first read, before any hashing), falls back to the full
/// capacity cap.
pub fn decompress_chunk_optimized(
    chunk_path: &Path,
    out_file: &File,
    offset: u64,
    expected_size: u64,
    file_hasher: Option<&mut Md5>,
    chunk_decompressed_hash_md5: &str,
) -> SophonResult<u64> {
    let mut file_hasher = file_hasher;

    if expected_size > 0 && expected_size <= ONESHOT_MAX_SIZE as u64 {
        let result = decompress_chunk_oneshot(
            chunk_path,
            out_file,
            offset,
            expected_size,
            &mut file_hasher,
            chunk_decompressed_hash_md5,
        );
        if !is_window_too_small(&result) {
            return result;
        }
    }

    let dynamic_log = window_log_for_size(expected_size);
    let result = decompress_chunk_with_window(
        chunk_path,
        out_file,
        offset,
        expected_size,
        &mut file_hasher,
        chunk_decompressed_hash_md5,
        dynamic_log,
    );
    if is_window_too_small(&result) {
        return decompress_chunk_with_window(
            chunk_path,
            out_file,
            offset,
            expected_size,
            &mut file_hasher,
            chunk_decompressed_hash_md5,
            MAX_WINDOW_LOG,
        );
    }
    result
}

pub(crate) fn is_window_too_small(r: &SophonResult<u64>) -> bool {
    match r {
        Err(SophonError::Io(e)) => {
            let msg = e.to_string();
            msg.contains("out of bound") || msg.contains("too much memory for decoding")
        }
        _ => false,
    }
}

fn decompress_chunk_with_window(
    chunk_path: &Path,
    out_file: &File,
    offset: u64,
    expected_size: u64,
    file_hasher: &mut Option<&mut Md5>,
    chunk_decompressed_hash_md5: &str,
    window_log: u32,
) -> SophonResult<u64> {
    let f = File::open(chunk_path)?;
    let compressed_fd = f.as_raw_fd();
    posix_advise(compressed_fd, 0, 0, libc::POSIX_FADV_SEQUENTIAL);

    let (bytes_written, mut chunk_hasher) = {
        let mut ctx = take_dctx();
        ctx.reset(zstd::zstd_safe::ResetDirective::SessionAndParameters)
            .map_err(|_| SophonError::Io(std::io::Error::other("zstd DCtx reset")))?;
        ctx.set_parameter(zstd::zstd_safe::DParameter::WindowLogMax(window_log))
            .map_err(|_| SophonError::Io(std::io::Error::other("zstd DCtx WindowLogMax")))?;

        let (bytes, chunk_hasher) = {
            let buf_reader = BufReader::with_capacity(FILE_WRITE_BUFFER_SIZE, f);
            let mut decoder = zstd::Decoder::with_context(buf_reader, &mut ctx);

            let need_chunk_hash = chunk_hash_required(chunk_decompressed_hash_md5);
            let mut chunk_hasher = if need_chunk_hash {
                Some(take_md5()?)
            } else {
                None
            };

            let mut buffer = OPT_BUFFER.with(|cell| {
                let mut buf = cell.take();
                if buf.capacity() < ASSEMBLY_BUFFER_SIZE {
                    buf = Vec::with_capacity(ASSEMBLY_BUFFER_SIZE);
                }
                unsafe { buf.set_len(ASSEMBLY_BUFFER_SIZE) };
                buf
            });

            let mut bytes: u64 = 0;
            let mut write_offset = offset;
            match file_hasher.as_deref_mut() {
                Some(hasher) => loop {
                    let n = decoder.read(&mut buffer)?;
                    if n == 0 {
                        break;
                    }
                    if let Some(ref mut ch) = chunk_hasher {
                        ch.update(&buffer[..n])?;
                    }
                    hasher.update(&buffer[..n])?;
                    out_file.write_all_at(&buffer[..n], write_offset)?;
                    write_offset += n as u64;
                    bytes += n as u64;
                },
                None => loop {
                    let n = decoder.read(&mut buffer)?;
                    if n == 0 {
                        break;
                    }
                    if let Some(ref mut ch) = chunk_hasher {
                        ch.update(&buffer[..n])?;
                    }
                    out_file.write_all_at(&buffer[..n], write_offset)?;
                    write_offset += n as u64;
                    bytes += n as u64;
                },
            }

            buffer.clear();
            OPT_BUFFER.with(|cell| cell.replace(buffer));
            (bytes, chunk_hasher)
        }; // decoder dropped, ctx borrow released

        return_dctx(ctx);
        (bytes, chunk_hasher)
    };

    posix_advise(compressed_fd, 0, 0, libc::POSIX_FADV_DONTNEED);

    if bytes_written != expected_size {
        return Err(SophonError::SizeMismatch {
            item: chunk_path.display().to_string(),
            expected: expected_size,
            actual: bytes_written,
        });
    }

    if let Some(ref mut ch) = chunk_hasher {
        let digest = ch.finish()?;
        return_md5(chunk_hasher.take().unwrap());
        if !md5_hex_eq(&digest, chunk_decompressed_hash_md5) {
            return Err(SophonError::Md5Mismatch {
                item: chunk_path.display().to_string(),
                expected: chunk_decompressed_hash_md5.to_string(),
                actual: md5_to_hex(&digest),
            });
        }
    }

    Ok(bytes_written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_at_new(path: &Path) -> File {
        File::create(path).unwrap()
    }

    #[test]
    fn write_chunk_from_mmap_basic() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.bin");
        std::fs::write(&src, b"hello world").unwrap();

        let dst_path = dir.path().join("out.bin");
        let out = write_at_new(&dst_path);
        let src_file = File::open(&src).unwrap();
        let bytes = write_chunk_from_mmap(&src_file, &src, &out, 0, 0, 11, None, "").unwrap();
        drop(out);
        assert_eq!(bytes, 11);
        assert_eq!(std::fs::read(&dst_path).unwrap(), b"hello world");
    }

    #[test]
    fn write_chunk_from_mmap_reads_multiple_buffers() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("large.bin");
        let data = vec![0xABu8; ASSEMBLY_BUFFER_SIZE * 3];
        std::fs::write(&src, &data).unwrap();

        let dst_path = dir.path().join("out.bin");
        let out = write_at_new(&dst_path);
        let src_file = File::open(&src).unwrap();
        let bytes = write_chunk_from_mmap(&src_file, &src, &out, 0, 0, data.len() as u64, None, "")
            .unwrap();
        drop(out);
        assert_eq!(bytes, data.len() as u64);
        assert_eq!(std::fs::read(&dst_path).unwrap().len(), data.len());
    }

    #[test]
    fn write_chunk_from_mmap_with_chunk_hash() {
        let dir = tempfile::tempdir().unwrap();
        let data = b"verify my md5 please";
        let src = dir.path().join("src.bin");
        std::fs::write(&src, data).unwrap();
        let chunk_md5 = {
            let mut h = Md5::new().unwrap();
            h.update(data).unwrap();
            hex::encode(h.finish().unwrap())
        };

        let dst_path = dir.path().join("out.bin");
        let out = write_at_new(&dst_path);
        let src_file = File::open(&src).unwrap();
        let bytes = write_chunk_from_mmap(
            &src_file,
            &src,
            &out,
            0,
            0,
            data.len() as u64,
            None,
            &chunk_md5,
        )
        .unwrap();
        drop(out);
        assert_eq!(bytes, data.len() as u64);
        assert_eq!(std::fs::read(&dst_path).unwrap(), data);
    }

    #[test]
    fn write_chunk_from_mmap_hash_mismatch_fails() {
        let dir = tempfile::tempdir().unwrap();
        let data = b"real data";
        let src = dir.path().join("src.bin");
        std::fs::write(&src, data).unwrap();
        let wrong = "ffffffffffffffffffffffffffffffff";

        let dst_path = dir.path().join("out.bin");
        let out = write_at_new(&dst_path);
        let src_file = File::open(&src).unwrap();
        let res =
            write_chunk_from_mmap(&src_file, &src, &out, 0, 0, data.len() as u64, None, wrong);
        assert!(res.is_err());
    }

    #[test]
    fn decompress_chunk_optimized_basic() {
        let dir = tempfile::tempdir().unwrap();
        let raw = b"decompress this payload";
        let chunk_path = dir.path().join("chunk.zstd");
        let f = File::create(&chunk_path).unwrap();
        let mut enc = zstd::Encoder::new(f, 3).unwrap();
        std::io::Write::write_all(&mut enc, raw).unwrap();
        enc.finish().unwrap();

        let dst_path = dir.path().join("out.bin");
        let out = write_at_new(&dst_path);
        let bytes =
            decompress_chunk_optimized(&chunk_path, &out, 0, raw.len() as u64, None, "").unwrap();
        drop(out);
        assert_eq!(bytes, raw.len() as u64);
        assert_eq!(std::fs::read(&dst_path).unwrap(), raw);
    }

    #[test]
    fn window_log_for_size_returns_minimal_fit() {
        assert_eq!(window_log_for_size(1), 10);
        assert_eq!(window_log_for_size(2), 10);
        assert_eq!(window_log_for_size(4), 10);
        assert_eq!(window_log_for_size(1024), 10);
        assert_eq!(window_log_for_size(1 << 20), 20);
        assert_eq!(window_log_for_size((1 << 20) + 1), 21);
        assert_eq!(window_log_for_size(1 << 23), 23);
        assert_eq!(window_log_for_size(1u64 << 32), MAX_WINDOW_LOG);
        assert_eq!(window_log_for_size(0), 10);
    }

    #[test]
    fn md5_hex_eq_matches_string_comparison() {
        let data = b"the quick brown fox";
        let mut h = Md5::new().unwrap();
        h.update(data).unwrap();
        let digest = h.finish().unwrap();
        let hex = md5_to_hex(&digest);
        assert!(md5_hex_eq(&digest, &hex));
        assert!(!md5_hex_eq(&digest, "00000000000000000000000000000000"));
        assert!(!md5_hex_eq(&digest, "deadbeef"));
        assert!(!md5_hex_eq(&digest, &hex[..31]));
    }

    #[test]
    fn md5_to_hex_roundtrips_against_hex_encode() {
        let data = b"-stack md5 test payload-";
        let mut h = Md5::new().unwrap();
        h.update(data).unwrap();
        let digest = h.finish().unwrap();
        let stack = md5_to_hex(&digest);
        let heap = hex::encode(digest);
        assert_eq!(stack, heap);
    }

    #[test]
    fn xxh64_hex_eq_matches_known_value() {
        let value: u64 = 0x0123456789abcdef;
        let hex = format!("{:016x}", value);
        assert!(xxh64_hex_eq(value, &hex));
        assert!(xxh64_hex_eq(value, &hex.to_uppercase()));
    }

    #[test]
    fn xxh64_hex_eq_rejects_mismatch() {
        let value: u64 = 0x0123456789abcdef;
        assert!(!xxh64_hex_eq(value, "fedcba9876543210"));
        assert!(!xxh64_hex_eq(value, "0123456789abcde"));
        assert!(!xxh64_hex_eq(value, "0123456789abcdeeff"));
    }

    #[test]
    fn xxh64_hex_eq_zero() {
        assert!(xxh64_hex_eq(0, "0000000000000000"));
        assert!(!xxh64_hex_eq(1, "0000000000000000"));
    }
}
