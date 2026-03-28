use std::{sync::atomic::AtomicU64, time::Instant};

#[derive(Debug)]
pub struct Metrics {
    pub total_queries: AtomicU64,
    pub blocked_queries: AtomicU64,

    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,

    pub upstream_errors: AtomicU64,
    pub upstream_latency_ms: AtomicU64,
    pub upstream_requests: AtomicU64,

    pub start_time: Instant,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            total_queries: AtomicU64::new(0),
            blocked_queries: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            upstream_errors: AtomicU64::new(0),
            upstream_latency_ms: AtomicU64::new(0),
            upstream_requests: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }
}
