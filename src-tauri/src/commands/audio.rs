use crate::tools::ffmpeg::{self, FfprobeStream};
use crate::tools::mkvtoolnix;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioTrackInfo {
    pub id: u64,
    pub codec: String,
    pub codec_long_name: Option<String>,
    pub language: String,
    pub title: Option<String>,
}

impl From<&FfprobeStream> for AudioTrackInfo {
    fn from(s: &FfprobeStream) -> Self {
        Self {
            id: s.index,
            codec: s.codec_name.clone().unwrap_or_default(),
            codec_long_name: s.codec_long_name.clone(),
            language: s
                .tags
                .as_ref()
                .and_then(|t| t.language.clone())
                .unwrap_or_else(|| "und".to_string()),
            title: s.tags.as_ref().and_then(|t| t.title.clone()),
        }
    }
}

#[tauri::command]
pub fn list_audio_tracks(file_path: String) -> Result<Vec<AudioTrackInfo>, String> {
    let streams = ffmpeg::probe_audio_tracks(&file_path).map_err(|e| e.to_string())?;
    Ok(streams.iter().map(|s| s.into()).collect())
}

#[tauri::command]
pub fn extract_audio_track(
    file_path: String,
    track_index: u64,
    output_path: String,
    format: Option<String>,
) -> Result<(), String> {
    ffmpeg::extract_audio(&file_path, track_index, &output_path, format.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn merge_audio_track(
    video_path: String,
    audio_path: String,
    output_path: String,
) -> Result<(), String> {
    ffmpeg::merge_audio(&video_path, &audio_path, &output_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_audio_tracks(
    file_path: String,
    output_path: String,
) -> Result<(), String> {
    mkvtoolnix::remove_all_audio(&file_path, &output_path).map_err(|e| e.to_string())
}
