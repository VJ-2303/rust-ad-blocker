use axum::{Json, extract::{Path, State}};
use serde::{Deserialize, Serialize};

use crate::admin::state::AppState;

#[derive(Serialize)]
pub struct DomainListResponse {
    pub domains: Vec<String>,
}

#[derive(Deserialize)]
pub struct AddDomainRequest {
    pub domain: String,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub status: String,
    pub message: String,
}

pub async fn list_custom_domains(State(state): State<AppState>) -> Json<DomainListResponse> {
    let mut domains = state.blocklist.get_custom_domains();
    domains.sort();
    Json(DomainListResponse { domains })
}

pub async fn add_custom_domain(
    State(state): State<AppState>,
    Json(payload): Json<AddDomainRequest>,
) -> Json<StatusResponse> {
    if payload.domain.is_empty() {
        return Json(StatusResponse {
            status: "error".to_string(),
            message: "Domain cannot be empty".to_string(),
        });
    }

    match state.blocklist.add_custom_domain(&payload.domain).await {
        Ok(_) => Json(StatusResponse {
            status: "success".to_string(),
            message: format!("Successfully blocked {}", payload.domain),
        }),
        Err(e) => Json(StatusResponse {
            status: "error".to_string(),
            message: format!("Failed to save domain: {}", e),
        }),
    }
}

pub async fn remove_custom_domain(
    Path(domain): Path<String>,
    State(state): State<AppState>,
) -> Json<StatusResponse> {
    if domain.is_empty() {
        return Json(StatusResponse {
            status: "error".to_string(),
            message: "Domain cannot be empty".to_string(),
        });
    }

    match state.blocklist.remove_custom_domain(&domain).await {
        Ok(true) => Json(StatusResponse {
            status: "success".to_string(),
            message: format!("Successfully unblocked {}", domain),
        }),
        Ok(false) => Json(StatusResponse {
            status: "not_found".to_string(),
            message: format!("'{}' was not in the custom blocklist", domain),
        }),
        Err(e) => Json(StatusResponse {
            status: "error".to_string(),
            message: format!("Failed to remove domain: {}", e),
        }),
    }
}
