//! Progress reporting and cancellation shared by long-running operations.

pub use tokio_util::sync::CancellationToken;

/// Progress callback. The first argument is a percentage in `0.0..=100.0`, the second an
/// optional human-readable status message.
pub type ProgressFn<'a> = &'a mut (dyn FnMut(f64, Option<&str>) + Send);
