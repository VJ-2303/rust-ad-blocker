use std::sync::Arc;

use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;

use crate::metrics::Metrics;

#[derive(Serialize)]
pub struct StatsResponse {
    pub total_queries: u64,
    pub blocked_queries: u64,

    pub cache_hits: u64,
    pub cache_misses: u64,
    pub upstream_errors: u64,

    pub block_percentage: f64,
    pub cache_hit_percentage: f64,
    pub average_upstream_latency_ms: f64,
    pub uptime_seconds: u64,
}

async fn health_check() -> &'static str {
    "Amin API is running!"
}

async fn stats(State(metrics): State<Arc<Metrics>>) -> Json<StatsResponse> {
    // 1. Read all atomic values
    let total = metrics
        .total_queries
        .load(std::sync::atomic::Ordering::Relaxed);
    let blocked = metrics
        .blocked_queries
        .load(std::sync::atomic::Ordering::Relaxed);
    let hits = metrics
        .cache_hits
        .load(std::sync::atomic::Ordering::Relaxed);
    let misses = metrics
        .cache_misses
        .load(std::sync::atomic::Ordering::Relaxed);
    let errors = metrics
        .upstream_errors
        .load(std::sync::atomic::Ordering::Relaxed);
    let latency_total = metrics
        .upstream_latency_ms
        .load(std::sync::atomic::Ordering::Relaxed);
    let requests = metrics
        .upstream_requests
        .load(std::sync::atomic::Ordering::Relaxed);

    // 2. Calculate Block Percentage
    let block_percentage = if total > 0 {
        (blocked as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    // 3. Calculate Cache Hit Percentage
    let total_cache_ops = hits + misses;
    let cache_hit_percentage = if total_cache_ops > 0 {
        (hits as f64 / total_cache_ops as f64) * 100.0
    } else {
        0.0
    };

    // 4. Calculate Average Upstream Latency
    let avg_latency = if requests > 0 {
        latency_total as f64 / requests as f64
    } else {
        0.0
    };

    // 5. Calculate Uptime
    let uptime = metrics.start_time.elapsed().as_secs();

    let stats_response = StatsResponse {
        total_queries: total,
        blocked_queries: blocked,
        cache_hits: hits,
        cache_misses: misses,
        upstream_errors: errors,
        block_percentage: (block_percentage * 100.0).round() / 100.0, // Round to 2 decimals
        cache_hit_percentage: (cache_hit_percentage * 100.0).round() / 100.0,
        average_upstream_latency_ms: (avg_latency * 100.0).round() / 100.0,
        uptime_seconds: uptime,
    };

    Json(stats_response)
}

pub fn app(metrics: Arc<Metrics>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/stats", get(stats))
        .with_state(metrics)
}
