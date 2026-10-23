//! Compute device selection and engine information.
//!
//! The CPU always works. With the `gpu-vulkan` feature, ggml's Vulkan backend is enumerated and
//! `Auto`/`Gpu` use the first GPU it finds; without the feature, or without a Vulkan runtime, they
//! run on the CPU and report `"cpu"`.

use std::sync::{Mutex, MutexGuard, Once, OnceLock};

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

/// Serialises everything that may initialise ggml's backend registry or Vulkan instance
/// (`ggml_vk_instance_init` is an unsynchronised check-then-set on a static, reached from GPU
/// detection and from `ggml_backend_vk_reg()` during any model load).
pub(crate) fn ggml_init_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

const VULKAN_RUNTIME_MISSING: &str = "The Vulkan runtime (vulkan-1.dll) is missing from this SubForge installation. Reinstall SubForge.";

/// Pure decision behind [`check_vulkan_runtime`]: when the loader is `required`, it must be
/// loadable, otherwise any ggml call would crash the process (see the build.rs files).
fn runtime_check(required: bool, loadable: impl FnOnce() -> bool) -> Result<(), &'static str> {
    if required && !loadable() {
        Err(VULKAN_RUNTIME_MISSING)
    } else {
        Ok(())
    }
}

/// Windows builds with `gpu-vulkan` delay-load `vulkan-1.dll`, but ggml touches Vulkan on EVERY
/// model load (CPU included), and a missing DLL then raises an SEH exception that kills the
/// process. Check the loader first (cached) so the user gets an error instead. Always `Ok` on
/// other platforms and without the feature.
pub(crate) fn check_vulkan_runtime() -> Result<(), &'static str> {
    #[cfg(all(windows, feature = "gpu-vulkan"))]
    {
        static LOADABLE: OnceLock<bool> = OnceLock::new();
        runtime_check(true, || {
            *LOADABLE.get_or_init(|| unsafe { ash::Entry::load() }.is_ok())
        })
    }
    #[cfg(not(all(windows, feature = "gpu-vulkan")))]
    {
        runtime_check(false, || true)
    }
}

/// Name of the GPU whisper.cpp would use, or `None` (no GPU, no Vulkan runtime, or a build
/// without `gpu-vulkan`). The result is computed once per process and cached, so a GPU or
/// driver that appears later is not noticed until restart. Blocking on first use: call from
/// `spawn_blocking`.
pub fn detect_gpu() -> Option<String> {
    static GPU: OnceLock<Option<String>> = OnceLock::new();
    GPU.get_or_init(detect_gpu_uncached).clone()
}

#[cfg(feature = "gpu-vulkan")]
fn detect_gpu_uncached() -> Option<String> {
    // ggml's Vulkan backend throws a C++ exception, which aborts the process (it cannot be
    // caught from Rust), when the machine has a loader but no driver (ICD). So probe with plain
    // Vulkan calls first and only ask ggml when a physical device exists. `catch_unwind` only
    // covers Rust panics. A Vulkan failure during real device initialisation is likewise not
    // recoverable in-process.
    if check_vulkan_runtime().is_err() {
        return None;
    }
    std::panic::catch_unwind(|| {
        // The loader's first-use ICD discovery races with ggml's own first use (seen as a
        // SIGSEGV inside libvulkan), so the probe takes the same lock as model loads.
        let _guard = ggml_init_lock();
        if !vulkan_probe() {
            return None;
        }
        // Count and description only enumerate physical devices and read their properties; they
        // do not create a VkDevice (unlike `whisper_rs::vulkan::list_devices`).
        // SAFETY: plain ggml queries; the buffer is larger than ggml's documented 128 bytes.
        unsafe {
            if whisper_rs_sys::ggml_backend_vk_get_device_count() < 1 {
                return None;
            }
            let mut buf: [std::os::raw::c_char; 256] = [0; 256];
            whisper_rs_sys::ggml_backend_vk_get_device_description(0, buf.as_mut_ptr(), buf.len());
            let name = std::ffi::CStr::from_ptr(buf.as_ptr())
                .to_string_lossy()
                .trim()
                .to_string();
            (!name.is_empty()).then_some(name)
        }
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

// whisper.cpp / ggml log lines, captured so a load can be inspected afterwards (which backend
// whisper really picked, why a GPU failed). Nothing is printed.
const LOG_KEEP: usize = 400;
static ENGINE_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

unsafe extern "C" fn log_callback(
    _level: whisper_rs_sys::ggml_log_level,
    text: *const std::os::raw::c_char,
    _user: *mut std::os::raw::c_void,
) {
    if text.is_null() {
        return;
    }
    // SAFETY: ggml passes a NUL-terminated string valid for the call.
    let line = unsafe { std::ffi::CStr::from_ptr(text) }.to_string_lossy();
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    if let Ok(mut log) = ENGINE_LOG.lock() {
        if log.len() >= LOG_KEEP {
            log.drain(..LOG_KEEP / 2);
        }
        log.push(line.to_string());
    }
}

/// Routes whisper.cpp / ggml logging into the capture buffer instead of stderr (once).
pub(crate) fn install_log_capture() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: `log_callback` is a valid callback for the life of the process.
        unsafe { whisper_rs_sys::whisper_log_set(Some(log_callback), std::ptr::null_mut()) };
    });
}

