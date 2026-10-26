pub mod commands;
pub mod config;
pub mod credentials;
pub mod jobs;
pub mod secrets;
pub mod self_test;
pub mod tools;

use commands::{
    audio as audio_cmds, batch as batch_cmds, deps as deps_cmds, jobs as jobs_cmds,
    providers as providers_cmds, rename as rename_cmds, subtitle as subtitle_cmds,
    subtitle_sources as subtitle_sources_cmds, transcribe as transcribe_cmds,
    translate as translate_cmds,
};
use credentials::{CredentialStore, KeyringCredentialStore};
use jobs::{JobControlRegistry, SharedJobQueue};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let context = tauri::generate_context!();
    // Hidden CI entry point; exits before any window or plugin is created.
    if let Some(args) = self_test::parse(std::env::args_os().skip(1)) {
        std::process::exit(self_test::run(args, context.package_info()));
    }

    #[cfg(target_os = "linux")]
    {
        // SAFETY: called before any threads are spawned, at the very start of run().
        unsafe {
            // Works around a WebKitGTK/Wayland compositor compatibility issue
            // ("Error 71 (Protocol error) dispatching to Wayland display") seen on
            // some Linux desktops (e.g. KWin/Wayland) with DMA-BUF rendering.
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");

            // `decorations: false` in tauri.conf.json means the WM draws no
            // frame at all — the app's own Vue TitleBar component is the only
            // titlebar. GTK_CSD is not needed here (there's no OS-drawn frame
            // to conflict with either way).

            // tauri-plugin-dialog's Linux backend only opens the desktop's
            // native file picker (e.g. KDE's Dolphin/KFileDialog via
            // xdg-desktop-portal) for non-sandboxed apps when this is set;
            // otherwise it silently falls back to a plain GTK dialog that
            // looks out of place on non-GNOME desktops.
            std::env::set_var("GTK_USE_PORTAL", "1");
        }
    }

    let job_queue: SharedJobQueue = Arc::new(Mutex::new(jobs::JobQueue::new()));
    let job_controls: JobControlRegistry = Arc::new(Mutex::new(HashMap::new()));
    let credential_store: Arc<dyn CredentialStore> = Arc::new(KeyringCredentialStore);

    tauri::Builder::default()
        .manage(job_queue)
        .manage(job_controls)
        .manage(subforge_core::whisper::ActiveDownloads::default())
        .manage(credential_store)
        .setup(|app| {
            config::migrate_saved_config();
            // Tools shipped with the app (Windows installer and AppImage; deb/rpm use system
            // packages). `self_test::run` resolves them the same way.
            if let Ok(resources) = app.path().resource_dir() {
                let bin = resources.join("bin");
                if bin.is_dir() {
                    tools::set_bundled_dir(bin);
                }
            }
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            deps_cmds::check_dependencies,
            deps_cmds::install_dependency,
            subtitle_cmds::enumerate_tracks,
            subtitle_cmds::extract_subtitle,
            subtitle_cmds::extract_all_subtitles,
            subtitle_cmds::merge_subtitle_into_video,
            subtitle_cmds::remove_subtitle_tracks,
            subtitle_cmds::convert_subtitle_format,
            subtitle_cmds::shift_subtitle_timing,
            subtitle_cmds::stretch_subtitle_timing,
            subtitle_cmds::clean_subtitle,
            subtitle_cmds::set_track_metadata,
            subtitle_cmds::get_tool_registry,
            subtitle_cmds::get_tool_status,
            subtitle_cmds::load_config,
            subtitle_cmds::save_config,
            audio_cmds::list_audio_tracks,
            audio_cmds::extract_audio_track,
            audio_cmds::merge_audio_track,
            audio_cmds::remove_audio_tracks,
            batch_cmds::scan_directory,
            batch_cmds::batch_extract_subtitles,
            batch_cmds::merge_external_subs,
            transcribe_cmds::get_whisper_info,
            transcribe_cmds::list_whisper_models,
            transcribe_cmds::download_whisper_model,
            transcribe_cmds::list_whisper_catalog,
            transcribe_cmds::whisper_legacy_cache,
            transcribe_cmds::delete_whisper_legacy_cache,
            transcribe_cmds::transcribe_audio,
            translate_cmds::detect_ollama,
            translate_cmds::detect_libretranslate,
            translate_cmds::translate_subtitles,
            translate_cmds::run_translation_pipeline,
            rename_cmds::parse_filename,
            rename_cmds::search_series,
            rename_cmds::get_episodes,
            rename_cmds::preview_rename,
            rename_cmds::apply_rename,
            providers_cmds::test_provider_config,
            subtitle_sources_cmds::search_online_subtitle,
            subtitle_sources_cmds::set_opensubtitles_credentials,
            subtitle_sources_cmds::clear_opensubtitles_credentials,
            subtitle_sources_cmds::get_opensubtitles_status,
            subtitle_sources_cmds::check_subsource_status,
            jobs_cmds::cancel_job,
            jobs_cmds::pause_job,
            jobs_cmds::resume_job,
        ])
        .run(context)
        .expect("error while running tauri application");
}
