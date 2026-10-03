use crate::config::AppConfig;
use axum::{http::StatusCode, Json};
use serde_json::Value;

pub async fn load_config() -> Json<AppConfig> {
    Json(AppConfig::load().masked())
}

pub async fn save_config(Json(body): Json<Value>) -> Result<StatusCode, StatusCode> {
    let mut updated: AppConfig =
        serde_json::from_value(body).map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    updated.restore_masked(AppConfig::load());
    updated.save().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}
