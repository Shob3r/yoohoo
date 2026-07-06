//! Peak RSS sampler for the Sophon installer hot paths.
//!
//! Resident-set size is a one-shot peak metric, not an iterative timing, so it
//! does not fit the criterion statistical model. This binary polls
//! `/proc/self/statm` from a background thread while each operation runs and
//! reports peak-vs-baseline RSS.
//!
//! Run:  cargo run --release --features benchmark --bin bench_memory -- [op]
//! where [op] is a substring of one of the operations below; runs all if none.

use std::collections::HashMap;
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
    installer::{ChunkNameLookup, intern_old_chunk_offsets},
    sysio,
};
use elysiae_lib::commands::sophon_downloader::proto_parse::{
    SophonManifestAssetChunk, SophonManifestAssetProperty, SophonManifestProto,
};

/// RSS poll interval. 2 ms catches short allocation spikes without measurable
/// load.
const SAMPLE_MS: u64 = 2;

/// Duration the diff ops hold their peak allocation so the sampler observes it
/// before the values drop; without it the allocation completes and frees
/// within a single poll window.
const PEAK_HOLD: Duration = Duration::from_millis(40);

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

/// Run `setup`, record baseline RSS, then run `op` under the sampler. The
/// delta reflects only `op`'s allocations, not the fixture data from `setup`.
fn measure_with_setup<T, S, O>(label: &str, setup: S, op: O)
where
    S: FnOnce() -> T,
    O: FnOnce(T),
{
    let context = setup();
    unsafe {
        libc::malloc_trim(0);
    }
    let baseline = rss_kib();
    let sampler = RssSampler::start();
    let t = Instant::now();
    op(context);
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

/// Create chunk files on disk and build the manifest without retaining the raw
/// assembled bytes in memory. Drops all intermediate data and trims the
/// allocator so RSS reflects only what assembly itself allocates.
fn setup_assembly_fixture(
    chunks_dir: &Path,
    chunk_size: u64,
    num_chunks: usize,
    with_chunk_hashes: bool,
) -> CompactManifest {
    let chunks: Vec<SophonManifestAssetChunk> = (0..num_chunks)
        .map(|i| {
            let name = format!("ck{i:02}");
            let data = vec![(i as u8).wrapping_mul(7).wrapping_add(0x40); chunk_size as usize];
            let comp = zstd::encode_all(&data[..], 3).unwrap();
            fs::write(chunks_dir.join(chunk_filename(&name)), &comp).unwrap();
            let offset = (i as u64) * chunk_size;
            let chunk_hash = if with_chunk_hashes {
                hex::encode(Md5::digest(&data))
            } else {
                String::new()
            };
            drop(data);
            drop(comp);
            SophonManifestAssetChunk {
                chunk_name: name,
                chunk_decompressed_hash_md5: chunk_hash,
                chunk_on_file_offset: offset,
                chunk_size: 0,
                chunk_size_decompressed: chunk_size,
                chunk_compressed_hash_xxh: 0,
                chunk_compressed_hash_md5: String::new(),
                chunk_old_offset: -1,
            }
        })
        .collect();
    let file = SophonManifestAssetProperty {
        asset_name: "assembled.bin".to_string(),
        asset_chunks: chunks,
        asset_type: 0,
        asset_size: chunk_size * num_chunks as u64,
        asset_hash_md5: String::new(),
    };
    let manifest = CompactManifest::from(vec![file]);
    // Return freed memory to the OS so it doesn't inflate the assembly
    // RSS measurement.
    unsafe {
        libc::malloc_trim(0);
    }
    manifest
}

fn temp_dir() -> PathBuf {
    std::path::PathBuf::from("/var/tmp").join(format!("bench_memory_{}", std::process::id()))
}

fn op_assembly_e2e(dir: &Path) {
    let game_dir = dir.join("game");
    let chunks_dir = dir.join("chunks");
    let tmp_dir = dir.join("tmp");
    fs::create_dir_all(&game_dir).unwrap();
    fs::create_dir_all(&chunks_dir).unwrap();
    fs::create_dir_all(&tmp_dir).unwrap();
    let manifest = setup_assembly_fixture(&chunks_dir, 8 * 1024 * 1024, 8, false);
    let name_refs: Vec<&str> = (0..8)
        .map(|i| Box::leak(format!("ck{i:02}").into_boxed_str()) as &str)
        .collect();
    let lookup = ChunkNameLookup::from_arena(StringArena::from(name_refs.as_slice()));
    let refcounts: Vec<std::sync::atomic::AtomicU32> = (0..8)
        .map(|_| std::sync::atomic::AtomicU32::new(1000))
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

fn op_assembly_e2e_parallel(dir: &Path) {
    let game_dir = dir.join("game_p");
    let chunks_dir = dir.join("chunks_p");
    let tmp_dir = dir.join("tmp_p");
    fs::create_dir_all(&game_dir).unwrap();
    fs::create_dir_all(&chunks_dir).unwrap();
    fs::create_dir_all(&tmp_dir).unwrap();
    let manifest = setup_assembly_fixture(&chunks_dir, 8 * 1024 * 1024, 8, true);
    let name_refs: Vec<&str> = (0..8)
        .map(|i| Box::leak(format!("ck{i:02}").into_boxed_str()) as &str)
        .collect();
    let lookup = ChunkNameLookup::from_arena(StringArena::from(name_refs.as_slice()));
    let refcounts: Vec<std::sync::atomic::AtomicU32> = (0..8)
        .map(|_| std::sync::atomic::AtomicU32::new(1000))
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

fn op_assembly_e2e_small(dir: &Path) {
    let game_dir = dir.join("game_s");
    let chunks_dir = dir.join("chunks_s");
    let tmp_dir = dir.join("tmp_s");
    fs::create_dir_all(&game_dir).unwrap();
    fs::create_dir_all(&chunks_dir).unwrap();
    fs::create_dir_all(&tmp_dir).unwrap();
    let manifest = setup_assembly_fixture(&chunks_dir, 256 * 1024, 32, false);
    let name_refs: Vec<&str> = (0..32)
        .map(|i| Box::leak(format!("ck{i:02}").into_boxed_str()) as &str)
        .collect();
    let lookup = ChunkNameLookup::from_arena(StringArena::from(name_refs.as_slice()));
    let refcounts: Vec<std::sync::atomic::AtomicU32> = (0..32)
        .map(|_| std::sync::atomic::AtomicU32::new(1000))
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

/// Build a synthetic manifest with `num_files` non-directory assets, each
/// carrying `chunks_per_file` chunks. Chunk hashes cycle through a pool of
/// `num_files` unique strings so interning deduplicates them. Asset names
/// are unique per file. The caller is responsible for trimming the allocator
/// after construction if measuring RSS of a subsequent operation.
fn build_synthetic_manifest(num_files: usize, chunks_per_file: usize) -> SophonManifestProto {
    let hash_pool: Vec<String> = (0..num_files).map(|i| format!("{i:032x}")).collect();
    let assets: Vec<SophonManifestAssetProperty> = (0..num_files)
        .map(|f| {
            let chunks: Vec<SophonManifestAssetChunk> = (0..chunks_per_file)
                .map(|c| {
                    let h = &hash_pool[(f + c) % num_files];
                    SophonManifestAssetChunk {
                        chunk_name: format!("f{f}c{c}"),
                        chunk_decompressed_hash_md5: h.clone(),
                        chunk_on_file_offset: (c * 1024 * 1024) as u64,
                        chunk_size: 0,
                        chunk_size_decompressed: 0,
                        chunk_compressed_hash_xxh: 0,
                        chunk_compressed_hash_md5: String::new(),
                        chunk_old_offset: -1,
                    }
                })
                .collect();
            let size = chunks.iter().map(|c| c.chunk_size_decompressed).sum();
            let mut hash = format!("{f:032x}");
            hash.push_str("00");
            SophonManifestAssetProperty {
                asset_name: format!("asset_{f}.pak"),
                asset_chunks: chunks,
                asset_type: 0,
                asset_size: size,
                asset_hash_md5: hash,
            }
        })
        .collect();
    SophonManifestProto { assets }
}

/// Replicate the pre-interning approach: `HashMap<(String, String), u64>`
/// keyed by cloned asset names and chunk hashes.
fn build_string_key_offsets(manifest: &SophonManifestProto) -> HashMap<(String, String), u64> {
    manifest
        .assets
        .iter()
        .filter(|f| !f.is_directory())
        .flat_map(|f| {
            let name = f.asset_name.clone();
            f.asset_chunks.iter().map(move |c| {
                (
                    (name.clone(), c.chunk_decompressed_hash_md5.clone()),
                    c.chunk_on_file_offset,
                )
            })
        })
        .collect()
}

fn op_diff_interned_keys(manifest: SophonManifestProto) {
    let (offsets, name_to_id, hash_to_id) = intern_old_chunk_offsets(&manifest);
    std::hint::black_box(&offsets);
    std::hint::black_box(&name_to_id);
    std::hint::black_box(&hash_to_id);
    std::hint::black_box(&manifest);
    thread::sleep(PEAK_HOLD);
}

fn op_diff_string_keys(manifest: SophonManifestProto) {
    let offsets = build_string_key_offsets(&manifest);
    std::hint::black_box(&offsets);
    std::hint::black_box(&manifest);
    thread::sleep(PEAK_HOLD);
}

/// Diagnostic: allocate a known size so the RSS delta proves the sampler works.
fn op_alloc_50mb() {
    let v: Vec<u8> = vec![0xFF; 50 * 1024 * 1024];
    let probe = v.iter().copied().fold(0u64, |a, b| a ^ b as u64);
    std::hint::black_box(probe);
    thread::sleep(PEAK_HOLD);
    drop(v);
}

/// mmap a 128 MiB file and MD5 it in one pass. Compares against the pread path.
fn op_mmap_md5_128m(dir: &Path) {
    let big = dir.join("mmap_md5.bin");
    fill_file(&big, 128, 0xCD);
    evict_page_cache(&big);
    let f = fs::File::open(&big).expect("open");
    let len = f.metadata().unwrap().len() as usize;
    let mmap = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            f.as_raw_fd(),
            0,
        )
    };
    if mmap == libc::MAP_FAILED {
        panic!("mmap failed");
    }
    let mut h = Md5::new();
    h.update(unsafe { std::slice::from_raw_parts(mmap as *const u8, len) });
    let _ = hex::encode(h.finalize());
    unsafe {
        libc::munmap(mmap, len);
    }
}

/// Streaming MD5 of 128 MiB via file_md5_digest (BufReader + fadvise).
fn op_stream_md5_128m(dir: &Path) {
    let big = dir.join("stream_md5.bin");
    fill_file(&big, 128, 0xCD);
    evict_page_cache(&big);
    let _ = elysiae_lib::commands::sophon_downloader::game_installer::cache::file_md5_digest(&big);
}

/// XXH64 of 128 MiB via pread. Compares throughput and RSS against MD5.
fn op_xxh64_128m(dir: &Path) {
    let big = dir.join("xxh64.bin");
    fill_file(&big, 128, 0xCD);
    evict_page_cache(&big);
    let f = fs::File::open(&big).expect("open");
    let mut buf = vec![0u8; 256 * 1024];
    let mut h = xxhash_rust::xxh64::Xxh64::new(0);
    let mut off = 0u64;
    let total: u64 = 128 * 1024 * 1024;
    while off < total {
        let n = f.read_at(&mut buf, off).expect("read");
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        off += n as u64;
    }
    let _ = h.digest();
}

/// One-shot zstd decompression of an 8 MiB frame into a heap buffer. Measures
/// the peak RSS of the single-pass path vs streaming.
fn op_zstd_oneshot_8m(dir: &Path) {
    let raw = vec![0x42u8; 8 * 1024 * 1024];
    let comp = zstd::encode_all(&raw[..], 3).unwrap();
    let comp_path = dir.join("oneshot.zst");
    fs::write(&comp_path, &comp).unwrap();
    drop(raw);
    drop(comp);
    unsafe {
        libc::malloc_trim(0);
    }
    let cf = fs::File::open(&comp_path).expect("open");
    let comp_size = cf.metadata().unwrap().len() as usize;
    let comp_mmap = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            comp_size,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            cf.as_raw_fd(),
            0,
        )
    };
    if comp_mmap == libc::MAP_FAILED {
        panic!("mmap failed");
    }
    let comp_slice = unsafe { std::slice::from_raw_parts(comp_mmap as *const u8, comp_size) };

    let out_path = dir.join("oneshot_out.bin");
    let of = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&out_path)
        .unwrap();
    of.set_len(8 * 1024 * 1024).unwrap();
    let out_mmap = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            8 * 1024 * 1024,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            of.as_raw_fd(),
            0,
        )
    };
    if out_mmap == libc::MAP_FAILED {
        panic!("out mmap failed");
    }
    let out_slice = unsafe { std::slice::from_raw_parts_mut(out_mmap as *mut u8, 8 * 1024 * 1024) };

    let mut ctx = zstd::zstd_safe::DCtx::create();
    let written = ctx.decompress(out_slice, comp_slice).expect("decompress");
    std::hint::black_box(written);

    unsafe {
        libc::munmap(comp_mmap, comp_size);
        libc::munmap(out_mmap, 8 * 1024 * 1024);
    }
}

