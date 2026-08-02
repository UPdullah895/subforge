use axum::http::StatusCode;
use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobControlRequest {
    pub job_id: String,
}

pub async fn cancel_job(
    State(state): State<AppState>,
    Json(payload): Json<JobControlRequest>,
) -> Result<StatusCode, StatusCode> {
    let registry = state.job_controls.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match registry.get(&payload.job_id) {
        Some(control) => {
            control.cancel();
            Ok(StatusCode::OK)
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn pause_job(
    State(state): State<AppState>,
    Json(payload): Json<JobControlRequest>,
) -> Result<StatusCode, StatusCode> {
    let registry = state.job_controls.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match registry.get(&payload.job_id) {
        Some(control) => {
            control.pause();
            Ok(StatusCode::OK)
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn resume_job(
    State(state): State<AppState>,
    Json(payload): Json<JobControlRequest>,
) -> Result<StatusCode, StatusCode> {
    let registry = state.job_controls.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match registry.get(&payload.job_id) {
        Some(control) => {
            control.resume();
            Ok(StatusCode::OK)
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}
