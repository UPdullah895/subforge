use crate::tools::mkvtoolnix;
use axum::{extract::Json, http::StatusCode};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct EnumerateRequest {
    file_path: String,
}

#[derive(Serialize)]
pub struct TrackInfo {
    id: u64,
    track_type: String,
    codec: String,
    language: String,
    track_name: Option<String>,
    default_track: bool,
    forced_track: bool,
}

pub async fn enumerate_tracks(
    Json(req): Json<EnumerateRequest>,
) -> Result<Json<Vec<TrackInfo>>, StatusCode> {
    let info = mkvtoolnix::enumerate_tracks(&req.file_path).map_err(|_| StatusCode::BAD_REQUEST)?;
    let tracks: Vec<TrackInfo> = info
        .tracks
        .iter()
        .map(|t| {
            let props = t.properties.as_ref();
            TrackInfo {
                id: t.id,
                track_type: t.track_type.clone(),
                codec: t.codec.clone(),
                language: props
                    .and_then(|p| p.language.clone())
                    .unwrap_or_else(|| "und".to_string()),
                track_name: props.and_then(|p| p.track_name.clone()),
                default_track: props.and_then(|p| p.default_track).unwrap_or(false),
                forced_track: props.and_then(|p| p.forced_track).unwrap_or(false),
            }
        })
        .collect();
    Ok(Json(tracks))
}

#[derive(Deserialize)]
pub struct ExtractRequest {
    file_path: String,
    track_id: u64,
    output_path: String,
}

pub async fn extract_subtitle(Json(req): Json<ExtractRequest>) -> Result<String, StatusCode> {
    mkvtoolnix::extract_subtitle_track(&req.file_path, req.track_id, &req.output_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(req.output_path)
}

#[derive(Deserialize)]
pub struct MergeRequest {
    video_path: String,
    subtitle_path: String,
    output_path: String,
    language: Option<String>,
}

pub async fn merge_subtitle(Json(req): Json<MergeRequest>) -> Result<StatusCode, StatusCode> {
    let subs = vec![mkvtoolnix::SubtitleMergeSpec::new(req.subtitle_path, req.language)];
    mkvtoolnix::merge_subtitles(&req.video_path, &subs, &req.output_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct RemoveRequest {
    file_path: String,
    output_path: String,
}

pub async fn remove_subtitles(Json(req): Json<RemoveRequest>) -> Result<StatusCode, StatusCode> {
    mkvtoolnix::remove_all_subtitles(&req.file_path, &req.output_path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct ConvertRequest {
    input: String,
    output: String,
    format: String,
}

pub async fn convert_format(Json(req): Json<ConvertRequest>) -> Result<StatusCode, StatusCode> {
    crate::tools::pysubs2::convert_format(&req.input, &req.output, &req.format)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct ShiftRequest {
    input: String,
    output: String,
    milliseconds: i64,
}

pub async fn shift_timing(Json(req): Json<ShiftRequest>) -> Result<StatusCode, StatusCode> {
    crate::tools::pysubs2::shift_subtitles(&req.input, &req.output, req.milliseconds)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
pub struct ExtractAllRequest {
    file_path: String,
    output_dir: String,
}

pub async fn extract_all_subtitles(
    Json(req): Json<ExtractAllRequest>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let paths = mkvtoolnix::extract_all_subtitles(&req.file_path, &req.output_dir)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(paths))
}

#[derive(Deserialize)]
pub struct SetTrackMetaRequest {
    file_path: String,
    track_id: u64,
    track_name: Option<String>,
    default_flag: Option<bool>,
    forced_flag: Option<bool>,
}

pub async fn set_track_metadata(
    Json(req): Json<SetTrackMetaRequest>,
) -> Result<StatusCode, StatusCode> {
    let mkvpropedit = crate::tools::which("mkvpropedit")
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let mut args: Vec<String> = vec![req.file_path.clone()];
    args.push("--edit".to_string());
    // mkvpropedit uses 1-indexed @N notation for tracks
    args.push(format!("track:@{}", req.track_id + 1));

    if let Some(name) = &req.track_name {
        args.push("--set".to_string());
        args.push(format!("name={}", name));
    }
    if let Some(default) = req.default_flag {
        args.push("--set".to_string());
        args.push(format!("flag-default={}", if default { 1 } else { 0 }));
    }
    if let Some(forced) = req.forced_flag {
        args.push("--set".to_string());
        args.push(format!("flag-forced={}", if forced { 1 } else { 0 }));
    }

    let output = std::process::Command::new(&mkvpropedit)
        .args(&args)
        .output()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if !output.status.success() {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    Ok(StatusCode::OK)
}
