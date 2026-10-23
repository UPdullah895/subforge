//! Compute device selection and engine information.
//!
//! The CPU always works. With the `gpu-vulkan` feature, ggml's Vulkan backend is enumerated and
//! `Auto`/`Gpu` use the first GPU it finds; without the feature, or without a Vulkan runtime, they
//! run on the CPU and report `"cpu"`.

use std::sync::OnceLock;

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

/// Name of the GPU whisper.cpp would use, or `None` (no GPU, no Vulkan runtime, or a build
/// without `gpu-vulkan`). Never panics. The result is computed once per process.
pub fn detect_gpu() -> Option<String> {
    static GPU: OnceLock<Option<String>> = OnceLock::new();
    GPU.get_or_init(detect_gpu_uncached).clone()
}

#[cfg(feature = "gpu-vulkan")]
fn detect_gpu_uncached() -> Option<String> {
    // ggml's Vulkan backend throws a C++ exception, which aborts the process, when the machine
    // has a loader but no driver (ICD). Probe with plain Vulkan calls first and only ask ggml
    // when a physical device exists. A panic in the probe must not take the app down either.
    std::panic::catch_unwind(|| {
        if !vulkan_probe() {
            return None;
        }
        whisper_rs::vulkan::list_devices()
            .into_iter()
            .map(|d| d.name.trim().to_string())
            .find(|n| !n.is_empty())
    })
    .ok()
    .flatten()
}

/// True when the Vulkan loader loads, an instance can be created and a physical device exists.
#[cfg(feature = "gpu-vulkan")]
fn vulkan_probe() -> bool {
    use ash::{vk, Entry};

    // SAFETY: loading the system Vulkan loader and making plain, argument-checked calls to it.
    unsafe {
        let Ok(entry) = Entry::load() else {
            return false;
        };
        let info = vk::InstanceCreateInfo::default();
        let Ok(instance) = entry.create_instance(&info, None) else {
            return false;
        };
        let found = instance
            .enumerate_physical_devices()
            .map(|d| !d.is_empty())
            .unwrap_or(false);
        instance.destroy_instance(None);
        found
    }
}

#[cfg(not(feature = "gpu-vulkan"))]
fn detect_gpu_uncached() -> Option<String> {
    None
}

pub fn engine_info() -> EngineInfo {
    let gpu = detect_gpu();
    let mut devices = vec!["auto".to_string(), "cpu".to_string()];
    if gpu.is_some() {
        devices.push("gpu".to_string());
    }
    EngineInfo {
        engine: format!("whisper.cpp {}", whisper_rs::WHISPER_CPP_VERSION),
        gpu,
        devices,
    }
}

/// Picks the device a request runs on, given the GPU that is available (if any). Returns the
/// device and the string reported as `TranscribeOutcome::device_used`. `Gpu` without a GPU falls
/// back to the CPU.
pub fn select_device(requested: Device, available_gpu: Option<&str>) -> (Device, String) {
    match (requested, available_gpu) {
        (Device::Auto | Device::Gpu, Some(name)) => (Device::Gpu, format!("gpu: {name}")),
        _ => (Device::Cpu, "cpu".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_device_falls_back_to_cpu() {
        assert_eq!(
            select_device(Device::Auto, None),
            (Device::Cpu, "cpu".into())
        );
        assert_eq!(
            select_device(Device::Gpu, None),
            (Device::Cpu, "cpu".into())
        );
        assert_eq!(
            select_device(Device::Auto, Some("RTX")),
            (Device::Gpu, "gpu: RTX".into())
        );
        assert_eq!(
            select_device(Device::Gpu, Some("RTX")),
            (Device::Gpu, "gpu: RTX".into())
        );
        assert_eq!(
            select_device(Device::Cpu, Some("RTX")),
            (Device::Cpu, "cpu".into())
        );
    }

    #[test]
    fn engine_info_is_consistent_with_detection() {
        let info = engine_info();
        assert!(info.devices.starts_with(&["auto".into(), "cpu".into()]));
        assert_eq!(
            info.devices.contains(&"gpu".to_string()),
            info.gpu.is_some()
        );
        #[cfg(not(feature = "gpu-vulkan"))]
        assert!(info.gpu.is_none());
    }
}
