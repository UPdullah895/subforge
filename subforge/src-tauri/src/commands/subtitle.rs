use crate::tools::mkvtoolnix::{self, Track};
use crate::tools::pysubs2;
use crate::tools::ToolRegistry;
use crate::config::AppConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackInfo {
    pub id: u64,
    pub track_type: String,
    pub codec: String,
    pub language: String,
    pub track_name: Option<String>,
    pub default_track: bool,
    pub forced_track: bool,
}

impl From<&Track> for TrackInfo {
    fn from(t: &Track) -> Self {
        let props = t.properties.as_ref();
        Self {
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
    }
}

#[tauri::command]
pub fn get_tool_registry() -> ToolRegistry {
    ToolRegistry::discover()
}

#[tauri::command]
pub fn load_config() -> AppConfig {
    AppConfig::load()
}

#[tauri::command]
pub fn save_config(config: AppConfig) -> Result<(), String> {
    config.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn enumerate_tracks(file_path: String) -> Result<Vec<TrackInfo>, String> {
    let info = mkvtoolnix::enumerate_tracks(&file_path).map_err(|e| e.to_string())?;
    let tracks: Vec<TrackInfo> = info.tracks.iter().map(|t| t.into()).collect();
    Ok(tracks)
}

#[tauri::command]
pub fn extract_subtitle(
    file_path: String,
    track_id: u64,
    output_path: String,
) -> Result<String, String> {
    mkvtoolnix::extract_subtitle_track(&file_path, track_id, &output_path)
        .map_err(|e| e.to_string())?;
    Ok(output_path)
}

#[tauri::command]
pub fn extract_all_subtitles(
    file_path: String,
    output_dir: String,
) -> Result<Vec<String>, String> {
    mkvtoolnix::extract_all_subtitles(&file_path, &output_dir)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn merge_subtitle_into_video(
    video_path: String,
    subtitle_path: String,
    output_path: String,
    language: Option<String>,
) -> Result<(), String> {
    let subs = vec![(subtitle_path, language)];
    mkvtoolnix::merge_subtitles(&video_path, &subs, &output_path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_subtitle_tracks(
    file_path: String,
    output_path: String,
) -> Result<(), String> {
    mkvtoolnix::remove_all_subtitles(&file_path, &output_path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn convert_subtitle_format(
    input: String,
    output: String,
    format: String,
) -> Result<(), String> {
    pysubs2::convert_format(&input, &output, &format).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn shift_subtitle_timing(
    input: String,
    output: String,
    milliseconds: i64,
) -> Result<(), String> {
    pysubs2::shift_subtitles(&input, &output, milliseconds).map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
pub struct SetTrackMetaRequest {
    pub file_path: String,
    pub track_id: u64,
    pub track_name: Option<String>,
    pub default_flag: Option<bool>,
    pub forced_flag: Option<bool>,
}

#[tauri::command]
pub async fn set_track_metadata(req: SetTrackMetaRequest) -> Result<(), String> {
    let mkvpropedit = crate::tools::which("mkvpropedit")
        .ok_or_else(|| "mkvpropedit not found in PATH".to_string())?;

    let mut args: Vec<String> = vec![req.file_path.clone()];
    args.push("--edit".to_string());
    // mkvpropedit uses 1-indexed @N notation for track selection
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
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    Ok(())
}
