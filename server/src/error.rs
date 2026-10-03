use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub struct AppError(StatusCode, String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

pub fn err500(msg: impl ToString) -> AppError {
    AppError(StatusCode::INTERNAL_SERVER_ERROR, msg.to_string())
}

pub fn err400(msg: impl ToString) -> AppError {
    AppError(StatusCode::BAD_REQUEST, msg.to_string())
}

pub fn err503(msg: impl ToString) -> AppError {
    AppError(StatusCode::SERVICE_UNAVAILABLE, msg.to_string())
}

/// Runs a blocking closure (std `Command`, sync file IO) on tokio's blocking
/// pool so it can't stall the async worker threads.
pub async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, StatusCode> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
