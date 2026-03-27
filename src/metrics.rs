use std::sync::atomic::AtomicU64;

#[derive(Debug, Default)]
pub struct Metrics {
    pub total_queries: AtomicU64,
    pub blocked_queries: AtomicU64,
}
