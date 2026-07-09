use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::*;

/// Interval between adaptive scaling checks (milliseconds).
const ADJUST_INTERVAL_MS: u64 = 500;
/// Minimum interval between actual adjustments to let throughput stabilize.
const MIN_ADJUST_INTERVAL_MS: u64 = 400;
/// Throughput drop threshold: if current throughput falls below this
/// fraction of the previous measurement, a bottleneck is assumed.
const BOTTLENECK_THRESHOLD: f64 = 0.80;
/// Scale-up multiplier when throughput is healthy.
const SCALE_UP_FACTOR: f64 = 1.5;
/// Scale-down multiplier when a bottleneck is detected.
const SCALE_DOWN_FACTOR: f64 = 0.5;
/// Minimum concurrency during adaptive phase (same as during download).
const MIN_ADAPTIVE_TARGET: usize = ASSEMBLY_CONCURRENCY;

/// Assembly concurrency controller.
///
/// While downloads are active concurrency is capped at `ASSEMBLY_CONCURRENCY`
/// so assembly does not starve download workers. Once downloads finish the
/// controller switches to adaptive scaling: it measures how many files are
/// assembled per time window and slowly raises concurrency. If throughput
/// drops (bottleneck detected) it scales back down.
pub struct AdaptiveAssembly {
    target: AtomicUsize,
    download_active: AtomicBool,
    assembled_files: Option<Arc<AtomicU64>>,
    last_count: AtomicUsize,
    last_throughput: AtomicU64, // files per second, scaled by 1000
    last_adjust_time: std::sync::Mutex<Instant>,
}

impl AdaptiveAssembly {
    pub fn new() -> Self {
        Self {
            target: AtomicUsize::new(ASSEMBLY_CONCURRENCY),
            download_active: AtomicBool::new(true),
            assembled_files: None,
            last_count: AtomicUsize::new(0),
            last_throughput: AtomicU64::new(0),
            last_adjust_time: std::sync::Mutex::new(Instant::now()),
        }
    }

    pub fn with_tracker(assembled_files: Arc<AtomicU64>) -> Self {
        Self {
            target: AtomicUsize::new(ASSEMBLY_CONCURRENCY),
            download_active: AtomicBool::new(true),
            assembled_files: Some(assembled_files),
            last_count: AtomicUsize::new(0),
            last_throughput: AtomicU64::new(0),
            last_adjust_time: std::sync::Mutex::new(Instant::now()),
        }
    }

    pub fn current_target(&self) -> usize {
        if self.download_active.load(Ordering::Relaxed) {
            ASSEMBLY_CONCURRENCY
        } else {
            self.target.load(Ordering::Relaxed).max(MIN_ADAPTIVE_TARGET)
        }
    }

    #[allow(dead_code)]
    pub fn adjust(&self) -> usize {
        self.current_target()
    }

    pub fn set_download_active(&self, active: bool) {
        self.download_active.store(active, Ordering::Relaxed);
    }

    /// Spawn a background task that periodically measures assembly throughput
    /// and adjusts the concurrency target.
    pub fn spawn_adjuster(self: &Arc<Self>, cancel_token: tokio_util::sync::CancellationToken) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(ADJUST_INTERVAL_MS));
            loop {
                tokio::select! {
                    biased;
                    _ = cancel_token.cancelled() => break,
                    _ = interval.tick() => {}
                }

                // Only adapt after downloads have finished.
                if this.download_active.load(Ordering::Relaxed) {
                    continue;
                }

                // Need a tracker to measure throughput.
                let assembled_files = match &this.assembled_files {
                    Some(tracker) => tracker,
                    None => continue,
                };

                let now = Instant::now();
                let mut last_time = match this.last_adjust_time.try_lock() {
                    Ok(g) => g,
                    Err(_) => continue,
                };

                let elapsed = now.duration_since(*last_time);
                if elapsed.as_millis() < MIN_ADJUST_INTERVAL_MS as u128 {
                    continue;
                }

                let current_count = assembled_files.load(Ordering::Relaxed) as usize;
                let prev_count = this.last_count.load(Ordering::Relaxed);
                let delta = current_count.saturating_sub(prev_count);
                this.last_count.store(current_count, Ordering::Relaxed);

                let elapsed_secs = elapsed.as_secs_f64().max(0.001);
                let throughput = (delta as f64) / elapsed_secs;
                let prev_throughput = this.last_throughput.load(Ordering::Relaxed) as f64 / 1000.0;
                this.last_throughput
                    .store((throughput * 1000.0) as u64, Ordering::Relaxed);
                *last_time = now;
                drop(last_time);

                // On the first measurement after downloads finish just record
                // the baseline and do not adjust yet.
                if prev_throughput == 0.0 {
                    continue;
                }

                let current_target = this.target.load(Ordering::Relaxed);
                let new_target = if throughput >= prev_throughput * BOTTLENECK_THRESHOLD {
                    // Throughput is healthy — scale up.
                    let scaled = (current_target as f64 * SCALE_UP_FACTOR) as usize;
                    let incremented = current_target.saturating_add(1);
                    scaled.max(incremented)
                } else {
                    // Bottleneck detected — scale down.
                    let scaled = (current_target as f64 * SCALE_DOWN_FACTOR) as usize;
                    scaled.max(MIN_ADAPTIVE_TARGET)
                };

                this.target.store(new_target, Ordering::Relaxed);
            }
        });
    }
}

impl Default for AdaptiveAssembly {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_has_initial_target() {
        let aa = AdaptiveAssembly::new();
        assert_eq!(aa.current_target(), ASSEMBLY_CONCURRENCY);
    }

    #[test]
    fn target_limited_during_download() {
        let aa = AdaptiveAssembly::new();
        assert_eq!(aa.current_target(), ASSEMBLY_CONCURRENCY);
    }

    #[test]
    fn target_adaptive_after_download_finishes() {
        let aa = AdaptiveAssembly::new();
        aa.set_download_active(false);
        // Before any adjustment the target is still the initial value.
        assert_eq!(aa.current_target(), ASSEMBLY_CONCURRENCY);
    }

    #[test]
    fn target_returns_to_limited_when_download_restarts() {
        let aa = AdaptiveAssembly::new();
        aa.set_download_active(false);
        aa.target.store(128, Ordering::Relaxed);
        aa.set_download_active(true);
        assert_eq!(aa.current_target(), ASSEMBLY_CONCURRENCY);
    }

    #[test]
    fn with_tracker_starts_at_concurrency() {
        let tracker = Arc::new(AtomicU64::new(0));
        let aa = AdaptiveAssembly::with_tracker(tracker);
        assert_eq!(aa.current_target(), ASSEMBLY_CONCURRENCY);
    }
}
