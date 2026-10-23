use crate::error::AppError;
use crate::tools::tool_paths;
use axum::{extract::Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use subforge_core::tools::{ffmpeg, mkvtoolnix};

#[derive(Serialize)]
pub struct AudioTrackInfo {
    id: u64,
    codec: String,
    codec_long_name: Option<String>,
    language: String,
    title: Option<String>,
}

#[derive(Deserialize)]
pub struct TrackRequest {
    file_path: String,
}

pub async fn list_audio_tracks(
    Json(req): Json<TrackRequest>,
) -> Result<Json<Vec<AudioTrackInfo>>, AppError> {
    let streams =
        crate::error::blocking(move || ffmpeg::probe_audio_tracks(&tool_paths(), &req.file_path))
            .await?
            .map_err(AppError::from)?;
    let tracks: Vec<AudioTrackInfo> = streams
        .iter()
        .map(|s| AudioTrackInfo {
            id: s.index,
            codec: s.codec_name.clone().unwrap_or_default(),
            codec_long_name: s.codec_long_name.clone(),
            language: s
                .tags
                .as_ref()
                .and_then(|t| t.language.clone())
                .unwrap_or_else(|| "und".to_string()),
            title: s.tags.as_ref().and_then(|t| t.title.clone()),
        })
        .collect();
    Ok(Json(tracks))
}

#[derive(Deserialize)]
pub struct ExtractRequest {
    file_path: String,
    track_index: u64,
    output_path: String,
    format: Option<String>,
}

pub async fn extract_audio_track(Json(req): Json<ExtractRequest>) -> Result<StatusCode, AppError> {
    crate::error::blocking(move || {
        ffmpeg::extract_audio(
            &tool_paths(),
            &req.file_path,
            req.track_index,
            &req.output_path,
            req.format.as_deref(),
        )
    })
    .await?
    .map_err(AppError::from)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct MergeRequest {
    video_path: String,
    audio_path: String,
    output_path: String,
}

pub async fn merge_audio_track(Json(req): Json<MergeRequest>) -> Result<StatusCode, AppError> {
    crate::error::blocking(move || {
        ffmpeg::merge_audio(
            &tool_paths(),
            &req.video_path,
            &req.audio_path,
            &req.output_path,
        )
    })
    .await?
    .map_err(AppError::from)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct RemoveRequest {
    file_path: String,
    output_path: String,
}

pub async fn remove_audio_tracks(Json(req): Json<RemoveRequest>) -> Result<StatusCode, AppError> {
    crate::error::blocking(move || {
        mkvtoolnix::remove_all_audio(&tool_paths(), &req.file_path, &req.output_path)
    })
    .await?
    .map_err(AppError::from)?;
    Ok(StatusCode::OK)
}
