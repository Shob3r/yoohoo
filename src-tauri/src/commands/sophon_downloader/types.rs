//! Core data types for the Sophon downloader.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Type of download operation, persisted for correct resumption dispatch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum DownloadType {
    Fresh,
    Update,
    Preinstall,
}

/// Persisted state for download resumption after app restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadState {
    pub game_id: String,
    pub vo_lang: String,
    pub output_path: String,
    pub download_type: DownloadType,
    pub current_tag: Option<String>,
    pub manifest_hash: String,
    pub downloaded_chunks: HashMap<String, u64>,
    #[serde(default)]
    pub completed_files: HashSet<String>,
}

/// Summary of persisted download state returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeInfo {
    pub game_id: String,
    pub download_type: DownloadType,
}

/// Save download state after every N completed chunks.
pub const CHUNK_STATE_SAVE_INTERVAL: u64 = 500;
