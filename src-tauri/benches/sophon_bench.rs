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

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::FileExt;
use std::sync::atomic::AtomicU32;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
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
/// chunks of `chunk_mib` MiB each, zstd-compressed on disk. When
/// `with_chunk_hashes` is true, each chunk's `chunk_decompressed_hash_md5` is
/// populated, enabling chunk-level verification and file-hash elision.
fn build_assembly_fixture(
    chunks_dir: &std::path::Path,
    chunk_mib: usize,
    num_chunks: usize,
    with_chunk_hashes: bool,
) -> (CompactManifest, Vec<u8>) {
    let mut raw = Vec::with_capacity(chunk_mib * 1024 * 1024 * num_chunks);
    let chunks: Vec<SophonManifestAssetChunk> = (0..num_chunks)
        .map(|i| {
            let name = format!("ck{i:02}");
            let data = vec![(i as u8).wrapping_mul(7).wrapping_add(0x40); chunk_mib * 1024 * 1024];
            let comp = zstd::encode_all(&data[..], 3).unwrap();
            fs::write(chunks_dir.join(chunk_filename(&name)), &comp).unwrap();
            raw.extend_from_slice(&data);
            let offset = (i * chunk_mib * 1024 * 1024) as u64;
            let chunk_hash = if with_chunk_hashes {
                hex::encode(Md5::digest(&data))
            } else {
                String::new()
            };
            SophonManifestAssetChunk {
                chunk_name: name,
                chunk_decompressed_hash_md5: chunk_hash,
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
        asset_hash_md5: file_md5,
    };
    (CompactManifest::from(vec![file]), raw)
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

    let (manifest, raw) = build_assembly_fixture(&chunks_dir, chunk_mib, num_chunks, false);

    let name_refs: Vec<&str> = (0..num_chunks)
        .map(|i| Box::leak(format!("ck{i:02}").into_boxed_str()) as &str)
        .collect();
    let lookup = ChunkNameLookup::from_arena(StringArena::from(name_refs.as_slice()));
    let refcounts: Vec<AtomicU32> = (0..num_chunks).map(|_| AtomicU32::new(1000)).collect();
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
    .expect("warmup assemble");
    let assembled = fs::read(game_dir.join("assembled.bin")).unwrap();
    assert_eq!(assembled, raw, "assemble_file bytes must match raw input");

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

    let (manifest_ch, raw_ch) = build_assembly_fixture(&chunks_dir, chunk_mib, num_chunks, true);
    let game_dir2 = dir.path().join("game_ch");
    let tmp_dir2 = dir.path().join("tmp_ch");
    fs::create_dir_all(&game_dir2).unwrap();
    fs::create_dir_all(&tmp_dir2).unwrap();
    assemble_file(
        &manifest_ch,
        0,
        &game_dir2,
        &chunks_dir,
        &tmp_dir2,
        &lookup,
        &refcounts,
        &cache,
        true,
    )
    .expect("warmup assemble (chunk hashes)");
    let assembled_ch = fs::read(game_dir2.join("assembled.bin")).unwrap();
    assert_eq!(
        assembled_ch, raw_ch,
        "assemble_file (chunk hashes) bytes must match raw input"
    );

    group.bench_function(
        BenchmarkId::new("assemble_file_chunk_verified", num_chunks),
        |b| {
            let game_dir2 = game_dir2.clone();
            let tmp_dir2 = tmp_dir2.clone();
            b.iter(|| {
                assemble_file(
                    &manifest_ch,
                    0,
                    &game_dir2,
                    &chunks_dir,
                    &tmp_dir2,
                    &lookup,
                    &refcounts,
                    &cache,
                    true,
                )
                .expect("assemble");
            });
        },
    );
    group.finish();
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
    let cache: VerificationCache<String, VerificationEntry> = VerificationCache::new();

    let mut group = c.benchmark_group("verify_file_md5");
    group.throughput(Throughput::Bytes(bytes));
    group.bench_function("miss", |b| {
        let game_dir = game_dir.clone();
        b.iter(|| {
            cache.clear();
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

const HASH_MIB: u64 = 128;

/// XXH64 vs MD5 throughput on a 128 MiB file via pread. XXH64 is the
/// compressed-chunk hash format used by some manifests.
fn bench_xxh64_vs_md5(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("hash.bin");
    fill_file(&path, HASH_MIB as usize, 0xCD);
    let bytes = HASH_MIB * 1024 * 1024;

    let mut group = c.benchmark_group("hash_128m");
    group.throughput(Throughput::Bytes(bytes));

    group.bench_function("md5_crate", |b| {
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
            let _ = h.finalize();
        });
    });

    group.bench_function("xxh64", |b| {
        b.iter(|| {
            evict_page_cache(&path);
            let f = fs::File::open(&path).unwrap();
            let mut h = xxhash_rust::xxh64::Xxh64::new(0);
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
            let _ = h.digest();
        });
    });

    group.finish();
}

const ONESHOT_MIB: u64 = 8;

/// One-shot zstd decompression (mmap input + single ZSTD_decompressDCtx) vs
/// streaming decompression (BufReader + Decoder). The one-shot path is what
/// `decompress_chunk_oneshot` uses for chunks <= 8 MiB.
fn bench_zstd_oneshot(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("tempdir");
    let raw = vec![0x42u8; ONESHOT_MIB as usize * 1024 * 1024];
    let comp = zstd::encode_all(&raw[..], 3).unwrap();
    let comp_path = dir.path().join("oneshot.zst");
    fs::write(&comp_path, &comp).unwrap();
    let out = dir.path().join("out.bin");
    let bytes = ONESHOT_MIB * 1024 * 1024;

    let mut group = c.benchmark_group("zstd_oneshot");
    group.throughput(Throughput::Bytes(bytes));

    group.bench_function("streaming", |b| {
        b.iter(|| {
            evict_page_cache(&comp_path);
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
        });
    });

    group.bench_function("oneshot_heap", |b| {
        b.iter(|| {
            let comp_data = fs::read(&comp_path).unwrap();
            let mut out_buf = vec![0u8; raw.len()];
            let mut dec = zstd::Decoder::new(comp_data.as_slice()).unwrap();
            std::io::Read::read_to_end(&mut dec, &mut out_buf).unwrap();
        });
    });

    group.finish();
}

const MANIFEST_FILES: usize = 5000;
const MANIFEST_CHUNKS_PER_FILE: usize = 50;

/// CompactManifest build from a large synthetic manifest. Measures the arena
/// interning and column vec construction cost.
fn bench_compact_manifest_build(c: &mut Criterion) {
    let hash_pool: Vec<String> = (0..MANIFEST_FILES).map(|i| format!("{i:032x}")).collect();
    let assets: Vec<SophonManifestAssetProperty> = (0..MANIFEST_FILES)
        .map(|f| {
            let chunks: Vec<SophonManifestAssetChunk> = (0..MANIFEST_CHUNKS_PER_FILE)
                .map(|c| {
                    let h = &hash_pool[(f + c) % MANIFEST_FILES];
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
            SophonManifestAssetProperty {
                asset_name: format!("asset_{f}.pak"),
                asset_chunks: chunks,
                asset_type: 0,
                asset_size: size,
                asset_hash_md5: format!("{f:032x}"),
            }
        })
        .collect();

    let mut group = c.benchmark_group("compact_manifest");
    group.throughput(Throughput::Elements(MANIFEST_FILES as u64));

    group.bench_function("build_5000x50", |b| {
        b.iter(|| {
            let assets = assets.clone();
            let manifest = CompactManifest::from(assets);
            std::hint::black_box(manifest.num_files());
        });
    });

    group.finish();
}

const LOOKUP_CHUNKS: usize = 10000;

/// ChunkNameLookup binary search vs HashMap lookup. Measures the lookup
/// throughput for the two approaches used to resolve chunk names.
fn bench_chunk_lookup(c: &mut Criterion) {
    let names: Vec<String> = (0..LOOKUP_CHUNKS)
        .map(|i| format!("chunk_{i:08}"))
        .collect();
    let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let arena = StringArena::from(name_refs.as_slice());
    let lookup = ChunkNameLookup::from_arena(arena);
    let mut hashmap: HashMap<String, usize> = HashMap::with_capacity(LOOKUP_CHUNKS);
    for (i, n) in names.iter().enumerate() {
        hashmap.insert(n.clone(), i);
    }

    let mut group = c.benchmark_group("chunk_lookup");
    group.throughput(Throughput::Elements(LOOKUP_CHUNKS as u64));

    group.bench_function("binary_search_10000", |b| {
        b.iter(|| {
            let mut found = 0usize;
            for n in &names {
                if lookup.lookup(n).is_some() {
                    found += 1;
                }
            }
            std::hint::black_box(found);
        });
    });

    group.bench_function("hashmap_10000", |b| {
        b.iter(|| {
            let mut found = 0usize;
            for n in &names {
                if hashmap.contains_key(n) {
                    found += 1;
                }
            }
            std::hint::black_box(found);
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
    bench_xxh64_vs_md5,
    bench_zstd_oneshot,
    bench_compact_manifest_build,
    bench_chunk_lookup,
);
criterion_main!(benches);
