//! Linux syscall wrappers for low-level assembly I/O.

use std::fs::File;
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::Path;

use super::error::{SophonError, SophonResult};

/// Kernel-side copy between two open file descriptors. Mirrors
/// `copy_file_range(2)`: copies up to `len` bytes from `src_fd` at
/// `src_off` to `dst_fd` at `dst_off`. Returns the number of bytes copied.
/// Loops if the kernel copies fewer than requested per call.
pub fn copy_file_range(
    src_fd: RawFd,
    src_off: u64,
    dst_fd: RawFd,
    dst_off: u64,
    len: u64,
) -> SophonResult<u64> {
    // copy_file_range takes off_t* (in/out); pass mutable stack slots so the
    // kernel updates the running offsets across iterations.
    let mut src_off_val: libc::off_t = src_off as libc::off_t;
    let mut dst_off_val: libc::off_t = dst_off as libc::off_t;
    let mut copied: u64 = 0;
    while copied < len {
        let remaining = len - copied;
        let this_iter = remaining.min(u64::try_from(isize::MAX).unwrap_or(usize::MAX as u64));
        let n = unsafe {
            libc::copy_file_range(
                src_fd,
                &mut src_off_val,
                dst_fd,
                &mut dst_off_val,
                this_iter as usize,
                0,
            )
        };
        if n < 0 {
            return Err(SophonError::Io(std::io::Error::last_os_error()));
        }
        if n == 0 {
            break;
        }
        copied += n as u64;
    }
    Ok(copied)
}

/// Copy a region of `src_path` into `dst_file` at `dst_off` using
/// `copy_file_range`. Applies `POSIX_FADV_DONTNEED` on the source region
/// once copied so the source pages do not stay resident.
pub fn copy_file_region_to(
    src_path: &Path,
    src_off: u64,
    dst_file: &File,
    dst_off: u64,
    len: u64,
) -> SophonResult<u64> {
    let src = File::open(src_path).map_err(SophonError::Io)?;
    let src_fd = src.as_raw_fd();
    let dst_fd = dst_file.as_raw_fd();
    let copied = copy_file_range(src_fd, src_off, dst_fd, dst_off, len)?;
    // Drop source pages from the cache now that the kernel has copied the data.
    let _ = unsafe {
        libc::posix_fadvise(
            src_fd,
            src_off as libc::off_t,
            len as libc::off_t,
            libc::POSIX_FADV_DONTNEED,
        )
    };
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_file_range_basic() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src.bin");
        let dst_path = dir.path().join("dst.bin");
        let data = b"hello copy_file_range world";
        std::fs::write(&src_path, data).unwrap();

        let dst = File::create(&dst_path).unwrap();
        // Pre-size so the offset write is valid.
        dst.set_len(data.len() as u64).unwrap();
        let n = copy_file_region_to(&src_path, 0, &dst, 0, data.len() as u64).unwrap();
        assert_eq!(n, data.len() as u64);
        dst.sync_all().unwrap();
        drop(dst);
        assert_eq!(std::fs::read(&dst_path).unwrap(), data);
    }

    #[test]
    fn copy_file_range_at_offset() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src.bin");
        let dst_path = dir.path().join("dst.bin");
        std::fs::write(&src_path, b"AAAABBBBCCCCDDDD").unwrap();

        let dst = File::create(&dst_path).unwrap();
        dst.set_len(16).unwrap();
        // Copy bytes 4..8 ("BBBB") to destination offset 0.
        let n = copy_file_region_to(&src_path, 4, &dst, 0, 4).unwrap();
        assert_eq!(n, 4);
        dst.sync_all().unwrap();
        drop(dst);
        assert_eq!(&std::fs::read(&dst_path).unwrap()[..4], b"BBBB");
    }

    #[test]
    fn copy_file_range_missing_source_fails() {
        let dir = tempfile::tempdir().unwrap();
        let dst_path = dir.path().join("dst.bin");
        let dst = File::create(&dst_path).unwrap();
        let nope = dir.path().join("nope.bin");
        let res = copy_file_region_to(&nope, 0, &dst, 0, 4);
        assert!(res.is_err());
    }
}
