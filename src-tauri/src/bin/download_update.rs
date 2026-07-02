use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use elysiae_lib::commands::sophon_downloader::SophonProgress;
use elysiae_lib::commands::sophon_downloader::game_installer;

fn progress(p: SophonProgress) {
    let msg = match &p {
        SophonProgress::FetchingManifest => "Fetching manifest...".into(),
        SophonProgress::CalculatingDownloads {
            checked_files,
            total_files,
        } => format!("Calculating: {checked_files}/{total_files}"),
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
        } => format!("Assembling: {assembled_files}/{total_files}"),
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
    let game_id = "hk4e";
    let vo_lang = "en";
    let home = std::env::var("HOME").expect("HOME");
    let game_dir = format!("{home}/.local/share/app.elysiae.Elysiae/games/hk4e");
    let game_dir = Path::new(&game_dir);

    let current_tag = std::fs::read_to_string(game_dir.join(".sophon_version"))
        .expect("no .sophon_version")
        .trim()
        .to_string();
    eprintln!("Current tag: {current_tag}");

    let client = reqwest::Client::new();

    eprintln!("Building update installers (from {current_tag} to latest)...");
    let (installers, deleted_files, new_tag, _manifest_hash) =
        game_installer::build_update_installers(&client, game_id, vo_lang, &current_tag, game_dir)
            .await
            .expect("build_update_installers failed");

    eprintln!("Update: {current_tag} -> {new_tag}");
    eprintln!("Installers: {}", installers.len());
    eprintln!("Deleted files: {}", deleted_files.len());

    for (i, inst) in installers.iter().enumerate() {
        let files = inst.manifest.assets.len();
        let chunks: usize = inst
            .manifest
            .assets
            .iter()
            .map(|a| a.asset_chunks.len())
            .sum();
        let new_chunks: usize = inst
            .manifest
            .assets
            .iter()
            .flat_map(|a| a.asset_chunks.iter())
            .filter(|c| c.chunk_old_offset < 0)
            .count();
        let reused_chunks: usize = inst
            .manifest
            .assets
            .iter()
            .flat_map(|a| a.asset_chunks.iter())
            .filter(|c| c.chunk_old_offset >= 0)
            .count();
        let new_bytes: u64 = inst
            .manifest
            .assets
            .iter()
            .flat_map(|a| a.asset_chunks.iter())
            .filter(|c| c.chunk_old_offset < 0)
            .map(|c| c.chunk_size)
            .sum();
        eprintln!(
            "  installer[{i}]: label={}, field={}, files={}, chunks={}, new={}, reused={}, new_bytes={}",
            inst.label, inst.matching_field, files, chunks, new_chunks, reused_chunks, new_bytes
        );
    }

    let handle = game_installer::DownloadHandle::new();
    let completed_files: Arc<std::sync::Mutex<HashSet<String>>> =
        Arc::new(std::sync::Mutex::new(HashSet::new()));

    let saver: game_installer::StateSaver = Arc::new(move |_chunks: &HashMap<String, u64>| {});
    let gd = game_dir.to_path_buf();
    let vo_langs = vec![vo_lang.to_string()];

    let result = game_installer::install(
        installers,
        &gd,
        deleted_files,
        &new_tag,
        game_installer::ResumeContext {
            prev_manifest_hash: String::new(),
            prev_downloaded_chunks: HashMap::new(),
        },
        game_installer::InstallOptions {
            is_preinstall: false,
            is_resume: false,
            handle,
        },
        game_installer::InstallCallbacks {
            updater: Arc::new(progress),
            state_saver: saver,
            completed_files,
        },
        game_id,
        &vo_langs,
    )
    .await;

    match result {
        Ok(()) => eprintln!("\nUpdate complete!"),
        Err(err) => eprintln!("\nUpdate failed: {err}"),
    }
}
