//! whisper.cpp model catalog, download, transcription and the background jobs around them.

pub mod device;
pub mod jobs;
pub mod models;
pub mod transcribe;

pub use device::{detect_gpu, engine_info, select_device, Device, EngineInfo};
pub use jobs::{
    catalog, delete_legacy_cache, legacy_cache, start_download_with, start_model_download,
    start_transcription, start_transcription_with, ActiveDownloads, CatalogEntry, JobKind,
    JobOutcome, JobSink,
};
pub use models::{
    default_cache_dir, download, download_with, legacy_cache_dir, list_downloaded,
    list_downloaded_specs, model_path, resolve_cache_dir, spec_for, ModelSpec, WhisperError,
    DEFAULT_BASE_URL, MODELS, VAD_MODEL, VAD_URL,
};
pub use transcribe::{transcribe, OutFormat, TranscribeOptions, TranscribeOutcome};
