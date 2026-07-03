//! Peak RSS sampler for the Sophon installer hot paths.
//!
//! Resident-set size is a one-shot peak metric, not an iterative timing, so it
//! does not fit the criterion statistical model. This binary polls
//! `/proc/self/statm` from a background thread while each operation runs and
//! reports peak-vs-baseline RSS. Linux-only (the installer is Linux-only).
//!
//! Run:  cargo run --release --features benchmark --bin bench_memory -- [op]
//! where [op] is a substring of one of the operations below; runs all if none.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::FileExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use md5::{Digest, Md5};

use elysiae_lib::commands::sophon_downloader::game_installer::{
    assembly::{assemble_file, chunk_filename},
    cache::{VerificationCache, VerificationEntry},
    compact_manifest::{CompactManifest, StringArena},
    installer::ChunkNameLookup,
    sysio,
};
use elysiae_lib::commands::sophon_downloader::proto_parse::{
    SophonManifestAssetChunk, SophonManifestAssetProperty,
};

/// RSS poll interval. 10 ms catches allocator spikes without measurable load.
const SAMPLE_MS: u64 = 10;

/// Resident set size in KiB, read from `/proc/self/statm`.
fn rss_kib() -> u64 {
    let mut buf = String::new();
    fs::File::open("/proc/self/statm")
        .and_then(|mut f| f.read_to_string(&mut buf))
        .expect("/proc/self/statm: linux-only sampler");
    let resident_pages: u64 = buf.split_whitespace().nth(1).unwrap().parse().unwrap();
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as u64 };
    resident_pages * page / 1024
}

/// Background RSS poller. Tracks the peak resident-set size observed while it
/// runs; stopping it returns the peak.
struct RssSampler {
    peak: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl RssSampler {
    fn start() -> Self {
        let peak = Arc::new(AtomicU64::new(rss_kib()));
        let stop = Arc::new(AtomicBool::new(false));
        let (p, s) = (Arc::clone(&peak), Arc::clone(&stop));
        let handle = thread::spawn(move || {
            loop {
                let r = rss_kib();
                let cur = p.load(Ordering::Relaxed);
                if r > cur {
                    p.store(r, Ordering::Relaxed);
                }
                if s.load(Ordering::Relaxed) {
                    return;
                }
                thread::sleep(Duration::from_millis(SAMPLE_MS));
            }
        });
        RssSampler {
            peak,
            stop,
            handle: Some(handle),
        }
    }

    fn finish(mut self) -> u64 {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        // Sample once more in case the operation finished between polls.
        let last = rss_kib();
        let tracked = self.peak.load(Ordering::Relaxed);
        tracked.max(last)
    }
}

/// Run `op` under the sampler; print peak RSS and delta vs a quiet baseline.
fn measure<F: FnOnce()>(label: &str, op: F) {
    let baseline = rss_kib();
    let sampler = RssSampler::start();
    let t = Instant::now();
    op();
    let dur = t.elapsed();
    let peak = sampler.finish();
    let delta = peak as i64 - baseline as i64;
    println!(
        "{label:<32} {dur:>6.0} ms   RSS {baseline:>6} -> {peak:>6} KiB   (peak delta {delta:+} KiB)",
        dur = dur.as_secs_f64() * 1000.0,
    );
}

fn fill_file(path: &Path, mib: usize, fill: u8) {
    let block = vec![fill; 1024 * 1024];
    let mut f = fs::File::create(path).expect("create");
    for _ in 0..mib {
        f.write_all(&block).expect("write");
    }
    f.sync_all().expect("sync");
}

fn evict_page_cache(path: &Path) {
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    unsafe {
        let fd = libc::open(c.as_ptr(), libc::O_RDONLY);
        if fd >= 0 {
            libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_DONTNEED);
            libc::close(fd);
        }
    }
}

/// Read `mib` MiB with per-region `POSIX_FADV_DONTNEED` so the kernel frees
/// the pages once hashed. Proves the page-cache eviction lowers resident RSS.
fn op_read_md5_with_fadvise(path: &Path, mib: usize, advise: bool) {
    let f = fs::File::open(path).expect("open");
    let fd = f.as_raw_fd();
    let mut buf = vec![0u8; 256 * 1024];
    let mut h = Md5::new();
    let mut off = 0u64;
    let total = (mib * 1024 * 1024) as u64;
    while off < total {
        let n = f.read_at(&mut buf, off).expect("read");
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        if advise {
            unsafe {
                libc::posix_fadvise(
                    fd,
                    off as libc::off_t,
                    n as libc::off_t,
                    libc::POSIX_FADV_DONTNEED,
                );
            }
        }
        off += n as u64;
    }
    let _ = hex::encode(h.finalize());
}

