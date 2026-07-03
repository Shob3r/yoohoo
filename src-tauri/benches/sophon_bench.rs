//! Real measurement benchmarks for the Sophon installer hot paths.
//!
//! The metrics here drive concrete optimization decisions: copy backend
//! (`copy_file_range` vs user-space read+write), MD5 backend (openssl EVP vs
//! the md-5 crate), zstd decompress buffer sizing, end-to-end assembly
//! throughput, and post-assembly file verification. Each group sets a per-bench
//! throughput so criterion reports MiB/s.
//!
//! Run:                cargo bench
//! Run one group:      cargo bench -- io_copy
//! Save a baseline:    cargo bench -- --save-baseline before
//! Compare to baseline: cargo bench -- --baseline before

use std::fs;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::FileExt;
use std::sync::atomic::AtomicUsize;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use dashmap::DashMap;
use md5::{Digest, Md5};

use elysiae_lib::commands::sophon_downloader::game_installer::{
    assembly::{assemble_file, chunk_filename},
    cache::VerificationEntry,
    compact_manifest::{CompactManifest, StringArena},
    installer::ChunkNameLookup,
    sysio,
};
use elysiae_lib::commands::sophon_downloader::proto_parse::{
    SophonManifestAssetChunk, SophonManifestAssetProperty,
};

/// Fill a file with `mib` MiB of byte `fill`.
fn fill_file(path: &std::path::Path, mib: usize, fill: u8) {
    let block = vec![fill; 1024 * 1024];
    let mut f = fs::File::create(path).expect("create");
    for _ in 0..mib {
        f.write_all(&block).expect("write");
    }
    f.sync_all().expect("sync");
}

/// Advise the kernel to drop a file's pages so the next bench run re-faults it.
fn evict_page_cache(path: &std::path::Path) {
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    unsafe {
        let fd = libc::open(c.as_ptr(), libc::O_RDONLY);
        if fd >= 0 {
            libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_DONTNEED);
            libc::close(fd);
        }
    }
}

const COPY_MIB: u64 = 64;

/// User-space copy through a 256 KiB buffer: the assembly fallback path.
fn bench_io_copy(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src.bin");
    fill_file(&src, COPY_MIB as usize, 0xAB);
    let dst = dir.path().join("dst.bin");
    let bytes = COPY_MIB * 1024 * 1024;

    let mut group = c.benchmark_group("io_copy");
    group.throughput(Throughput::Bytes(bytes));

    group.bench_function(BenchmarkId::new("read_write_256k", COPY_MIB), |b| {
        b.iter(|| {
            evict_page_cache(&src);
            let mut s = fs::File::open(&src).unwrap();
            let mut d = fs::File::create(&dst).unwrap();
            d.set_len(bytes).unwrap();
            let mut buf = vec![0u8; 256 * 1024];
            loop {
                let n = s.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                d.write_all(&buf[..n]).unwrap();
            }
            d.sync_all().unwrap();
        });
    });

    group.bench_function(BenchmarkId::new("copy_file_range", COPY_MIB), |b| {
        b.iter(|| {
            evict_page_cache(&src);
            let d = fs::File::create(&dst).unwrap();
            d.set_len(bytes).unwrap();
            sysio::copy_file_region_to(&src, 0, &d, 0, bytes).unwrap();
            d.sync_all().unwrap();
        });
    });

    group.bench_function(BenchmarkId::new("read_write_md5", COPY_MIB), |b| {
        b.iter(|| {
            evict_page_cache(&src);
            let mut s = fs::File::open(&src).unwrap();
            let mut d = fs::File::create(&dst).unwrap();
            d.set_len(bytes).unwrap();
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
            let _ = h.finalize();
        });
    });

    // `pread` + `pwrite` + MD5 is the actual assembly hot path (`write_all_at`).
    group.bench_function(BenchmarkId::new("pread_pwrite_md5", COPY_MIB), |b| {
        b.iter(|| {
            evict_page_cache(&src);
            let s = fs::File::open(&src).unwrap();
            let d = fs::File::create(&dst).unwrap();
            d.set_len(bytes).unwrap();
            let mut buf = vec![0u8; 256 * 1024];
            let mut h = Md5::new();
            let mut off = 0u64;
            loop {
                let n = s.read_at(&mut buf, off).unwrap();
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
                d.write_all_at(&buf[..n], off).unwrap();
                off += n as u64;
            }
            d.sync_all().unwrap();
            let _ = h.finalize();
        });
    });

    group.finish();
}

const MD5_MIB: u64 = 50;

/// MD5 of a 50 MiB file via the `md-5` crate (software compression) vs openssl
/// EVP (libcrypto, hardware-accelerated on x86_64). Directly comparable since
/// both share the throughput and the source file is identical.
fn bench_md5(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("md5.bin");
    fill_file(&path, MD5_MIB as usize, 0xAB);
    let bytes = MD5_MIB * 1024 * 1024;

    let mut group = c.benchmark_group("md5");
    group.throughput(Throughput::Bytes(bytes));

    group.bench_function(BenchmarkId::new("crate", MD5_MIB), |b| {
        b.iter(|| {
            evict_page_cache(&path);
            let f = fs::File::open(&path).unwrap();
            let mut h = Md5::new();
            let mut buf = vec![0u8; 256 * 1024];
            let mut off = 0u64;
            loop {
                let n = f.read_at(&mut buf, off).unwrap();
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
                off += n as u64;
            }
            let _ = hex::encode(h.finalize());
        });
    });

    group.bench_function(BenchmarkId::new("openssl_evp", MD5_MIB), |b| {
        b.iter(|| {
            evict_page_cache(&path);
            let f = fs::File::open(&path).unwrap();
            let mut hasher =
                openssl::hash::Hasher::new(openssl::hash::MessageDigest::md5()).unwrap();
            let mut buf = vec![0u8; 256 * 1024];
            let mut off = 0u64;
            loop {
                let n = f.read_at(&mut buf, off).unwrap();
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]).unwrap();
                off += n as u64;
            }
            let _ = hex::encode(hasher.finish().unwrap());
        });
    });

    group.finish();
}

