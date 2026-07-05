use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;

use elysiae_lib::commands::sophon_downloader::SophonProgress;
use elysiae_lib::commands::sophon_downloader::game_installer::{
    self, DownloadHandle, InstallCallbacks, InstallOptions, ResumeContext,
};
use serde::{Deserialize, Serialize};
use tokio::runtime::Builder;

#[derive(Serialize, Deserialize)]
struct SavedState {
    game_id: String,
    vo_lang: String,
    game_dir: String,
    tag: String,
    manifest_hash: String,
    downloaded_chunks: HashMap<String, u64>,
}

fn state_file(game_dir: &Path) -> std::path::PathBuf {
    game_dir.with_extension(".download_state.json")
}

fn progress(p: SophonProgress) {
    let msg = match &p {
        SophonProgress::FetchingManifest => "Fetching manifest...".into(),
        SophonProgress::CalculatingDownloads {
            checked_files,
            total_files,
        } => {
            format!("Calculating: {checked_files}/{total_files}")
        }
        SophonProgress::Downloading {
            downloaded_bytes,
            total_bytes,
            speed_bps,
            eta_seconds,
        } => {
            let dl = *downloaded_bytes as f64 / (1024.0 * 1024.0);
            let total = *total_bytes as f64 / (1024.0 * 1024.0);
            let speed = *speed_bps / (1024.0 * 1024.0);
            format!("Downloading: {dl:.1}/{total:.1} MiB ({speed:.1} MiB/s, ETA {eta_seconds:.0}s)")
        }
        SophonProgress::Assembling {
            assembled_files,
            total_files,
        } => {
            format!("Assembling: {assembled_files}/{total_files}")
        }
        SophonProgress::Verifying {
            scanned_files,
            total_files,
            error_count,
        } => {
            format!("Verifying: {scanned_files}/{total_files} (errors: {error_count})")
        }
        SophonProgress::Paused {
            downloaded_bytes,
            total_bytes,
        } => {
            let dl = *downloaded_bytes as f64 / (1024.0 * 1024.0);
            let total = *total_bytes as f64 / (1024.0 * 1024.0);
            format!("Paused: {dl:.1}/{total:.1} MiB")
        }
        SophonProgress::Warning { message } => format!("Warning: {message}"),
        SophonProgress::Error { message } => format!("Error: {message}"),
        SophonProgress::Finished => "Finished!".into(),
        other => format!("{other:?}"),
    };
    eprintln!("\r{msg}\x1b[K");
}

fn main() {
    let runtime = Builder::new_multi_thread()
        .worker_threads(4)
        .max_blocking_threads(32)
        .thread_stack_size(512 * 1024)
        .enable_all()
        .build()
        .expect("build tokio runtime");
    runtime.block_on(async_main());
}

async fn async_main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        let program = &args[0];
        eprintln!("Usage: {program} <game_id> <vo_lang> <tag> <game_dir>");
        std::process::exit(1);
    }
    let game_id = &args[1];
    let vo_lang = &args[2];
    let tag = &args[3];
    let game_dir = Path::new(&args[4]);
    let sf = state_file(game_dir);

    let (prev_manifest_hash, prev_downloaded_chunks, is_resume) = if sf.exists() {
        match serde_json::from_str::<SavedState>(&std::fs::read_to_string(&sf).unwrap()) {
            Ok(saved) => {
                if saved.tag.as_str() == tag.as_str() {
                    let count = saved.downloaded_chunks.len();
                    let bytes: u64 = saved.downloaded_chunks.values().sum();
                    eprintln!(
                        "Resuming download of {game_id} version {tag} ({count} chunks, {:.1} MiB already downloaded)...",
                        bytes as f64 / (1024.0 * 1024.0)
                    );
                    (saved.manifest_hash, saved.downloaded_chunks, true)
                } else {
                    eprintln!(
                        "Tag mismatch (saved={}, requested={}), starting fresh",
                        saved.tag, tag
                    );
                    (String::new(), HashMap::new(), false)
                }
            }
            Err(err) => {
                eprintln!("Corrupt state file, starting fresh: {err}");
                (String::new(), HashMap::new(), false)
            }
        }
    } else {
        eprintln!("Downloading {game_id} version {tag}...");
        (String::new(), HashMap::new(), false)
    };

    let client = reqwest::Client::new();

    let (installers, resolved_tag, manifest_hash) =
        game_installer::build_installers_for_tag(&client, game_id, vo_lang, tag)
            .await
            .expect("build_installers_for_tag failed");

    eprintln!("Resolved tag: {resolved_tag}, manifest_hash: {manifest_hash}");

    if !is_resume && game_dir.exists() {
        eprintln!("Removing existing game directory...");
        std::fs::remove_dir_all(game_dir).expect("remove_dir_all failed");
    }

    let handle = DownloadHandle::new();

    let sf_clone = sf.clone();
    let gd_clone = game_dir.to_path_buf();
    let saved_tag = resolved_tag.clone();
    let saved_mh = manifest_hash.clone();
    let saved_gid = game_id.to_string();
    let saved_vl = vo_lang.to_string();
    let saver: game_installer::StateSaver = Arc::new(move |chunks: &HashMap<String, u64>| {
        let state = SavedState {
            game_id: saved_gid.clone(),
            vo_lang: saved_vl.clone(),
            game_dir: gd_clone.to_string_lossy().to_string(),
            tag: saved_tag.clone(),
            manifest_hash: saved_mh.clone(),
            downloaded_chunks: chunks.clone(),
        };
        let tmp = sf_clone.with_extension("json.tmp");
        if let Ok(json) = serde_json::to_string(&state)
            && std::fs::write(&tmp, &json).is_ok()
        {
            let _ = std::fs::rename(&tmp, &sf_clone);
        }
    });

    let result = game_installer::install(
        installers,
        game_dir,
        vec![],
        &resolved_tag,
        ResumeContext {
            prev_manifest_hash,
            prev_downloaded_chunks,
            resume_seed: Default::default(),
        },
        InstallOptions {
            is_preinstall: false,
            is_resume,
            handle,
        },
        InstallCallbacks {
            updater: Arc::new(progress),
            state_saver: saver,
            completion_state: Arc::new(OnceLock::new()),
        },
        game_id,
        &[vo_lang.to_string()],
    )
    .await;

    if result.is_ok() {
        let _ = std::fs::remove_file(&sf);
    }

    match result {
        Ok(()) => eprintln!("\nInstall complete!"),
        Err(err) => {
            eprintln!("\nInstall failed: {err}");
            let sf_display = sf.display();
            eprintln!("State saved to {sf_display}. Re-run to resume.");
        }
    }
}