/// Build a CompactManifest from a large synthetic manifest. Measures the arena
/// and column vec allocations.
fn op_compact_manifest_build_with_assets(assets: Vec<SophonManifestAssetProperty>) {
    let compact = CompactManifest::from(assets);
    std::hint::black_box(compact.num_files());
    std::hint::black_box(compact.num_chunks());
    thread::sleep(PEAK_HOLD);
}

/// Fill a verification cache to the cap. Measures the DashMap's resident
/// memory.
fn op_verify_cache_fill(num_entries: usize) {
    let cache: VerificationCache<String, VerificationEntry> = VerificationCache::new();
    for i in 0..num_entries {
        let key = format!("asset_{i}.pak");
        cache.insert(
            key,
            VerificationEntry {
                size: 1024,
                md5: format!("{i:032x}"),
                mtime_secs: 0,
            },
        );
    }
    std::hint::black_box(cache.len());
    thread::sleep(PEAK_HOLD);
}

/// Build a ChunkNameLookup from a large arena. Measures the sort + index vec.
fn op_chunk_name_lookup(num_chunks: usize) {
    let names: Vec<String> = (0..num_chunks).map(|i| format!("chunk_{i:08}")).collect();
    let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let arena = StringArena::from(name_refs.as_slice());
    let lookup = ChunkNameLookup::from_arena(arena);
    std::hint::black_box(lookup.get(0));
    thread::sleep(PEAK_HOLD);
}