const ZSTD_MIB: u64 = 8;

/// zstd decompression throughput with 64/128/256 KiB output buffers. Picks the
/// buffer size `assembly_opt.rs` streams into.
fn bench_zstd_decompress(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");
    let raw_path = dir.path().join("raw.bin");
    fill_file(&raw_path, ZSTD_MIB as usize, 0x42);
    let raw = fs::read(&raw_path).unwrap();
    let comp_path = dir.path().join("chunk.zst");
    {
        let f = fs::File::create(&comp_path).unwrap();
        let mut enc = zstd::Encoder::new(f, 3).unwrap();
        std::io::Write::write_all(&mut enc, &raw).unwrap();
        enc.finish().unwrap();
    }
    let out = dir.path().join("out.bin");
    let bytes = ZSTD_MIB * 1024 * 1024;

    let mut group = c.benchmark_group("zstd_decompress");
    group.throughput(Throughput::Bytes(bytes));

    for buf_kib in [64usize, 128, 256] {
        let buf_bytes = buf_kib * 1024;
        group.bench_function(BenchmarkId::new("buf", buf_kib), |b| {
            b.iter(|| {
                evict_page_cache(&comp_path);
                let o = fs::File::create(&out).unwrap();
                o.set_len(raw.len() as u64).unwrap();
                let cf = fs::File::open(&comp_path).unwrap();
                let br = std::io::BufReader::with_capacity(buf_bytes, cf);
                let mut dec = zstd::Decoder::new(br).unwrap();
                let mut buf = vec![0u8; buf_bytes];
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
            });
        });
    }
    group.finish();
}

/// Build a single-file `CompactManifest` over `num_chunks` equal decompressed
/// chunks of `chunk_mib` MiB each, zstd-compressed on disk. Returns the
/// manifest, the raw assembled bytes, and the expected MD5.
fn build_assembly_fixture(
    chunks_dir: &std::path::Path,
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

/// End-to-end `assemble_file` throughput: chunk reads, zstd decompress, pwrite,
/// file-level MD5, sync, and rename. This is what the user actually waits on.
fn bench_assembly_e2e(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");
    let game_dir = dir.path().join("game");
    let chunks_dir = dir.path().join("chunks");
    let tmp_dir = dir.path().join("tmp");
    fs::create_dir_all(&game_dir).unwrap();
    fs::create_dir_all(&chunks_dir).unwrap();
    fs::create_dir_all(&tmp_dir).unwrap();

    let chunk_mib = 8;
    let num_chunks = 8;
    let total_bytes = (chunk_mib * num_chunks) as u64 * 1024 * 1024;
    let (manifest, raw, _md5) = build_assembly_fixture(&chunks_dir, chunk_mib, num_chunks);

    let name_refs: Vec<&str> = (0..num_chunks)
        .map(|i| Box::leak(format!("ck{i:02}").into_boxed_str()) as &str)
        .collect();
    let lookup = ChunkNameLookup::from_arena(StringArena::from(name_refs.as_slice()));
    // High refcount so chunk files survive across bench iterations.
    let refcounts: Vec<AtomicUsize> = (0..num_chunks).map(|_| AtomicUsize::new(1000)).collect();
    let cache: DashMap<String, VerificationEntry> = DashMap::new();

    let mut group = c.benchmark_group("assembly_e2e");
    group.throughput(Throughput::Bytes(total_bytes));
    group.bench_function(BenchmarkId::new("assemble_file", num_chunks), |b| {
        let game_dir = game_dir.clone();
        let tmp_dir = tmp_dir.clone();
        b.iter(|| {
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
        });
    });
    group.finish();

    // Correctness: the assembled file must match the expected bytes. Runs once
    // after timing so the bench doubles as a smoke test.
    let assembled = fs::read(game_dir.join("assembled.bin")).unwrap();
    assert_eq!(assembled, raw, "assembled file bytes must match");
}

const VERIFY_MIB: u64 = 50;

/// `check_file_md5_cached` cache-miss path: full read + MD5 of a 50 MiB file.
fn bench_verify_file_md5(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");
    let game_dir = dir.path().to_path_buf();
    let path = game_dir.join("verify.bin");
    fill_file(&path, VERIFY_MIB as usize, 0xAB);
    let md5 = hex::encode(Md5::digest(fs::read(&path).unwrap()));
    let bytes = VERIFY_MIB * 1024 * 1024;
    let cache: DashMap<String, VerificationEntry> = DashMap::new();

    let mut group = c.benchmark_group("verify_file_md5");
    group.throughput(Throughput::Bytes(bytes));
    group.bench_function("miss", |b| {
        let game_dir = game_dir.clone();
        b.iter(|| {
            evict_page_cache(&path);
            let _ =
                elysiae_lib::commands::sophon_downloader::game_installer::cache::check_file_md5_cached(
                    &path,
                    bytes,
                    &md5,
                    &game_dir,
                    &cache,
                )
                .unwrap();
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_io_copy,
    bench_md5,
    bench_zstd_decompress,
    bench_assembly_e2e,
    bench_verify_file_md5,
);
criterion_main!(benches);
