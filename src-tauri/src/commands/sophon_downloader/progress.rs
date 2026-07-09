//! Progress events and structured errors for the Sophon downloader IPC
//! boundary.

use serde::{Deserialize, Serialize};

use crate::commands::sophon_downloader::game_installer::SophonError;

/// Progress events emitted during download operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SophonProgress {
    /// Manifest is being fetched from the API.
    FetchingManifest,
    /// Existing files are being checked to determine what needs downloading.
    #[serde(rename_all = "camelCase")]
    CalculatingDownloads {
        checked_files: u64,
        total_files: u64,
    },
    /// Chunks are being downloaded.
    #[serde(rename_all = "camelCase")]
    Downloading {
        downloaded_bytes: u64,
        total_bytes: u64,
        speed_bps: f64,
        eta_seconds: f64,
    },
    /// Download is paused.
    #[serde(rename_all = "camelCase")]
    Paused {
        downloaded_bytes: u64,
        total_bytes: u64,
    },
    /// Files are being assembled from downloaded chunks.
    #[serde(rename_all = "camelCase")]
    Assembling {
        assembled_files: u64,
        total_files: u64,
    },
    /// Files are being verified before assembly.
    #[serde(rename_all = "camelCase")]
    CheckingFiles {
        checked_files: u64,
        total_files: u64,
    },
    /// Files are being verified for integrity.
    #[serde(rename_all = "camelCase")]
    Verifying {
        scanned_files: u64,
        total_files: u64,
        error_count: u64,
    },
    /// Non-fatal warning occurred.
    Warning { message: String },
    /// Fatal error occurred.
    Error { message: String },
    /// Installing plugins into the game directory.
    #[serde(rename_all = "camelCase")]
    InstallingPlugins {
        current_plugin: String,
        total_plugins: usize,
    },
    /// Installing channel SDKs into the game directory.
    #[serde(rename_all = "camelCase")]
    InstallingSdks {
        current_sdk: String,
        total_sdks: usize,
    },
    /// Downloading a plugin/SDK ZIP package.
    #[serde(rename_all = "camelCase")]
    DownloadingPlugin {
        name: String,
        downloaded_bytes: u64,
        total_bytes: u64,
    },
    /// Applying preinstall patches to game files.
    #[serde(rename_all = "camelCase")]
    ApplyingPreinstall {
        applied_files: u64,
        total_files: u64,
    },
    /// Download completed successfully.
    Finished,
}

/// Structured error payload for the Tauri IPC boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CommandError {
    Cancelled,
    NoSpaceAvailable {
        path: String,
        needed: u64,
        available: u64,
    },
    Md5Mismatch {
        item: String,
    },
    SizeMismatch {
        item: String,
        expected: u64,
        actual: u64,
    },
    OriginalFileMissing {
        path: String,
    },
    DownloadFailed {
        chunk: String,
        attempts: u32,
    },
    HdiffPatchFailed {
        file: String,
    },
    AssemblyFailed {
        file: String,
    },
    NoGameManifest,
    NoVoiceManifest {
        locale: String,
    },
    InvalidAssetName {
        name: String,
    },
    PathTraversal {
        path: String,
    },
    ApiError {
        retcode: i32,
        message: String,
    },
    PluginValidationFailed {
        name: String,
    },
    Generic {
        message: String,
    },
}

impl From<SophonError> for CommandError {
    fn from(e: SophonError) -> Self {
        match e {
            SophonError::Cancelled => CommandError::Cancelled,
            SophonError::NoSpaceAvailable {
                path,
                needed,
                available,
            } => CommandError::NoSpaceAvailable {
                path,
                needed,
                available,
            },
            SophonError::Md5Mismatch { item, .. } => CommandError::Md5Mismatch { item },
            SophonError::SizeMismatch {
                item,
                expected,
                actual,
            } => CommandError::SizeMismatch {
                item,
                expected,
                actual,
            },
            SophonError::OriginalFileMissing(path) => CommandError::OriginalFileMissing { path },
            SophonError::DownloadFailed {
                chunk, attempts, ..
            } => CommandError::DownloadFailed { chunk, attempts },
            SophonError::HDiffPatchFailed { file, .. } => CommandError::HdiffPatchFailed { file },
            SophonError::AssemblyFailed { file, .. } => CommandError::AssemblyFailed { file },
            SophonError::NoGameManifest => CommandError::NoGameManifest,
            SophonError::NoVoiceManifest(locale) => CommandError::NoVoiceManifest { locale },
            SophonError::InvalidAssetName(name) => CommandError::InvalidAssetName { name },
            SophonError::PathTraversal(path) => CommandError::PathTraversal {
                path: path.to_string_lossy().to_string(),
            },
            SophonError::ApiError(retcode, message) => CommandError::ApiError { retcode, message },
            SophonError::PluginValidationFailed(name) => {
                CommandError::PluginValidationFailed { name }
            }
            _ => CommandError::Generic {
                message: e.to_string(),
            },
        }
    }
}

impl From<&SophonError> for CommandError {
    fn from(e: &SophonError) -> Self {
        match e {
            SophonError::Cancelled => CommandError::Cancelled,
            SophonError::NoSpaceAvailable {
                path,
                needed,
                available,
            } => CommandError::NoSpaceAvailable {
                path: path.clone(),
                needed: *needed,
                available: *available,
            },
            SophonError::Md5Mismatch { item, .. } => {
                CommandError::Md5Mismatch { item: item.clone() }
            }
            SophonError::SizeMismatch {
                item,
                expected,
                actual,
            } => CommandError::SizeMismatch {
                item: item.clone(),
                expected: *expected,
                actual: *actual,
            },
            SophonError::OriginalFileMissing(path) => {
                CommandError::OriginalFileMissing { path: path.clone() }
            }
            SophonError::DownloadFailed {
                chunk, attempts, ..
            } => CommandError::DownloadFailed {
                chunk: chunk.clone(),
                attempts: *attempts,
            },
            SophonError::HDiffPatchFailed { file, .. } => {
                CommandError::HdiffPatchFailed { file: file.clone() }
            }
            SophonError::AssemblyFailed { file, .. } => {
                CommandError::AssemblyFailed { file: file.clone() }
            }
            SophonError::NoGameManifest => CommandError::NoGameManifest,
            SophonError::NoVoiceManifest(locale) => CommandError::NoVoiceManifest {
                locale: locale.clone(),
            },
            SophonError::InvalidAssetName(name) => {
                CommandError::InvalidAssetName { name: name.clone() }
            }
            SophonError::PathTraversal(path) => CommandError::PathTraversal {
                path: path.to_string_lossy().to_string(),
            },
            SophonError::ApiError(retcode, message) => CommandError::ApiError {
                retcode: *retcode,
                message: message.clone(),
            },
            SophonError::PluginValidationFailed(name) => {
                CommandError::PluginValidationFailed { name: name.clone() }
            }
            other => CommandError::Generic {
                message: other.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sophon_progress_downloading_serializes_total_bytes() {
        let p = SophonProgress::Downloading {
            downloaded_bytes: 1024,
            total_bytes: 30_000_000_000,
            speed_bps: 45_000_000.0,
            eta_seconds: 600.0,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(
            json,
            r#"{"type":"downloading","downloadedBytes":1024,"totalBytes":30000000000,"speedBps":45000000.0,"etaSeconds":600.0}"#
        );
    }
}
