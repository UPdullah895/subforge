use super::{which, ToolInfo};
use crate::config::AppConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use subforge_metadata::{filenaming, languages};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MkvError {
    #[error("mkvmerge not found in PATH")]
    MkvmergeNotFound,
    #[error("mkvextract not found in PATH")]
    MkvextractNotFound,
    #[error("Failed to run command: {0}")]
    CommandFailed(String),
    #[error("Failed to parse JSON: {0}")]
    JsonParseError(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("merge cancelled")]
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackProperties {
    #[serde(default)]
    pub language: Option<String>,
    #[serde(rename = "track_name", default)]
    pub track_name: Option<String>,
    #[serde(default)]
    pub default_track: Option<bool>,
    #[serde(default)]
    pub forced_track: Option<bool>,
    // mkvmerge's `-J` output calls this "hearing_impaired_flag"; expose it as
    // the plainer `hearing_impaired` name the frontend/track-naming logic use.
    #[serde(rename(deserialize = "hearing_impaired_flag"), default)]
    pub hearing_impaired: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: u64,
    // mkvmerge's `-J` output names this field "type"; `rename` alone would
    // apply to serialization too, which would send "type" back to the
    // frontend instead of "track_type" that every consumer (batch table,
    // language picker, auto-translate track lookup) expects. Scoping the
    // rename to `deserialize` only keeps both directions correct.
    #[serde(rename(deserialize = "type"))]
    pub track_type: String,
    pub codec: String,
    #[serde(default)]
    pub properties: Option<TrackProperties>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerProperties {
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MkvInfo {
    #[serde(default)]
    pub container: Option<ContainerProperties>,
    #[serde(default)]
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalSub {
    pub lang: String,
    pub status: String,
    #[serde(default)]
    pub error_message: Option<String>,
    pub path: String,
    // "forced"/"sdh" when the filename carries a flavor tag (see
    // `subforge_metadata::filenaming`) — lets the UI tell apart e.g. two
    // external subs in the same language that are actually a regular track
    // and an SDH track.
    #[serde(default)]
    pub flavor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFileInfo {
    pub path: String,
    pub name: String,
    pub tracks: Vec<Track>,
    pub subtitle_count: usize,
    pub audio_count: usize,
    pub video_count: usize,
    #[serde(default)]
    pub external_subs: Vec<ExternalSub>,
}

pub fn discover_mkvmerge() -> ToolInfo {
    let path = which("mkvmerge");
    let available = path.is_some();
    let version = path.as_ref().and_then(|p| {
        Command::new(p)
            .args(["--version"])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8(o.stdout)
                    .ok()
                    .and_then(|s| s.lines().next().map(|l| l.trim().to_string()))
            })
    });

    ToolInfo {
        name: "mkvmerge".to_string(),
        path: path.map(|p| p.to_string_lossy().to_string()),
        version,
        available,
    }
}

pub fn discover_mkvextract() -> ToolInfo {
    let path = which("mkvextract");
    let available = path.is_some();
    let version = path.as_ref().and_then(|p| {
        Command::new(p)
            .args(["--version"])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8(o.stdout)
                    .ok()
                    .and_then(|s| s.lines().next().map(|l| l.trim().to_string()))
            })
    });

    ToolInfo {
        name: "mkvextract".to_string(),
        path: path.map(|p| p.to_string_lossy().to_string()),
        version,
        available,
    }
}

pub fn get_mkvmerge_path() -> Result<PathBuf, MkvError> {
    let config = AppConfig::load();
    if let Some(p) = config.mkvmerge_path {
        let path = PathBuf::from(&p);
        if path.is_file() {
            return Ok(path);
        }
    }
    which("mkvmerge").ok_or(MkvError::MkvmergeNotFound)
}

pub fn get_mkvextract_path() -> Result<PathBuf, MkvError> {
    let config = AppConfig::load();
    if let Some(p) = config.mkvextract_path {
        let path = PathBuf::from(&p);
        if path.is_file() {
            return Ok(path);
        }
    }
    which("mkvextract").ok_or(MkvError::MkvextractNotFound)
}

pub fn enumerate_tracks(file_path: &str) -> Result<MkvInfo, MkvError> {
    let mkvmerge = get_mkvmerge_path()?;

    let output = Command::new(&mkvmerge)
        .args(["-J", file_path])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(MkvError::CommandFailed(stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let info: MkvInfo = serde_json::from_str(&stdout)
        .map_err(|e| MkvError::JsonParseError(e.to_string()))?;

    Ok(info)
}

pub fn extract_subtitle_track(
    file_path: &str,
    track_id: u64,
    output_path: &str,
) -> Result<String, MkvError> {
    let mkvextract = get_mkvextract_path()?;

    let output = Command::new(&mkvextract)
        .args(["tracks", file_path, &format!("{}:{}", track_id, output_path)])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(MkvError::CommandFailed(stderr));
    }

    Ok(output_path.to_string())
}

pub fn extract_all_subtitles(
    file_path: &str,
    output_dir: &str,
) -> Result<Vec<String>, MkvError> {
    extract_subtitles_filtered(file_path, output_dir, None)
}

/// Like `extract_all_subtitles`, but only extracts tracks whose language
/// (case-insensitive) is in `languages` — used by the batch language picker
/// so users can extract e.g. just English instead of every track.
pub fn extract_subtitles_by_language(
    file_path: &str,
    output_dir: &str,
    languages: &[String],
) -> Result<Vec<String>, MkvError> {
    extract_subtitles_filtered(file_path, output_dir, Some(languages))
}

fn extract_subtitles_filtered(
    file_path: &str,
    output_dir: &str,
    languages: Option<&[String]>,
) -> Result<Vec<String>, MkvError> {
    let info = enumerate_tracks(file_path)?;
    let video_stem = Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("subtitle")
        .to_string();

    let subtitle_tracks: Vec<&Track> = info
        .tracks
        .iter()
        .filter(|t| {
            if t.track_type != "subtitles" {
                return false;
            }
            match languages {
                None => true,
                Some(langs) => {
                    let lang = t
                        .properties
                        .as_ref()
                        .and_then(|p| p.language.clone())
                        .unwrap_or_else(|| "und".to_string());
                    langs.iter().any(|l| l.eq_ignore_ascii_case(&lang))
                }
            }
        })
        .collect();

    // Group by (language, flavor) — a flavor tag (forced/sdh) already makes a
    // track distinguishable in the filename, so only tracks that share both
    // language AND flavor (e.g. two plain "ar" tracks) fall back to a
    // trailing running number.
    let mut group_counts: HashMap<(String, Option<&'static str>), usize> = HashMap::new();
    for track in &subtitle_tracks {
        let lang = track
            .properties
            .as_ref()
            .and_then(|p| p.language.clone())
            .unwrap_or_else(|| "und".to_string());
        *group_counts.entry((lang, flavor_suffix(track))).or_insert(0) += 1;
    }

    let mut seen: HashMap<(String, Option<&'static str>), u32> = HashMap::new();
    let mut extracted = Vec::new();

    for track in &subtitle_tracks {
        let lang = track
            .properties
            .as_ref()
            .and_then(|p| p.language.clone())
            .unwrap_or_else(|| "und".to_string());
        let flavor = flavor_suffix(track);
        let key = (lang.clone(), flavor);
        let disambiguator = if group_counts.get(&key).copied().unwrap_or(0) > 1 {
            let n = seen.entry(key).or_insert(0);
            *n += 1;
            Some(*n)
        } else {
            None
        };
        let filename =
            filenaming::external_sub_filename_flavored(&video_stem, &lang, flavor, "srt", disambiguator);
        let out = PathBuf::from(output_dir)
            .join(filename)
            .to_string_lossy()
            .to_string();
        extract_subtitle_track(file_path, track.id, &out)?;
        extracted.push(out);
    }

    Ok(extracted)
}

/// Returns "sdh" or "forced" when a track's properties mark it as such, so
/// extracted filenames and auto-generated track names can carry that flavor
/// instead of a meaningless running number.
fn flavor_suffix(track: &Track) -> Option<&'static str> {
    let props = track.properties.as_ref()?;
    if props.hearing_impaired.unwrap_or(false) {
        Some("sdh")
    } else if props.forced_track.unwrap_or(false) {
        Some("forced")
    } else {
        None
    }
}

/// One subtitle file to mux into a video via `merge_subtitles`. `track_name`
/// defaults to an auto-generated "English - English"-style name (via
/// `subforge_metadata::languages`) when not given explicitly, so merged
/// tracks are self-describing without the user having to type one in.
#[derive(Debug, Clone)]
pub struct SubtitleMergeSpec {
    pub path: String,
    pub language: Option<String>,
    pub track_name: Option<String>,
    pub hearing_impaired: bool,
    pub make_default: bool,
}

impl SubtitleMergeSpec {
    pub fn new(path: String, language: Option<String>) -> Self {
        Self {
            path,
            language,
            track_name: None,
            hearing_impaired: false,
            make_default: false,
        }
    }
}

pub fn merge_subtitles(
    video_path: &str,
    subtitles: &[SubtitleMergeSpec],
    output_path: &str,
) -> Result<(), MkvError> {
    let mkvmerge = get_mkvmerge_path()?;
    let mut args = vec!["-o".to_string(), output_path.to_string(), video_path.to_string()];

    for sub in subtitles {
        let lang = sub.language.as_deref().unwrap_or("und");
        args.push("--language".to_string());
        args.push(format!("0:{lang}"));

        let track_name = sub
            .track_name
            .clone()
            .unwrap_or_else(|| languages::track_display_name(lang, sub.hearing_impaired));
        args.push("--track-name".to_string());
        args.push(format!("0:{track_name}"));

        args.push("--default-track-flag".to_string());
        args.push(format!("0:{}", if sub.make_default { "yes" } else { "no" }));

        args.push(sub.path.clone());
    }

    let output = Command::new(&mkvmerge).args(&args).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(MkvError::CommandFailed(stderr));
    }

    Ok(())
}

pub fn remove_tracks(
    video_path: &str,
    track_ids: &[u64],
    output_path: &str,
) -> Result<(), MkvError> {
    let mkvmerge = get_mkvmerge_path()?;
    let info = enumerate_tracks(video_path)?;
    let remove_set: std::collections::HashSet<u64> = track_ids.iter().copied().collect();
    let mut args = vec!["-o".to_string(), output_path.to_string()];

    let has_sub_removal = info.tracks.iter()
        .any(|t| t.track_type == "subtitles" && remove_set.contains(&t.id));
    if has_sub_removal {
        let keep: Vec<String> = info.tracks.iter()
            .filter(|t| t.track_type == "subtitles" && !remove_set.contains(&t.id))
            .map(|t| t.id.to_string())
            .collect();
        if keep.is_empty() {
            args.push("--no-subtitles".to_string());
        } else {
            args.push("--subtitle-tracks".to_string());
            args.push(keep.join(","));
        }
    }

    let has_audio_removal = info.tracks.iter()
        .any(|t| t.track_type == "audio" && remove_set.contains(&t.id));
    if has_audio_removal {
        let keep: Vec<String> = info.tracks.iter()
            .filter(|t| t.track_type == "audio" && !remove_set.contains(&t.id))
            .map(|t| t.id.to_string())
            .collect();
        if keep.is_empty() {
            args.push("--no-audio".to_string());
        } else {
            args.push("--audio-tracks".to_string());
            args.push(keep.join(","));
        }
    }

    args.push(video_path.to_string());
    let output = Command::new(&mkvmerge).args(&args).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(MkvError::CommandFailed(stderr));
    }

    Ok(())
}

pub fn remove_all_subtitles(video_path: &str, output_path: &str) -> Result<(), MkvError> {
    let mkvmerge = get_mkvmerge_path()?;
    let output = Command::new(&mkvmerge)
        .args(["-o", output_path, "--no-subtitles", video_path])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(MkvError::CommandFailed(stderr));
    }

    Ok(())
}

