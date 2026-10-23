// Vulkan loader linkage (cargo feature `gpu-vulkan`):
// - Linux: ggml links libvulkan.so.1 at build time (NEEDED). The deb/rpm/AUR packages depend on
//   the loader and the AppImage bundles it, so it is always present.
// - Windows (MSVC): delay-load vulkan-1.dll so the app still starts where the DLL is missing.
//   `detect_gpu` probes the loader with `ash` (a run-time `LoadLibrary`) before any ggml Vulkan
//   call; no loader means no GPU and the CPU path, which never calls into Vulkan.
fn vulkan_link_args() {
    let windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if windows_msvc && std::env::var_os("CARGO_FEATURE_GPU_VULKAN").is_some() {
        println!("cargo:rustc-link-arg=/DELAYLOAD:vulkan-1.dll");
        println!("cargo:rustc-link-lib=delayimp");
    }
}

fn main() {
    vulkan_link_args();
    tauri_build::build()
}
