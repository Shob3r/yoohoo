use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use elysiae_lib::commands::sophon_downloader::SophonProgress;
use elysiae_lib::commands::sophon_downloader::game_installer::{self, DownloadHandle};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct SavedState {
    game_id: String,
    vo_lang: String,
    game_dir: String,
    tag: String,
    downloaded_chunks: HashSet<String>,
}

fn state_file(game_dir: &Path) -> std::path::PathBuf {
    game_dir.with_extension(".preinstall_state.json")
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
            let speed = *speed_bps as f64 / (1024.0 * 1024.0);
            format!("Downloading: {dl:.1}/{total:.1} MiB ({speed:.1} MiB/s, ETA {eta_seconds:.0}s)")
        }
        SophonProgress::Assembling {
            assembled_files,
            total_files,
        } => {
            format!("Assembling: {assembled_files}/{total_files}")
        }
        SophonProgress::ApplyingPreinstall {
            applied_files,
            total_files,
        } => {
            format!("Applying: {applied_files}/{total_files}")
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

#[tokio::main]
async fn main() {
    let game_id = std::env::args().nth(1).unwrap_or_else(|| "hk4e".into());
    let vo_lang = std::env::args().nth(2).unwrap_or_else(|| "en".into());
    let home = std::env::var("HOME").expect("HOME not set");
    let game_dir = format!("{home}/.local/share/app.elysiae.Elysiae/games/{game_id}");
    let game_dir = Path::new(&game_dir);
    let sf = state_file(game_dir);

    let client = reqwest::Client::new();
    let handle = DownloadHandle::new();

    eprintln!("Building preinstall plan for {game_id}...");

    let plan = game_installer::build_preinstall_plan(&client, &game_id, &vo_lang, game_dir)
        .await
        .expect("build_preinstall_plan failed");

    let tag = plan.tag.clone();
    eprintln!("Preinstall tag: {tag}");
    eprintln!(
        "Unique chunks: {}, total size: {:.1} MiB",
        plan.unique_chunks.len(),
        plan.unique_chunks
            .iter()
            .map(|c| c.patch_size)
            .fold(0u64, |acc, x| acc.saturating_add(x)) as f64
            / (1024.0 * 1024.0)
    );

    let (resume_chunks, _is_resume) = if sf.exists() {
        match serde_json::from_str::<SavedState>(&std::fs::read_to_string(&sf).unwrap()) {
            Ok(saved) => {
                if saved.tag == tag {
                    let count = saved.downloaded_chunks.len();
                    eprintln!("Resuming preinstall ({count} chunks already downloaded)...");
                    (
                        saved
                            .downloaded_chunks
                            .into_iter()
                            .map(|name| (name, 0u64))
                            .collect(),
                        true,
                    )
                } else {
                    eprintln!(
                        "Tag mismatch (saved={}, requested={}), starting fresh",
                        saved.tag, tag
                    );
                    (HashMap::new(), false)
                }
            }
            Err(err) => {
                eprintln!("Corrupt state file, starting fresh: {err}");
                (HashMap::new(), false)
            }
        }
    } else {
        eprintln!("Starting preinstall download...");
        (HashMap::new(), false)
    };

    let sf_clone = sf.clone();
    let gd_clone = game_dir.to_path_buf();
    let saved_tag = tag.clone();
    let saved_gid = game_id.to_string();
    let saved_vl = vo_lang.to_string();
    let saver: game_installer::StateSaver = Arc::new(move |chunks: &HashMap<String, u64>| {
        let state = SavedState {
            game_id: saved_gid.clone(),
            vo_lang: saved_vl.clone(),
            game_dir: gd_clone.to_string_lossy().to_string(),
            tag: saved_tag.clone(),
            downloaded_chunks: chunks.keys().cloned().collect(),
        };
        let tmp = sf_clone.with_extension("json.tmp");
        if let Ok(json) = serde_json::to_string(&state) {
            if std::fs::write(&tmp, &json).is_ok() {
                let _ = std::fs::rename(&tmp, &sf_clone);
            }
        }
    });

    let result = game_installer::preinstall_download(
        &client,
        &plan,
        game_dir,
        &game_id,
        &vo_lang,
        handle,
        Arc::new(progress),
        saver,
        resume_chunks,
    )
    .await;

    match result {
        Ok(preinstall_state) => {
            let _ = std::fs::remove_file(&sf);
            eprintln!(
                "\nPreinstall download complete for tag {}",
                preinstall_state.tag
            );
        }
        Err(err) => {
            eprintln!("\nPreinstall download failed: {err}");
            eprintln!("State saved to {}. Re-run to resume.", sf.display());
        }
    }
}
