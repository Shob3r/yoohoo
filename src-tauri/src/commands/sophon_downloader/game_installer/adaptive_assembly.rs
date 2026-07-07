use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use super::*;

/// Maximum assembly concurrency is fixed at `ASSEMBLY_CONCURRENCY`.
/// We intentionally do NOT throttle based on available RAM —
/// the goal is 100% download bandwidth, not RAM savings.
pub struct AdaptiveAssembly {
    #[allow(dead_code)]
    target: AtomicUsize,
}

impl AdaptiveAssembly {
    pub fn new() -> Self {
        Self {
            target: AtomicUsize::new(ASSEMBLY_CONCURRENCY),
        }
    }

    pub fn current_target(&self) -> usize {
        ASSEMBLY_CONCURRENCY
    }

    #[allow(dead_code)]
    pub fn adjust(&self) -> usize {
        ASSEMBLY_CONCURRENCY
    }

    pub fn spawn_adjuster(self: &Arc<Self>, _cancel_token: tokio_util::sync::CancellationToken) {
        // No-op: throttling removed. Performance > RAM savings.
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
    fn target_always_max() {
        let aa = AdaptiveAssembly::new();
        assert_eq!(aa.current_target(), ASSEMBLY_CONCURRENCY);
        let _ = aa.adjust();
        assert_eq!(aa.current_target(), ASSEMBLY_CONCURRENCY);
    }
}
