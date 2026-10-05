use subforge_deps::HideWindow;
use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WhisperError {
    #[error("Python not found in PATH")]
    PythonNotFound,
    #[error("Transcription script failed: {0}")]
    ScriptFailed(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parse error: {0}")]
    JsonError(String),
}

fn get_python() -> Result<PathBuf, WhisperError> {
    super::find_python().ok_or(WhisperError::PythonNotFound)
}

fn default_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .map(|d| d.join("subforge").join("whisper-models"))
        .unwrap_or_else(|| PathBuf::from(".cache/subforge/whisper-models"))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WhisperInfo {
    pub faster_whisper_version: String,
    pub cuda_available: bool,
    pub devices: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelList {
    pub models: Vec<String>,
}

pub fn get_whisper_info() -> Result<WhisperInfo, WhisperError> {
    let python = get_python()?;
    let script = super::script_path("transcribe.py");

    let output = Command::new(&python).hide_window()
        .args([&script, "info"])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout)
        .map_err(|e| WhisperError::JsonError(e.to_string()))
}

pub fn list_models(cache_dir: Option<String>) -> Result<Vec<String>, WhisperError> {
    let python = get_python()?;
    let script = super::script_path("transcribe.py");
    let cache = cache_dir.map(PathBuf::from).unwrap_or_else(default_cache_dir);

    let output = Command::new(&python).hide_window()
        .args([
            &script,
            "list-models",
            "--cache-dir",
            &cache.to_string_lossy(),
        ])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let list: ModelList = serde_json::from_str(&stdout)
        .map_err(|e| WhisperError::JsonError(e.to_string()))?;
    Ok(list.models)
}

pub fn download_model(model: &str, cache_dir: Option<String>) -> Result<(), WhisperError> {
    let python = get_python()?;
    let script = super::script_path("transcribe.py");
    let cache = cache_dir.map(PathBuf::from).unwrap_or_else(default_cache_dir);

    let output = Command::new(&python).hide_window()
        .args([
            &script,
            "download",
            "--model",
            model,
            "--cache-dir",
            &cache.to_string_lossy(),
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(WhisperError::ScriptFailed(stderr));
    }

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TranscribeOptions {
    pub input: String,
    pub output: String,
    pub model: String,
    pub device: String,
    pub language: Option<String>,
    pub format: String,
    pub word_timestamps: bool,
    pub vad_filter: bool,
    pub cache_dir: Option<String>,
}

pub fn build_transcribe_args(options: &TranscribeOptions) -> Vec<String> {
    let script = super::script_path("transcribe.py");
    let cache = options
        .cache_dir
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(default_cache_dir);

    let mut args = vec![
        script,
        "transcribe".to_string(),
        "--input".to_string(),
        options.input.clone(),
        "--output".to_string(),
        options.output.clone(),
        "--model".to_string(),
        options.model.clone(),
        "--device".to_string(),
        options.device.clone(),
        "--format".to_string(),
        options.format.clone(),
        "--cache-dir".to_string(),
        cache.to_string_lossy().to_string(),
    ];

    if let Some(lang) = &options.language {
        args.push("--language".to_string());
        args.push(lang.clone());
    }

    if options.word_timestamps {
        args.push("--word-timestamps".to_string());
    }

    if options.vad_filter {
        args.push("--vad-filter".to_string());
    }

    args
}
