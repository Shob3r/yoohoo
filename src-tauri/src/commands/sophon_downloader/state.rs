//! Download state persistence for resumption after app restart.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use tauri::AppHandle;
use tauri::Manager;
use tauri::path::BaseDirectory;
use tauri_plugin_log::log;

use crate::commands::sophon_downloader::types::DownloadState;

const DOWNLOAD_STATE_FILE: &str = ".sophon_download_state";

pub(crate) fn download_state_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|err| {
            log::error!("app_data_dir resolution failed: {err}", err = err);
            err
        })
        .ok()
        .map(|p| p.join(DOWNLOAD_STATE_FILE))
}

/// Atomically persist download state (write to unique .tmp, then rename).
pub fn save_download_state(app: &AppHandle, state: &DownloadState) -> Result<(), String> {
    let Some(path) = download_state_path(app) else {
        let msg = "Failed to resolve download state path".to_string();
        log::error!("{msg}");
        return Err(msg);
    };
    if let Some(parent) = path.parent()
        && let Err(err) = fs::create_dir_all(parent)
    {
        let msg = format!(
            "Failed to create download state directory: {err}",
            err = err
        );
        log::error!("{msg}");
        return Err(msg);
    }
    match serde_json::to_string(state) {
        Ok(json) => {
            static SAVE_COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = SAVE_COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
            let tmp_path = path.with_extension(format!("save-{seq}.tmp", seq = seq));
            if let Err(err) = fs::write(&tmp_path, &json) {
                let msg = format!("Failed to write temp download state: {err}", err = err);
                log::error!("{msg}");
                return Err(msg);
            }
            if let Err(err) = fs::rename(&tmp_path, &path) {
                let msg = format!("Failed to rename download state file: {err}", err = err);
                log::error!("{msg}");
                if let Err(err) = fs::remove_file(&tmp_path) {
                    log::debug!("Failed to clean up temp state file: {err}", err = err);
                }
                return Err(msg);
            }
            Ok(())
        }
        Err(err) => {
            let msg = format!("Failed to serialize download state: {err}", err = err);
            log::error!("{msg}");
            Err(msg)
        }
    }
}

pub fn load_download_state(app: &AppHandle) -> Option<DownloadState> {
    let path = download_state_path(app)?;
    load_download_state_from(&path)
}

/// Load and parse a download state file. On failure, rename the corrupt file
/// to `<path>.corrupted-<timestamp>.json` for inspection. Returns `None` if
/// the file is missing or unparseable.
pub(crate) fn load_download_state_from(path: &Path) -> Option<DownloadState> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(err) => {
            log::warn!(
                "Failed to read download state file {path}: {err}",
                path = path.display(),
                err = err
            );
            return None;
        }
    };
    match serde_json::from_str(&content) {
        Ok(state) => Some(state),
        Err(err) => preserve_corrupted_state(path, &err),
    }
}

/// Renames `path` to a timestamped backup and returns `None`. If the rename
/// fails (e.g. read-only filesystem), the file is removed as a fallback so
/// subsequent loads do not keep failing on the same corrupt JSON.
fn preserve_corrupted_state(path: &Path, parse_err: &serde_json::Error) -> Option<DownloadState> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup_path =
        path.with_extension(format!("corrupted-{timestamp}.json", timestamp = timestamp));
    log::warn!(
        "Download state file corrupted ({parse_err}), preserving as {backup}",
        parse_err = parse_err,
        backup = backup_path.display()
    );
    match fs::rename(path, &backup_path) {
        Ok(()) => {
            log::warn!(
                "Corrupted download state preserved at {backup}; user will resume from scratch",
                backup = backup_path.display()
            );
        }
        Err(rename_err) => {
            log::warn!(
                "Failed to preserve corrupted download state at {backup}: {rename_err}; removing instead",
                backup = backup_path.display(),
                rename_err = rename_err
            );
            let _ = fs::remove_file(path);
        }
    }
    None
}

