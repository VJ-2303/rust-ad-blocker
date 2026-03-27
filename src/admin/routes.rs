use std::sync::Arc;

use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;

use crate::metrics::Metrics;

#[derive(Serialize)]
pub struct StatsResponse {
    pub total_queries: u64,
    pub blocked_queries: u64,
}

async fn health_check() -> &'static str {
    "Amin API is running!"
}

async fn stats(State(metrics): State<Arc<Metrics>>) -> Json<StatsResponse> {
    let stats_response = StatsResponse {
        total_queries: metrics
            .total_queries
            .load(std::sync::atomic::Ordering::Relaxed),
        blocked_queries: metrics
            .blocked_queries
            .load(std::sync::atomic::Ordering::Relaxed),
    };
    Json(stats_response)
}

pub fn app(metrics: Arc<Metrics>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/stats", get(stats))
        .with_state(metrics)
}
