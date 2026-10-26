pub mod audio;
pub mod batch;
pub mod deps;
pub mod jobs;
pub mod paths;
pub mod providers;
pub mod rename;
pub mod subtitle;
pub mod subtitle_sources;
pub mod transcribe;
pub mod translate;

use serde::Serialize;
use subforge_core::tools::ToolError;

/// Error returned by commands that run external tools. Serialises as
/// `{ "message": "...", "details": "..." }`; the frontend's `apiInvoke` turns it
/// into an `ApiError` whose `details` (raw tool output) is shown in an expander.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl From<ToolError> for CommandError {
    fn from(e: ToolError) -> Self {
        Self {
            details: e.details().map(str::to_string),
            message: e.to_string(),
        }
    }
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self {
            message,
            details: None,
        }
    }
}

/// Runs blocking work (process spawns, file I/O on user media) off the async
/// runtime and flattens the join error into the command's error type.
pub(crate) async fn blocking<T, E, F>(f: F) -> Result<T, E>
where
    F: FnOnce() -> Result<T, E> + Send + 'static,
    T: Send + 'static,
    E: From<String> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| E::from(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use subforge_core::tools::ToolId;

    #[test]
    fn command_error_serialises_message_and_tool_details() {
        let e = CommandError::from(ToolError::Failed {
            tool: ToolId::Mkvmerge,
            summary: "The type of file 'x' could not be recognized.".into(),
            details: "mkvmerge v102.0\nError: The type of file 'x' could not be recognized.".into(),
        });
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(
            json["message"],
            "mkvmerge failed: The type of file 'x' could not be recognized."
        );
        assert!(json["details"].as_str().unwrap().contains("v102.0"));
    }

    #[test]
    fn command_error_without_details_omits_the_field() {
        let json = serde_json::to_value(CommandError::from("boom".to_string())).unwrap();
        assert_eq!(json, serde_json::json!({ "message": "boom" }));
    }

    #[tokio::test]
    async fn blocking_keeps_tool_details() {
        let r: Result<(), CommandError> = blocking(|| {
            Err(ToolError::Failed {
                tool: ToolId::Ffmpeg,
                summary: "s".into(),
                details: "raw".into(),
            }
            .into())
        })
        .await;
        assert_eq!(r.unwrap_err().details.as_deref(), Some("raw"));
    }
}
