use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{delete, get},
};
use include_dir::{Dir, include_dir};
use serde::Serialize;

use crate::admin::domains::{add_custom_domain, list_custom_domains, remove_custom_domain};
use crate::admin::state::AppState;

static ADMIN_ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/static/admin");

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
    pub blocked_domains_count: u64,
}

async fn health_check() -> &'static str {
    "Admin API is running!"
}

async fn admin_index() -> impl IntoResponse {
    if let Some(file) = ADMIN_ASSETS.get_file("index.html") {
        let html = String::from_utf8_lossy(file.contents()).to_string();
        return Html(html).into_response();
    }

    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Embedded index.html not found",
    )
        .into_response()
}

async fn admin_asset(Path(path): Path<String>) -> Response {
    let clean_path = path.trim_start_matches('/');

    let requested = if clean_path.is_empty() {
        "index.html"
    } else {
        clean_path
    };

    if let Some(file) = ADMIN_ASSETS.get_file(requested) {
        let mime = mime_guess::from_path(requested).first_or_octet_stream();

        let mut headers = HeaderMap::new();
        if let Ok(value) = HeaderValue::from_str(mime.as_ref()) {
            headers.insert(header::CONTENT_TYPE, value);
        }

        return (StatusCode::OK, headers, file.contents().to_vec()).into_response();
    }

    (StatusCode::NOT_FOUND, "Asset not found").into_response()
}

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

    let blocked_domains_count = state.blocklist.len() as u64;

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
        blocked_domains_count,
    })
}

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/admin", get(admin_index))
        .route("/admin/", get(admin_index))
        .route("/admin/*path", get(admin_asset))
        .route("/api/v1/stats", get(stats))
        .route(
            "/api/v1/domains/custom",
            get(list_custom_domains).post(add_custom_domain),
        )
        .route(
            "/api/v1/domains/custom/:domain",
            delete(remove_custom_domain),
        )
        .with_state(state)
}
