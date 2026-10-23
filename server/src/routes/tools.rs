use crate::error::{blocking, err500, AppError};
use crate::tools::{self, ToolRegistry};
use axum::Json;
use subforge_core::tools::ToolStatus;

pub async fn get_registry() -> Result<Json<ToolRegistry>, AppError> {
    let registry = blocking(ToolRegistry::discover)
        .await
        .map_err(|_| err500("tool discovery failed"))?;
    Ok(Json(registry))
}

pub async fn get_status() -> Result<Json<Vec<ToolStatus>>, AppError> {
    let status = blocking(tools::status)
        .await
        .map_err(|_| err500("tool discovery failed"))?;
    Ok(Json(status))
}
