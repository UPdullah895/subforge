// Vulkan loader linkage (cargo feature `gpu-vulkan`):
// - Windows (MSVC): vulkan-1.dll is delay-loaded, so the app can START without it. ggml touches
//   Vulkan on every model load (CPU included), so subforge-core checks that the loader can be
//   loaded first (`check_vulkan_runtime`) and fails the transcription with an error instead of
//   crashing. The server binary is not shipped by the installer, so vulkan-1.dll comes from
//   the GPU driver (or a Vulkan runtime install) on the host.
// - Linux: ggml hard-links libvulkan.so.1 (NEEDED), so the host needs the Vulkan loader
//   package (libvulkan1 / vulkan-loader) installed to run a `gpu-vulkan` server build.
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
}
