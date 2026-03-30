use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;

use crate::admin::domains::{add_custom_domain, list_custom_domains, remove_custom_domain};
use crate::admin::state::AppState;

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
    "Admin API is running!"
}

// FIX: Updated to extract AppState instead of Arc<Metrics>
async fn stats(State(state): State<AppState>) -> Json<StatsResponse> {
    let metrics = &state.metrics;

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

    let block_percentage = if total > 0 {
        (blocked as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    let total_cache_ops = hits + misses;
    let cache_hit_percentage = if total_cache_ops > 0 {
        (hits as f64 / total_cache_ops as f64) * 100.0
    } else {
        0.0
    };

    let avg_latency = if requests > 0 {
        latency_total as f64 / requests as f64
    } else {
        0.0
    };

    let uptime = metrics.start_time.elapsed().as_secs();

    Json(StatsResponse {
        total_queries: total,
        blocked_queries: blocked,
        cache_hits: hits,
        cache_misses: misses,
        upstream_errors: errors,
        block_percentage: (block_percentage * 100.0).round() / 100.0,
        cache_hit_percentage: (cache_hit_percentage * 100.0).round() / 100.0,
        average_upstream_latency_ms: (avg_latency * 100.0).round() / 100.0,
        uptime_seconds: uptime,
    })
}

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/stats", get(stats))
        // FIX: Chained get and post on the same exact path
        .route(
            "/api/v1/domains/custom",
            get(list_custom_domains)
                .post(add_custom_domain)
                .delete(remove_custom_domain),
        )
        .with_state(state)
}
