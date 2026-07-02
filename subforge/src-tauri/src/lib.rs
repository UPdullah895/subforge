pub mod commands;
pub mod config;
pub mod jobs;
pub mod tools;

use commands::{
    audio as audio_cmds, batch as batch_cmds, providers as providers_cmds, rename as rename_cmds,
    subtitle as subtitle_cmds, transcribe as transcribe_cmds, translate as translate_cmds,
};
use jobs::SharedJobQueue;
use std::sync::{Arc, Mutex};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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

    tauri::Builder::default()
        .manage(job_queue)
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            subtitle_cmds::enumerate_tracks,
            subtitle_cmds::extract_subtitle,
            subtitle_cmds::extract_all_subtitles,
            subtitle_cmds::merge_subtitle_into_video,
            subtitle_cmds::remove_subtitle_tracks,
            subtitle_cmds::convert_subtitle_format,
            subtitle_cmds::shift_subtitle_timing,
            subtitle_cmds::set_track_metadata,
            subtitle_cmds::get_tool_registry,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
