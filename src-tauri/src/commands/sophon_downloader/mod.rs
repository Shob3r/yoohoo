//! Sophon game downloader. Manifest-based chunk downloads with zstd
//! compression.

pub mod api_scrape;
pub mod game_installer;
pub mod proto_parse;

mod client;
pub mod commands;
mod manifest;
mod progress;
mod state;
mod types;

pub use client::{ActiveDownload, DownloadClient, HttpClient};
pub use manifest::compute_content_manifest_hash;
pub use progress::{CommandError, SophonProgress};
pub use state::{clear_download_state, load_download_state, save_download_state};
pub use types::{CHUNK_STATE_SAVE_INTERVAL, DownloadState, DownloadType, ResumeInfo};
