//! Pipeline endpoints: what the policy says, and what it would do to one file.
//!
//! The plan endpoint is the dry run a library owner reviews before turning automation on —
//! it reads the file's tracks and returns the ordered steps without changing anything.

use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::pipeline::PipelineConfig;

/// `GET /api/pipeline/config`
pub async fn get_config() -> Json<Value> {
    let config = PipelineConfig::load();
    Json(json!({
        "enabled": config.enabled(),
        "watch_dirs": config.watch_dirs,
        "recursive": config.recursive,
        "policy": config.policy,
        "config_path": PipelineConfig::path().map(|path| path.to_string_lossy().to_string()),
    }))
}

#[derive(Deserialize)]
pub struct PlanRequest {
    pub file: String,
}

/// `POST /api/pipeline/plan` — the plan for one file under the configured policy.
pub async fn post_plan(Json(req): Json<PlanRequest>) -> Result<Json<Value>, AppError> {
    let policy = PipelineConfig::load().policy;
    let file = req.file;
    let plan = tokio::task::spawn_blocking(move || {
        let paths = crate::tools::tool_paths();
        subforge_pipeline::plan_file(&paths, &file, &policy)
    })
    .await
    .map_err(|error| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, error))?
    .map_err(|error| AppError::new(StatusCode::UNPROCESSABLE_ENTITY, error))?;

    let plan = serde_json::to_value(plan)
        .map_err(|error| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(Json(json!({ "plan": plan })))
}
