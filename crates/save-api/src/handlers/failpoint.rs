use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ConfigureFailpointRequest {
    pub name: String,
    pub action: String,
}

#[derive(Debug, Serialize)]
pub struct ConfigureFailpointResponse {
    pub success: bool,
    pub message: String,
}

pub async fn configure_failpoint(
    Json(req): Json<ConfigureFailpointRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    fail::cfg(&req.name, &req.action).map_err(|_| StatusCode::BAD_REQUEST)?;

    Ok(Json(ConfigureFailpointResponse {
        success: true,
        message: format!(
            "Configured failpoint '{}' with action '{}'",
            req.name, req.action
        ),
    }))
}

pub async fn remove_failpoint(
    Json(req): Json<ConfigureFailpointRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    fail::remove(&req.name);

    Ok(Json(ConfigureFailpointResponse {
        success: true,
        message: format!("Removed failpoint '{}'", req.name),
    }))
}
