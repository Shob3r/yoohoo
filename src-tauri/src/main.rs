// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

fn main() {
    // CEF re-execs this binary as helper processes (renderer/GPU/plugin)
    // identified by a `--type=` arg. Only tune jemalloc in the browser
    // process — helpers are short-lived and don't need decay tuning.
    if !std::env::args().any(|arg| arg.starts_with("--type=")) {
        tune_jemalloc();
    }
    elysiae_lib::run()
}

/// Configure jemalloc to return unused pages to the OS quickly, reducing
/// resident memory fragmentation during long-running downloads.
#[cfg(not(target_env = "msvc"))]
fn tune_jemalloc() {
    use tikv_jemalloc_ctl::epoch;
    let _ = epoch::advance();
    unsafe {
        let _ = tikv_jemalloc_ctl::raw::write(b"opt.dirty_decay_ms\0", 100u64);
        let _ = tikv_jemalloc_ctl::raw::write(b"opt.muzzy_decay_ms\0", 0u64);
        let _ = tikv_jemalloc_ctl::raw::write(b"opt.background_thread\0", true);
        let _ = tikv_jemalloc_ctl::raw::write(b"opt.lg_tcache_max\0", 14u64);
        let _ = tikv_jemalloc_ctl::raw::write(b"opt.narenas\0", 4u64);
    }
}

#[cfg(target_env = "msvc")]
fn tune_jemalloc() {}
