use crate::config::AppConfig;
use axum::{http::StatusCode, Json};
use serde_json::Value;

pub async fn load_config() -> Json<AppConfig> {
    Json(AppConfig::load().masked())
}

/// Merges an incoming config with the stored one: masked secrets keep their
/// stored values, and `config_version` never goes backwards. The web client
/// doesn't send `config_version`, so without this every save would reset it to
/// 0 and re-arm the desktop app's one-time path migration.
fn merge_for_save(mut incoming: AppConfig, stored: AppConfig) -> AppConfig {
    incoming.config_version = incoming.config_version.max(stored.config_version);
    incoming.restore_masked(stored);
    incoming
}

pub async fn save_config(Json(body): Json<Value>) -> Result<StatusCode, StatusCode> {
    let incoming: AppConfig =
        serde_json::from_value(body).map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    let updated = merge_for_save(incoming, AppConfig::load());
    updated
        .save()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}
