pub mod audio;
pub mod batch;
pub mod deps;
pub mod jobs;
pub mod providers;
pub mod rename;
pub mod subtitle;
pub mod subtitle_sources;
pub mod transcribe;
pub mod translate;

/// Runs blocking work (process spawns, file I/O on user media) off the async
/// runtime and flattens the join error into the commands' `String` error shape.
pub(crate) async fn blocking<T, F>(f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}