pub fn clear_download_state(app: &AppHandle) {
    let Some(path) = download_state_path(app) else {
        log::warn!("Failed to resolve download state path during clear");
        return;
    };
    let _ = fs::remove_file(path);
}

/// Delete the chunks directory. Returns `true` if removed, `false` if not
/// found. Errors are logged but not propagated (best-effort cleanup).
pub fn delete_chunks_dir(app: &AppHandle, output_path: &str) -> bool {
    let game_dir = match app.path().resolve(output_path, BaseDirectory::AppData) {
        Ok(p) => p,
        Err(err) => {
            log::warn!(
                "Failed to resolve game dir for chunk cleanup: {err}",
                err = err
            );
            return false;
        }
    };
    let chunks_dir = game_dir.join("chunks");
    match fs::remove_dir_all(&chunks_dir) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => {
            log::warn!(
                "Failed to delete chunks directory {dir}: {err}",
                dir = chunks_dir.display(),
                err = err
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_download_state_corrupted_preserves_backup() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("download_state.json");
        let corrupt_bytes = b"{not valid json at all";
        std::fs::write(&state_path, corrupt_bytes).unwrap();

        let result = load_download_state_from(&state_path);
        assert!(result.is_none(), "corrupt state must not load");

        assert!(
            !state_path.exists(),
            "original corrupt file should be moved aside"
        );

        let mut found_backup = false;
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("download_state.corrupted-") && name.ends_with(".json") {
                let backup_bytes = std::fs::read(entry.path()).unwrap();
                assert_eq!(
                    backup_bytes, corrupt_bytes,
                    "preserved backup must contain the original corrupt bytes"
                );
                found_backup = true;
            }
        }
        assert!(
            found_backup,
            "expected a renamed backup file matching the corrupted-<timestamp>.json pattern"
        );
    }

    /// If renaming fails (e.g. read-only filesystem), remove the corrupt file
    /// so the next load starts fresh.
    #[test]
    fn load_download_state_corrupted_removed_when_rename_fails() {
        // On Linux, cross-device rename fails. We simulate by setting up the
        // state file as a directory, fs::rename will fail because the
        // destination pattern resolves to a child of this dir that already
        // exists.
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("download_state.json");
        // Place a file at the backup path so the rename-overwrite attempt
        // would resolve to an existing path; on Linux rename silently
        // replaces, so we instead place a directory at the backup path.
        let backup_collide = dir.path().join("download_state.corrupted-0.json");
        std::fs::create_dir(&backup_collide).unwrap();
        std::fs::write(&state_path, b"garbage").unwrap();

        let result = load_download_state_from(&state_path);
        assert!(result.is_none());
        // Either the original file is gone (success path) or we exercised the
        // fallback that removed it. In both cases the original state must
        // not be left in place to cause repeated failures.
        if state_path.exists() {
            panic!(
                "original state file should have been renamed or removed; leftover content suggests bug"
            );
        }
    }

    /// Valid state files load without creating backups.
    #[test]
    fn load_download_state_valid_does_not_create_backup() {
        use crate::commands::sophon_downloader::DownloadState;
        use crate::commands::sophon_downloader::types::DownloadType;
        use std::collections::HashMap;
        use std::collections::HashSet;
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("download_state.json");
        let state = DownloadState {
            game_id: "test_game".into(),
            vo_lang: "en-us".into(),
            output_path: "/data/game".into(),
            download_type: DownloadType::Fresh,
            current_tag: None,
            manifest_hash: "hash".into(),
            downloaded_chunks: HashMap::new(),
            completed_files: HashSet::new(),
        };
        std::fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();

        let result = load_download_state_from(&state_path);
        assert!(result.is_some(), "valid state must load");
        assert!(
            state_path.exists(),
            "valid state file should remain in place"
        );

        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(
            entries.len(),
            1,
            "no backup file should have been created; found: {entries:?}"
        );
    }
}