pub fn remove_all_audio(video_path: &str, output_path: &str) -> Result<(), MkvError> {
    let mkvmerge = get_mkvmerge_path()?;
    let output = Command::new(&mkvmerge)
        .args(["-o", output_path, "--no-audio", video_path])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(MkvError::CommandFailed(stderr));
    }

    Ok(())
}

/// Parses a GUI progress line from mkvmerge --gui-mode output.
/// Returns Some(percentage) if line matches "#GUI#progress <N>%" format, None otherwise.
fn parse_gui_progress(line: &str) -> Option<f64> {
    if let Some(stripped) = line.strip_prefix("#GUI#progress ") {
        if let Some(percent_str) = stripped.strip_suffix("%") {
            if let Ok(pct) = percent_str.parse::<i32>() {
                return Some((pct.max(0).min(100)) as f64);
            }
        }
    }
    None
}

/// Async version of merge_subtitles with progress callback and cancellation support.
/// Spawns mkvmerge with --gui-mode, reads stdout line-by-line for progress updates,
/// and checks the cancellation flag every 200ms.
pub async fn merge_subtitles_async(
    video_path: &str,
    subtitles: &[SubtitleMergeSpec],
    output_path: &str,
    control: Option<&crate::jobs::JobControl>,
    mut on_progress: impl FnMut(f64) + Send,
) -> Result<(), MkvError> {
    use tokio::io::AsyncBufReadExt;

    let mkvmerge = get_mkvmerge_path()?;

    // Build args using the same logic as the sync version
    let mut args = vec!["--gui-mode".to_string(), "-o".to_string(), output_path.to_string(), video_path.to_string()];

    for sub in subtitles {
        let lang = sub.language.as_deref().unwrap_or("und");
        args.push("--language".to_string());
        args.push(format!("0:{lang}"));

        let track_name = sub
            .track_name
            .clone()
            .unwrap_or_else(|| languages::track_display_name(lang, sub.hearing_impaired));
        args.push("--track-name".to_string());
        args.push(format!("0:{track_name}"));

        args.push("--default-track-flag".to_string());
        args.push(format!("0:{}", if sub.make_default { "yes" } else { "no" }));

        args.push(sub.path.clone());
    }

    let mut child = tokio::process::Command::new(&mkvmerge)
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().ok_or_else(|| {
        MkvError::CommandFailed("Failed to open stdout".to_string())
    })?;

    let mut reader = tokio::io::BufReader::new(stdout).lines();

    // Read output line-by-line, checking for progress and cancellation
    loop {
        let read_future = reader.next_line();
        let timeout = tokio::time::sleep(tokio::time::Duration::from_millis(200));

        tokio::select! {
            line_result = read_future => {
                match line_result {
                    Ok(Some(line)) => {
                        if let Some(pct) = parse_gui_progress(&line) {
                            on_progress(pct);
                        }
                    }
                    Ok(None) => {
                        // EOF
                        break;
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
            _ = timeout => {
                // Check cancellation every 200ms
                if let Some(ctl) = control {
                    if ctl.is_cancelled() {
                        let _ = child.kill().await;
                        return Err(MkvError::Cancelled);
                    }
                }
            }
        }
    }

    let status = child.wait().await?;

    if !status.success() {
        let stderr = match child.stderr.take() {
            Some(mut stderr) => {
                let mut buf = String::new();
                use tokio::io::AsyncReadExt;
                let _ = stderr.read_to_string(&mut buf).await;
                buf
            }
            None => "Unknown error".to_string(),
        };
        return Err(MkvError::CommandFailed(stderr));
    }

    Ok(())
}

#[cfg(test)]
mod track_serde_tests {
    use super::*;

    #[test]
    fn deserializes_mkvmerge_type_field_into_track_type() {
        let raw = r#"{"id":3,"type":"subtitles","codec":"SubRip/SRT","properties":{"language":"ita","forced_track":true}}"#;
        let track: Track = serde_json::from_str(raw).unwrap();
        assert_eq!(track.track_type, "subtitles");
        assert_eq!(track.properties.unwrap().language.as_deref(), Some("ita"));
    }

    #[test]
    fn serializes_track_type_as_track_type_not_type() {
        let track = Track {
            id: 3,
            track_type: "subtitles".to_string(),
            codec: "SubRip/SRT".to_string(),
            properties: None,
        };
        let json = serde_json::to_value(&track).unwrap();
        assert_eq!(json.get("track_type").and_then(|v| v.as_str()), Some("subtitles"));
        assert!(json.get("type").is_none());
    }
}

#[cfg(test)]
mod parse_gui_progress_tests {
    use super::*;

    #[test]
    fn parses_valid_progress_line() {
        assert_eq!(parse_gui_progress("#GUI#progress 50%"), Some(50.0));
    }

    #[test]
    fn returns_none_for_non_matching_line() {
        assert_eq!(parse_gui_progress("some random output"), None);
    }

    #[test]
    fn clamps_to_0_percent() {
        assert_eq!(parse_gui_progress("#GUI#progress 0%"), Some(0.0));
    }

    #[test]
    fn clamps_to_100_percent() {
        assert_eq!(parse_gui_progress("#GUI#progress 100%"), Some(100.0));
    }
}
