use super::{blocking, CommandError};
use crate::tools::tool_paths;
use serde::{Deserialize, Serialize};
use subforge_core::tools::ffmpeg::{self, FfprobeStream};
use subforge_core::tools::mkvtoolnix;

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
pub async fn list_audio_tracks(file_path: String) -> Result<Vec<AudioTrackInfo>, CommandError> {
    blocking(move || {
        let streams =
            ffmpeg::probe_audio_tracks(&tool_paths(), &file_path).map_err(CommandError::from)?;
        Ok(streams.iter().map(|s| s.into()).collect())
    })
    .await
}

#[tauri::command]
pub async fn extract_audio_track(
    file_path: String,
    track_index: u64,
    output_path: String,
    format: Option<String>,
) -> Result<(), CommandError> {
    blocking(move || {
        ffmpeg::extract_audio(
            &tool_paths(),
            &file_path,
            track_index,
            &output_path,
            format.as_deref(),
        )
        .map_err(CommandError::from)
    })
    .await
}

#[tauri::command]
pub async fn merge_audio_track(
    video_path: String,
    audio_path: String,
    output_path: String,
) -> Result<(), CommandError> {
    blocking(move || {
        ffmpeg::merge_audio(&tool_paths(), &video_path, &audio_path, &output_path)
            .map_err(CommandError::from)
    })
    .await
}

#[tauri::command]
pub async fn remove_audio_tracks(
    file_path: String,
    output_path: String,
) -> Result<(), CommandError> {
    blocking(move || {
        mkvtoolnix::remove_all_audio(&tool_paths(), &file_path, &output_path)
            .map_err(CommandError::from)
    })
    .await
}
