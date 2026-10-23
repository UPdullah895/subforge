//! whisper.cpp model catalog and download.

pub mod models;

pub use models::{
    default_cache_dir, download, download_with, legacy_cache_dir, list_downloaded,
    list_downloaded_specs, model_path, spec_for, ModelSpec, WhisperError, DEFAULT_BASE_URL, MODELS,
    VAD_MODEL, VAD_URL,
};
