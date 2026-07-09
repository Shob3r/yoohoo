//! HTTP client types for the Sophon downloader.

use std::time::Duration;

/// HTTP client wrapper for dependency injection.
pub struct HttpClient(pub reqwest::Client);

/// HTTP/1.1-only client for chunk downloads. Each connection gets an
/// independent TCP congestion window, avoiding HTTP/2's shared-window
/// bottleneck when multiplexing many streams over one connection.
pub struct DownloadClient(pub reqwest::Client);

impl Default for DownloadClient {
    fn default() -> Self {
        Self::new()
    }
}

impl DownloadClient {
    pub fn new() -> Self {
        Self(
            reqwest::Client::builder()
                .pool_max_idle_per_host(24)
                .pool_idle_timeout(Duration::from_secs(90))
                .tcp_nodelay(true)
                .http1_only()
                .tcp_keepalive(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(10))
                .read_timeout(Duration::from_secs(300))
                .build()
                .unwrap(),
        )
    }
}

/// Thread-safe container for the active download handle.
pub struct ActiveDownload(
    pub  tokio::sync::Mutex<
        Option<crate::commands::sophon_downloader::game_installer::DownloadHandle>,
    >,
);
