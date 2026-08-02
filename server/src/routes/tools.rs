use crate::tools::ToolRegistry;
use axum::Json;

pub async fn get_registry() -> Json<ToolRegistry> {
    Json(ToolRegistry::discover())
}
