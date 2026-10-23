//! whisper.cpp model catalog, download and transcription.

pub mod device;
pub mod models;
pub mod transcribe;

pub use device::{engine_info, Device, EngineInfo};
pub use models::{
    default_cache_dir, download, download_with, legacy_cache_dir, list_downloaded,
    list_downloaded_specs, model_path, spec_for, ModelSpec, WhisperError, DEFAULT_BASE_URL, MODELS,
    VAD_MODEL, VAD_URL,
};
pub use transcribe::{transcribe, OutFormat, TranscribeOptions, TranscribeOutcome};
