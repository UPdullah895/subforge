use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use subforge_core::tools::ToolError;

/// A JSON error reply: `{"error": "...", "details": "..."}`. `details` holds raw
/// tool output and is omitted when there is none.
pub struct AppError {
    status: StatusCode,
    message: String,
    details: Option<String>,
}

impl AppError {
    pub fn new(status: StatusCode, message: impl ToString) -> Self {
        Self {
            status,
            message: message.to_string(),
            details: None,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = match self.details {
            Some(d) => json!({ "error": self.message, "details": d }),
            None => json!({ "error": self.message }),
        };
        (self.status, Json(body)).into_response()
    }
}

impl From<ToolError> for AppError {
    fn from(e: ToolError) -> Self {
        let status = match &e {
            ToolError::CustomPathInvalid { .. } | ToolError::NotFound(_) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            ToolError::Failed { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            ToolError::Cancelled => StatusCode::CONFLICT,
            ToolError::Io(_) | ToolError::Json(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self {
            status,
            details: e.details().map(str::to_string),
            message: e.to_string(),
        }
    }
}

/// Bare status codes (e.g. from [`blocking`]) become a generic sentence.
impl From<StatusCode> for AppError {
    fn from(status: StatusCode) -> Self {
        let message = status
            .canonical_reason()
            .unwrap_or("Request failed")
            .to_string();
        Self::new(status, message)
    }
}

pub fn err500(msg: impl ToString) -> AppError {
    AppError::new(StatusCode::INTERNAL_SERVER_ERROR, msg)
}

pub fn err400(msg: impl ToString) -> AppError {
    AppError::new(StatusCode::BAD_REQUEST, msg)
}

pub fn err403(msg: impl ToString) -> AppError {
    AppError::new(StatusCode::FORBIDDEN, msg)
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
