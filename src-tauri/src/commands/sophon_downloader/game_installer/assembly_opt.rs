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

const EMPTY_MD5: &str = "00000000000000000000000000000000";

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

    pub fn finish(mut self) -> io::Result<Vec<u8>> {
        self.inner
            .finish()
            .map(|d| d.to_vec())
            .map_err(|e| io::Error::other(e.to_string()))
    }
}

thread_local! {
    static OPT_BUFFER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
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
/// chunk files. Blocking: waits for writeback to complete before evicting.
#[inline]
pub(crate) fn sync_and_evict_range(fd: std::os::unix::io::RawFd, offset: u64, len: u64) {
    let _ = unsafe {
        libc::sync_file_range(
            fd,
            offset as libc::off64_t,
            len as libc::off64_t,
            libc::SYNC_FILE_RANGE_WRITE | libc::SYNC_FILE_RANGE_WAIT_AFTER,
        )
    };
    posix_advise(fd, offset, len, libc::POSIX_FADV_DONTNEED);
}

/// Returns true when the chunk hash must be verified against the decompressed
/// bytes.
#[inline]
fn chunk_hash_required(chunk_decompressed_hash_md5: &str) -> bool {
    chunk_decompressed_hash_md5.len() == 32 && chunk_decompressed_hash_md5 != EMPTY_MD5
}

/// Copy a chunk from an old file into `out_file` at `new_offset`. Uses the
/// kernel `copy_file_range` syscall when no per-chunk or per-file hashing is
/// required, avoiding any in-user-space copy of the bytes. Otherwise streams
/// the region through a thread-local buffer to compute MD5 while writing.
pub fn write_chunk_from_mmap(
    old_file_path: &Path,
    out_file: &File,
    new_offset: u64,
    old_offset: u64,
    expected_size: u64,
    mut file_hasher: Option<&mut Md5>,
    chunk_decompressed_hash_md5: &str,
) -> SophonResult<u64> {
    let needs_chunk_hash = chunk_hash_required(chunk_decompressed_hash_md5);
    // When neither the file nor the chunk require hashing, defer to a pure
    // kernel-side copy: zero bytes touch user space.
    if file_hasher.is_none() && !needs_chunk_hash {
        let copied = sysio::copy_file_region_to(
            old_file_path,
            old_offset,
            out_file,
            new_offset,
            expected_size,
        )?;
        if copied != expected_size {
            return Err(SophonError::SizeMismatch {
                item: old_file_path.display().to_string(),
                expected: expected_size,
                actual: copied,
            });
        }
        return Ok(copied);
    }

    let file = File::open(old_file_path).map_err(SophonError::Io)?;
    let file_len = file.metadata().map_err(SophonError::Io)?.len();
    // Streaming read of a single region: enable read-ahead, then evict the
    // pages once consumed so the old file does not crowd the page cache.
    posix_advise(
        file.as_raw_fd(),
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

    let mut chunk_hasher = Md5::new()?;
    let mut buf = OPT_BUFFER.with(|cell| {
        let mut buf = cell.take();
        if buf.capacity() < ASSEMBLY_BUFFER_SIZE {
            buf = Vec::with_capacity(ASSEMBLY_BUFFER_SIZE);
        }
        // Safety: buffer is fully overwritten by read_at before write_all_at.
        unsafe { buf.set_len(ASSEMBLY_BUFFER_SIZE) };
        buf
    });

    let mut remaining = expected_size_u;
    let mut write_offset = new_offset;
    while remaining > 0 {
        let to_read = remaining.min(buf.len());
        let read_at = old_offset + (expected_size_u - remaining) as u64;
        let n = file
            .read_at(&mut buf[..to_read], read_at)
            .map_err(SophonError::Io)?;
        if n == 0 {
            break;
        }
        chunk_hasher.update(&buf[..n])?;
        if let Some(hasher) = file_hasher.as_deref_mut() {
            hasher.update(&buf[..n])?;
        }
        out_file.write_all_at(&buf[..n], write_offset)?;
        write_offset += n as u64;
        remaining -= n;
    }

    posix_advise(
        file.as_raw_fd(),
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

    if needs_chunk_hash {
        let actual = hex::encode(chunk_hasher.finish()?);
        if actual != chunk_decompressed_hash_md5 {
            return Err(SophonError::Md5Mismatch {
                item: old_file_path.display().to_string(),
                expected: chunk_decompressed_hash_md5.to_string(),
                actual,
            });
        }
    }

    Ok(bytes_written)
}

/// Decompress a chunk file and write the bytes to `out_file` at `offset`,
/// computing the chunk and file MD5 values in the same pass.
pub fn decompress_chunk_optimized(
    chunk_path: &Path,
    out_file: &File,
    offset: u64,
    expected_size: u64,
    file_hasher: Option<&mut Md5>,
    chunk_decompressed_hash_md5: &str,
) -> SophonResult<u64> {
    let f = File::open(chunk_path)?;
    // Compressed chunks are read once and then deleted; advise sequential
    // read-ahead and evict the pages after decompression to keep the page
    // cache free for the larger decompressed output.
    let compressed_fd = f.as_raw_fd();
    posix_advise(compressed_fd, 0, 0, libc::POSIX_FADV_SEQUENTIAL);
    let buf_reader = BufReader::with_capacity(FILE_WRITE_BUFFER_SIZE, f);
    let mut decoder = zstd::Decoder::new(buf_reader)?;

    let window_log: u32 = if cfg!(target_pointer_width = "64") {
        26
    } else {
        25
    };
    decoder.set_parameter(zstd::zstd_safe::DParameter::WindowLogMax(window_log))?;

    let mut bytes_written: u64 = 0;
    let mut write_offset = offset;
    let mut chunk_hasher = Md5::new()?;

    let mut buffer = OPT_BUFFER.with(|cell| {
        let mut buf = cell.take();
        if buf.capacity() < ASSEMBLY_BUFFER_SIZE {
            buf = Vec::with_capacity(ASSEMBLY_BUFFER_SIZE);
        }
        unsafe { buf.set_len(ASSEMBLY_BUFFER_SIZE) };
        buf
    });

    match file_hasher {
        Some(hasher) => loop {
            let n = decoder.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            chunk_hasher.update(&buffer[..n])?;
            hasher.update(&buffer[..n])?;
            out_file.write_all_at(&buffer[..n], write_offset)?;
            write_offset += n as u64;
            bytes_written += n as u64;
        },
        None => loop {
            let n = decoder.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            if chunk_hash_required(chunk_decompressed_hash_md5) {
                chunk_hasher.update(&buffer[..n])?;
            }
            out_file.write_all_at(&buffer[..n], write_offset)?;
            write_offset += n as u64;
            bytes_written += n as u64;
        },
    }

    buffer.clear();
    OPT_BUFFER.with(|cell| cell.replace(buffer));

    // Release the compressed chunk's pages now that decompression is done.
    posix_advise(compressed_fd, 0, 0, libc::POSIX_FADV_DONTNEED);

    if bytes_written != expected_size {
        return Err(SophonError::SizeMismatch {
            item: chunk_path.display().to_string(),
            expected: expected_size,
            actual: bytes_written,
        });
    }

    if chunk_hash_required(chunk_decompressed_hash_md5) {
        let actual = hex::encode(chunk_hasher.finish()?);
        if actual != chunk_decompressed_hash_md5 {
            return Err(SophonError::Md5Mismatch {
                item: chunk_path.display().to_string(),
                expected: chunk_decompressed_hash_md5.to_string(),
                actual,
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
        let bytes = write_chunk_from_mmap(&src, &out, 0, 0, 11, None, "").unwrap();
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
        let bytes = write_chunk_from_mmap(&src, &out, 0, 0, data.len() as u64, None, "").unwrap();
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
        let bytes =
            write_chunk_from_mmap(&src, &out, 0, 0, data.len() as u64, None, &chunk_md5).unwrap();
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
        let res = write_chunk_from_mmap(&src, &out, 0, 0, data.len() as u64, None, wrong);
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
}