/// Forgets captured log lines; call before a load whose log you want to inspect.
pub(crate) fn clear_engine_log() {
    if let Ok(mut log) = ENGINE_LOG.lock() {
        log.clear();
    }
}

/// Captured lines since the last [`clear_engine_log`].
pub(crate) fn engine_log() -> Vec<String> {
    ENGINE_LOG.lock().map(|l| l.clone()).unwrap_or_default()
}

/// What a context load really used, from the captured whisper.cpp log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Backend {
    Gpu,
    Cpu,
}

/// `Gpu` only when whisper.cpp logged that it initialised a GPU backend.
pub(crate) fn backend_from_log(lines: &[String]) -> Backend {
    let used = lines
        .iter()
        .any(|l| l.contains("whisper_backend_init_gpu: using "));
    let failed = lines
        .iter()
        .any(|l| l.contains("whisper_backend_init_gpu: failed to initialize"));
    if used && !failed {
        Backend::Gpu
    } else {
        Backend::Cpu
    }
}

/// The last line of a log that reports a GPU backend failure (whisper's own failure messages,
/// not ordinary `Vulkan0 total size` lines), for messages.
pub(crate) fn gpu_failure_line(lines: &[String]) -> Option<String> {
    lines
        .iter()
        .rev()
        .find(|l| {
            let l = l.to_ascii_lowercase();
            l.contains("failed to initialize") || l.contains("no gpu found")
        })
        .cloned()
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
    fn backend_from_log_reads_whisper_messages() {
        let l = |s: &str| s.to_string();
        let ok = vec![
            l("whisper_backend_init_gpu: found GPU device 0: Vulkan0 (type: 1, cnt: 0)"),
            l("whisper_backend_init_gpu: using Vulkan0 backend"),
        ];
        assert_eq!(backend_from_log(&ok), Backend::Gpu);
        let none = vec![l("whisper_backend_init_gpu: no GPU found")];
        assert_eq!(backend_from_log(&none), Backend::Cpu);
        let failed = vec![
            l("whisper_backend_init_gpu: using Vulkan0 backend"),
            l("whisper_backend_init_gpu: failed to initialize Vulkan0 backend"),
        ];
        assert_eq!(backend_from_log(&failed), Backend::Cpu);
        assert!(gpu_failure_line(&failed)
            .unwrap()
            .contains("failed to initialize"));
        assert_eq!(backend_from_log(&[]), Backend::Cpu);
        let normal = vec![l(
            "whisper_model_load:     Vulkan0 total size =    77.11 MB",
        )];
        assert_eq!(gpu_failure_line(&normal), None);
        assert_eq!(
            gpu_failure_line(&none).as_deref(),
            Some("whisper_backend_init_gpu: no GPU found")
        );
    }

    #[test]
    fn runtime_check_decides_on_the_loader() {
        // Not required (Linux, or a build without gpu-vulkan): the loader is never probed.
        assert_eq!(runtime_check(false, || panic!("must not probe")), Ok(()));
        assert_eq!(runtime_check(true, || true), Ok(()));
        let err = runtime_check(true, || false).unwrap_err();
        assert!(
            err.contains("vulkan-1.dll") && err.contains("Reinstall"),
            "{err}"
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