/// Build the download_items_index HashMap used during download state setup.
/// Mirrors the per-chunk `&str -> usize` map that locates duplicate chunks.
fn op_download_items_index(num_chunks: usize) {
    let names: Vec<String> = (0..num_chunks).map(|i| format!("chunk_{i:08}")).collect();
    let mut map: HashMap<&str, usize> = HashMap::with_capacity(num_chunks);
    for (i, n) in names.iter().enumerate() {
        map.insert(n.as_str(), i);
    }
    std::hint::black_box(map.len());
    thread::sleep(PEAK_HOLD);
}

/// Measure streaming-decoder RSS for a zstd frame at a given WindowLogMax cap.
/// Reports whether the cap actually pre-allocates the full window or only
/// allocates what the frame declares.
fn op_zstd_decoder_rss(frame_bytes: usize, window_log_max: u32) {
    let data: Vec<u8> = (0..frame_bytes).map(|i| (i % 251) as u8).collect();
    let comp = zstd::encode_all(&data[..], 3).unwrap();
    let mut dec = zstd::stream::read::Decoder::new(comp.as_slice()).unwrap();
    dec.set_parameter(zstd::zstd_safe::DParameter::WindowLogMax(window_log_max))
        .unwrap();
    let mut out = Vec::with_capacity(data.len());
    std::io::copy(&mut dec, &mut out).unwrap();
    std::hint::black_box(out.len());
    thread::sleep(PEAK_HOLD);
    drop(out);
    drop(dec);
    drop(comp);
    drop(data);
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

    let d = dir.clone();
    run("assembly_e2e_parallel_8x8m", &|| {
        op_assembly_e2e_parallel(&d)
    });

    let d = dir.clone();
    run("assembly_e2e_32x256k", &|| op_assembly_e2e_small(&d));

    let d = dir.clone();
    run("mmap_md5_128m", &|| op_mmap_md5_128m(&d));

    let d = dir.clone();
    run("stream_md5_128m", &|| op_stream_md5_128m(&d));

    let d = dir.clone();
    run("xxh64_128m", &|| op_xxh64_128m(&d));

    let d = dir.clone();
    run("zstd_oneshot_8m", &|| op_zstd_oneshot_8m(&d));

    if filter
        .as_deref()
        .is_none_or(|f| "compact_manifest_build_5000x50".contains(f))
    {
        measure_with_setup(
            "compact_manifest_build_5000x50",
            || build_synthetic_manifest(5000, 50).assets,
            op_compact_manifest_build_with_assets,
        );
    }

    if filter
        .as_deref()
        .is_none_or(|f| "verify_cache_fill_5000".contains(f))
    {
        run("verify_cache_fill_5000", &|| op_verify_cache_fill(5000));
    }

    if filter
        .as_deref()
        .is_none_or(|f| "chunk_name_lookup_10000".contains(f))
    {
        run("chunk_name_lookup_10000", &|| op_chunk_name_lookup(10000));
    }

    if filter
        .as_deref()
        .is_none_or(|f| "download_items_index_10000".contains(f))
    {
        run("download_items_index_10000", &|| {
            op_download_items_index(10000)
        });
    }

    // Diagnostic runs only with an explicit filter so a 50 MiB allocation does
    // not inflate the baseline of subsequent ops on a full run.
    if filter
        .as_deref()
        .is_some_and(|f| "alloc_50mb_diag".contains(f))
    {
        run("alloc_50mb_diag", &|| op_alloc_50mb());
    }

    if filter
        .as_deref()
        .is_some_and(|f| "zstd_decoder_rss".contains(f))
    {
        for &(frame_bytes, wl) in &[
            (256 * 1024usize, 26u32),
            (256 * 1024, 19),
            (1024 * 1024, 26),
            (1024 * 1024, 23),
            (1024 * 1024, 21),
            (8 * 1024 * 1024, 26),
            (8 * 1024 * 1024, 24),
            (8 * 1024 * 1024, 23),
            (32 * 1024 * 1024, 26),
            (32 * 1024 * 1024, 23),
        ] {
            let size_label = if frame_bytes >= 1024 * 1024 {
                format!("{}MiB", frame_bytes / (1024 * 1024))
            } else {
                format!("{}KiB", frame_bytes / 1024)
            };
            let label = format!("zstd_decoder_rss_{size_label}_wl{wl}");
            run(&label, &|| op_zstd_decoder_rss(frame_bytes, wl));
        }
    }

    if filter
        .as_deref()
        .is_none_or(|f| "diff_interned_keys_2000x100".contains(f))
    {
        measure_with_setup(
            "diff_interned_keys_2000x100",
            || build_synthetic_manifest(2000, 100),
            op_diff_interned_keys,
        );
    }

    if filter
        .as_deref()
        .is_none_or(|f| "diff_string_keys_2000x100".contains(f))
    {
        measure_with_setup(
            "diff_string_keys_2000x100",
            || build_synthetic_manifest(2000, 100),
            op_diff_string_keys,
        );
    }

    let _ = fs::remove_dir_all(&dir);
}
