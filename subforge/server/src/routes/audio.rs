use crate::tools::ffmpeg;
use axum::{extract::Json, http::StatusCode};
use serde::{Deserialize, Serialize};

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
) -> Result<Json<Vec<AudioTrackInfo>>, StatusCode> {
    let streams = ffmpeg::probe_audio_tracks(&req.file_path).map_err(|_| StatusCode::BAD_REQUEST)?;
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

pub async fn extract_audio_track(Json(req): Json<ExtractRequest>) -> Result<StatusCode, StatusCode> {
    ffmpeg::extract_audio(&req.file_path, req.track_index, &req.output_path, req.format.as_deref())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct MergeRequest {
    video_path: String,
    audio_path: String,
    output_path: String,
}

pub async fn merge_audio_track(Json(req): Json<MergeRequest>) -> Result<StatusCode, StatusCode> {
    ffmpeg::merge_audio(&req.video_path, &req.audio_path, &req.output_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct RemoveRequest {
    file_path: String,
    output_path: String,
}

pub async fn remove_audio_tracks(Json(req): Json<RemoveRequest>) -> Result<StatusCode, StatusCode> {
    crate::tools::mkvtoolnix::remove_all_audio(&req.file_path, &req.output_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}
