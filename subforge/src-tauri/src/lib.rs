pub mod commands;
pub mod config;
pub mod jobs;
pub mod tools;

use commands::{
    audio as audio_cmds, batch as batch_cmds, subtitle as subtitle_cmds,
    transcribe as transcribe_cmds, translate as translate_cmds,
};
use jobs::SharedJobQueue;
use std::sync::{Arc, Mutex};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
            subtitle_cmds::get_tool_registry,
            subtitle_cmds::load_config,
            subtitle_cmds::save_config,
            audio_cmds::list_audio_tracks,
            audio_cmds::extract_audio_track,
            audio_cmds::merge_audio_track,
            audio_cmds::remove_audio_tracks,
            batch_cmds::scan_directory,
            batch_cmds::batch_extract_subtitles,
            transcribe_cmds::get_whisper_info,
            transcribe_cmds::list_whisper_models,
            transcribe_cmds::download_whisper_model,
            transcribe_cmds::transcribe_audio,
            translate_cmds::detect_ollama,
            translate_cmds::detect_libretranslate,
            translate_cmds::translate_subtitles,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
