//! FFmpeg / ffprobe wrappers (audio probing, extraction and merging).

use std::process::Command;

use serde::{Deserialize, Serialize};
use subforge_deps::HideWindow;

use super::{resolve, ToolError, ToolId, ToolPaths};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfprobeStream {
    pub index: u64,
    #[serde(rename = "codec_type")]
    pub codec_type: String,
    pub codec_name: Option<String>,
    pub codec_long_name: Option<String>,
    #[serde(default)]
    pub tags: Option<FfprobeTags>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfprobeTags {
    pub language: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfprobeResult {
    pub streams: Vec<FfprobeStream>,
}

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

pub fn probe_audio_tracks(
    paths: &ToolPaths,
    video_path: &str,
) -> Result<Vec<FfprobeStream>, ToolError> {
    let ffprobe = resolve(ToolId::Ffprobe, paths)?;

    let output = Command::new(&ffprobe.path)
        .hide_window()
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=index,codec_type,codec_name,codec_long_name:stream_tags=language,title",
            "-of",
            "json",
            video_path,
        ])
        .output()?;

    if !output.status.success() {
        return Err(ToolError::failed(
            ToolId::Ffprobe,
            &output.status,
            stderr_text(&output),
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: FfprobeResult =
        serde_json::from_str(&stdout).map_err(|e| ToolError::Json(e.to_string()))?;

    Ok(result.streams)
}

pub fn extract_audio(
    paths: &ToolPaths,
    video_path: &str,
    track_index: u64,
    output_path: &str,
    format: Option<&str>,
) -> Result<(), ToolError> {
    let ffmpeg = resolve(ToolId::Ffmpeg, paths)?;

    // `track_index` is the stream's absolute index as ffprobe reports it (that
    // is what the track list shows and sends back), so map by absolute index.
    // `0:a:N` would be the N-th *audio* stream and picks the wrong track, or
    // none at all, in any file where audio isn't the first stream.
    let source_codec = probe_audio_tracks(paths, video_path)
        .ok()
        .and_then(|streams| streams.into_iter().find(|s| s.index == track_index))
        .and_then(|s| s.codec_name);

    let mut args: Vec<String> = vec![
        "-hide_banner".to_string(),
        "-v".to_string(),
        "error".to_string(),
        "-y".to_string(),
        "-nostdin".to_string(),
        "-i".to_string(),
        video_path.to_string(),
        "-map".to_string(),
        format!("0:{}", track_index),
        "-vn".to_string(),
    ];

    let fmt = format.map(|f| f.to_lowercase());
    // Stream-copy (lossless, instant) when the track already has the codec the
    // chosen format implies; otherwise encode.
    let copy_if = |codec: &str| source_codec.as_deref() == Some(codec);
    let codec_args: Vec<&str> = match fmt.as_deref() {
        Some("aac") if copy_if("aac") => vec!["-c:a", "copy"],
        Some("aac") => vec!["-c:a", "aac", "-b:a", "256k"],
        Some("mp3") if copy_if("mp3") => vec!["-c:a", "copy"],
        Some("mp3") => vec!["-c:a", "libmp3lame", "-q:a", "2"],
        Some("opus") if copy_if("opus") => vec!["-c:a", "copy"],
        Some("opus") => vec!["-c:a", "libopus", "-b:a", "128k"],
        Some("flac") => vec!["-c:a", "flac"],
        Some("wav") => vec!["-c:a", "pcm_s16le"],
        _ => vec!["-c:a", "copy"],
    };
    args.extend(codec_args.into_iter().map(String::from));
    args.push(output_path.to_string());

    let output = Command::new(&ffmpeg.path)
        .hide_window()
        .args(&args)
        .output()?;

    if !output.status.success() {
        return Err(ToolError::failed(
            ToolId::Ffmpeg,
            &output.status,
            stderr_text(&output),
        ));
    }

    Ok(())
}

pub fn merge_audio(
    paths: &ToolPaths,
    video_path: &str,
    audio_path: &str,
    output_path: &str,
) -> Result<(), ToolError> {
    let ffmpeg = resolve(ToolId::Ffmpeg, paths)?;

    let output = Command::new(&ffmpeg.path)
        .hide_window()
        .args([
            "-hide_banner",
            "-v",
            "error",
            "-y",
            "-nostdin",
            "-i",
            video_path,
            "-i",
            audio_path,
            "-c",
            "copy",
            "-map",
            "0:v:0",
            "-map",
            "0:a?",
            "-map",
            "1:a:0",
            output_path,
        ])
        .output()?;

    if !output.status.success() {
        return Err(ToolError::failed(
            ToolId::Ffmpeg,
            &output.status,
            stderr_text(&output),
        ));
    }

    Ok(())
}

/// Container-level facts used to sanity-check a subtitle against its video.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MediaInfo {
    /// Container duration in milliseconds, when ffprobe reports one.
    pub duration_ms: Option<i64>,
    /// Nominal frame rate of the first video stream (e.g. 23.976).
    pub fps: Option<f64>,
}

#[derive(Deserialize)]
struct FfprobeMedia {
    #[serde(default)]
    streams: Vec<FfprobeMediaStream>,
    #[serde(default)]
    format: Option<FfprobeFormat>,
}

#[derive(Deserialize)]
struct FfprobeMediaStream {
    codec_type: String,
    #[serde(default)]
    avg_frame_rate: Option<String>,
    #[serde(default)]
    r_frame_rate: Option<String>,
}

#[derive(Deserialize)]
struct FfprobeFormat {
    #[serde(default)]
    duration: Option<String>,
}

fn parse_rate(value: &str) -> Option<f64> {
    let (num, den) = value.split_once('/')?;
    let num: f64 = num.parse().ok()?;
    let den: f64 = den.parse().ok()?;
    (den > 0.0 && num > 0.0).then_some(num / den)
}

/// Duration and frame rate of a media file.
pub fn probe_media(paths: &ToolPaths, path: &str) -> Result<MediaInfo, ToolError> {
    let ffprobe = resolve(ToolId::Ffprobe, paths)?;
    let output = Command::new(&ffprobe.path)
        .hide_window()
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type,avg_frame_rate,r_frame_rate",
            "-of",
            "json",
            path,
        ])
        .output()?;
    if !output.status.success() {
        return Err(ToolError::failed(
            ToolId::Ffprobe,
            &output.status,
            stderr_text(&output),
        ));
    }
    let parsed: FfprobeMedia =
        serde_json::from_slice(&output.stdout).map_err(|e| ToolError::Json(e.to_string()))?;

    let duration_ms = parsed
        .format
        .and_then(|f| f.duration)
        .and_then(|d| d.parse::<f64>().ok())
        .filter(|d| d.is_finite() && *d > 0.0)
        .map(|d| (d * 1000.0).round() as i64);

    let fps = parsed
        .streams
        .iter()
        .find(|s| s.codec_type == "video")
        .and_then(|s| {
            s.avg_frame_rate
                .as_deref()
                .and_then(parse_rate)
                .filter(|f| *f > 1.0 && *f < 1000.0)
                .or_else(|| s.r_frame_rate.as_deref().and_then(parse_rate))
        });

    Ok(MediaInfo { duration_ms, fps })
}

/// Decode `duration_ms` of audio starting at `start_ms` to 16 kHz mono WAV.
/// Used to align short speech windows against the subtitle.
pub fn extract_audio_snippet(
    paths: &ToolPaths,
    input: &str,
    output: &str,
    start_ms: i64,
    duration_ms: i64,
) -> Result<(), ToolError> {
    let ffmpeg = resolve(ToolId::Ffmpeg, paths)?;
    let start = format!("{:.3}", (start_ms.max(0) as f64) / 1000.0);
    let duration = format!("{:.3}", (duration_ms.max(100) as f64) / 1000.0);
    let output_proc = Command::new(&ffmpeg.path)
        .hide_window()
        .args([
            "-hide_banner",
            "-v",
            "error",
            "-y",
            "-nostdin",
            "-ss",
            &start,
            "-t",
            &duration,
            "-i",
            input,
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
            output,
        ])
        .output()?;
    if !output_proc.status.success() {
        return Err(ToolError::failed(
            ToolId::Ffmpeg,
            &output_proc.status,
            stderr_text(&output_proc),
        ));
    }
    Ok(())
}