/// Build the same end-to-end assembly fixture the criterion bench uses.
fn build_assembly_fixture(
    chunks_dir: &Path,
    chunk_mib: usize,
    num_chunks: usize,
) -> (CompactManifest, Vec<u8>, String) {
    let mut raw = Vec::with_capacity(chunk_mib * 1024 * 1024 * num_chunks);
    let chunks: Vec<SophonManifestAssetChunk> = (0..num_chunks)
        .map(|i| {
            let name = format!("ck{i:02}");
            let data = vec![(i as u8).wrapping_mul(7).wrapping_add(0x40); chunk_mib * 1024 * 1024];
            let comp = zstd::encode_all(&data[..], 3).unwrap();
            fs::write(chunks_dir.join(chunk_filename(&name)), &comp).unwrap();
            raw.extend_from_slice(&data);
            let offset = (i * chunk_mib * 1024 * 1024) as u64;
            SophonManifestAssetChunk {
                chunk_name: name,
                chunk_decompressed_hash_md5: String::new(),
                chunk_on_file_offset: offset,
                chunk_size: 0,
                chunk_size_decompressed: (chunk_mib * 1024 * 1024) as u64,
                chunk_compressed_hash_xxh: 0,
                chunk_compressed_hash_md5: String::new(),
                chunk_old_offset: -1,
            }
        })
        .collect();
    let file_md5 = hex::encode(Md5::digest(&raw));
    let file = SophonManifestAssetProperty {
        asset_name: "assembled.bin".to_string(),
        asset_chunks: chunks,
        asset_type: 0,
        asset_size: raw.len() as u64,
        asset_hash_md5: file_md5.clone(),
    };
    (CompactManifest::from(vec![file]), raw, file_md5)
}

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("bench_memory_{}", std::process::id()))
}

fn op_assembly_e2e(dir: &Path) {
    let game_dir = dir.join("game");
    let chunks_dir = dir.join("chunks");
    let tmp_dir = dir.join("tmp");
    fs::create_dir_all(&game_dir).unwrap();
    fs::create_dir_all(&chunks_dir).unwrap();
    fs::create_dir_all(&tmp_dir).unwrap();
    let (manifest, _raw, _md5) = build_assembly_fixture(&chunks_dir, 8, 8);
    let name_refs: Vec<&str> = (0..8)
        .map(|i| Box::leak(format!("ck{i:02}").into_boxed_str()) as &str)
        .collect();
    let lookup = ChunkNameLookup::from_arena(StringArena::from(name_refs.as_slice()));
    let refcounts: Vec<std::sync::atomic::AtomicUsize> = (0..8)
        .map(|_| std::sync::atomic::AtomicUsize::new(1000))
        .collect();
    let cache: VerificationCache<String, VerificationEntry> = VerificationCache::new();
    assemble_file(
        &manifest,
        0,
        &game_dir,
        &chunks_dir,
        &tmp_dir,
        &lookup,
        &refcounts,
        &cache,
        true,
    )
    .expect("assemble");
}

fn op_copy_file_range(dir: &Path) {
    let src = dir.join("src.bin");
    fill_file(&src, 64, 0xAB);
    let dst = dir.join("dst.bin");
    let d = fs::File::create(&dst).unwrap();
    d.set_len(64 * 1024 * 1024).unwrap();
    sysio::copy_file_region_to(&src, 0, &d, 0, 64 * 1024 * 1024).unwrap();
    d.sync_all().unwrap();
}

fn op_read_write_md5(dir: &Path) {
    let src = dir.join("src.bin");
    fill_file(&src, 64, 0xAB);
    let dst = dir.join("dst.bin");
    let mut s = fs::File::open(&src).unwrap();
    let mut d = fs::File::create(&dst).unwrap();
    d.set_len(64 * 1024 * 1024).unwrap();
    let mut buf = vec![0u8; 256 * 1024];
    let mut h = Md5::new();
    loop {
        let n = s.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        d.write_all(&buf[..n]).unwrap();
    }
    d.sync_all().unwrap();
    let _ = hex::encode(h.finalize());
}

fn op_zstd_decompress(dir: &Path) {
    let raw = vec![0x42u8; 8 * 1024 * 1024];
    let comp = zstd::encode_all(&raw[..], 3).unwrap();
    let comp_path = dir.join("chunk.zst");
    fs::write(&comp_path, &comp).unwrap();
    let out = dir.join("out.bin");
    let o = fs::File::create(&out).unwrap();
    o.set_len(raw.len() as u64).unwrap();
    let cf = fs::File::open(&comp_path).unwrap();
    let br = std::io::BufReader::with_capacity(256 * 1024, cf);
    let mut dec = zstd::Decoder::new(br).unwrap();
    let mut buf = vec![0u8; 256 * 1024];
    let mut woff = 0u64;
    loop {
        let n = dec.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        o.write_all_at(&buf[..n], woff).unwrap();
        woff += n as u64;
    }
    o.sync_all().unwrap();
}

fn main() {
    let filter = std::env::args().nth(1);
    let dir = temp_dir();
    fs::create_dir_all(&dir).expect("tmp");
    let run = |name: &str, op: &dyn Fn()| {
        if filter.as_deref().is_none_or(|f| name.contains(f)) {
            measure(name, op);
        }
    };

    let dir = dir.clone();
    run("baseline", &|| {
        thread::sleep(Duration::from_millis(50));
    });

    let d = dir.clone();
    run("read_md5_128m_no_advise", &|| {
        let big = d.join("big.bin");
        fill_file(&big, 128, 0xCD);
        evict_page_cache(&big);
        op_read_md5_with_fadvise(&big, 128, false);
    });

    let d = dir.clone();
    run("read_md5_128m_fadvise", &|| {
        let big = d.join("big.bin");
        evict_page_cache(&big);
        op_read_md5_with_fadvise(&big, 128, true);
    });

    let d = dir.clone();
    run("copy_file_range_64m", &|| op_copy_file_range(&d));

    let d = dir.clone();
    run("read_write_md5_64m", &|| op_read_write_md5(&d));

    let d = dir.clone();
    run("zstd_decompress_8m", &|| op_zstd_decompress(&d));

    let d = dir.clone();
    run("assembly_e2e_8x8m", &|| op_assembly_e2e(&d));

    let _ = fs::remove_dir_all(&dir);
}
