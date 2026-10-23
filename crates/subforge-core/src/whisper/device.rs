//! Compute device selection and engine information.
//!
//! This build is CPU only: `Auto` and `Gpu` both run on the CPU and report `"cpu"`.

use serde::{Deserialize, Serialize};

/// Requested compute device. Serialised lowercase; the legacy value `"cuda"` reads as `Auto`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Device {
    #[default]
    #[serde(alias = "cuda")]
    Auto,
    Cpu,
    Gpu,
}

/// What the transcription engine can do on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineInfo {
    /// `"whisper.cpp <version>"`.
    pub engine: String,
    /// Name of the GPU used for `Device::Gpu`, if any.
    pub gpu: Option<String>,
    /// Device values the UI may offer: `"auto"`, `"cpu"` and, with a GPU, `"gpu"`.
    pub devices: Vec<String>,
}

pub fn engine_info() -> EngineInfo {
    EngineInfo {
        engine: format!("whisper.cpp {}", whisper_rs::WHISPER_CPP_VERSION),
        gpu: None,
        devices: vec!["auto".to_string(), "cpu".to_string()],
    }
}

/// The device a request actually runs on, as reported in `TranscribeOutcome::device_used`.
pub(crate) fn resolve(_requested: Device) -> &'static str {
    "cpu"
}
