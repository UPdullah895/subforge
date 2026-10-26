// Vulkan loader linkage (cargo feature `gpu-vulkan`):
// - Windows (MSVC): vulkan-1.dll is delay-loaded, so the app can START without it. ggml touches
//   Vulkan on every model load (CPU included), so subforge-core checks that the loader can be
//   loaded first (`check_vulkan_runtime`) and fails the transcription with a "reinstall" error
//   instead of crashing. The installer bundles the DLL next to the exe, so that is the
//   exceptional path.
// - Linux: ggml hard-links libvulkan.so.1 (NEEDED). The deb/rpm/AUR packages depend on it; the
//   AppImage bundles it (tauri.linux.conf.json, from scripts/fetch-bundled-tools.mjs).
fn vulkan_link_args() {
    let windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if windows_msvc && std::env::var_os("CARGO_FEATURE_GPU_VULKAN").is_some() {
        println!("cargo:rustc-link-arg=/DELAYLOAD:vulkan-1.dll");
        println!("cargo:rustc-link-lib=delayimp");
    }
}

// tauri.windows.conf.json maps these folders as resources and tauri-build fails on a missing
// path. Release builds fill them with scripts/fetch-bundled-tools.mjs; for every other build
// (dev, `cargo check`) they stay empty, so the app falls back to the system tools.
fn ensure_bundled_dirs() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        for dir in ["binaries/bin", "binaries/licenses", "binaries/beside-exe"] {
            std::fs::create_dir_all(dir).expect("create src-tauri/binaries folder");
        }
    }
}

fn main() {
    vulkan_link_args();
    ensure_bundled_dirs();
    tauri_build::build()
}
