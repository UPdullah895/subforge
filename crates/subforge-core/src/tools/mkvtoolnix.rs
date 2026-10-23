//! MKVToolNix wrappers (track listing, subtitle extract/merge/remove, property edits).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};

use serde::{Deserialize, Serialize};
use subforge_deps::HideWindow;
use subforge_metadata::{filenaming, languages};
use tokio_util::sync::CancellationToken;

use super::{resolve, ToolError, ToolId, ToolPaths};

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
    // Newer mkvmerge (v70+) emits "flag_hearing_impaired"; older builds used
    // "hearing_impaired_flag". Accept both.
    #[serde(
        alias = "flag_hearing_impaired",
        alias = "hearing_impaired_flag",
        default
    )]
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

/// mkvmerge/mkvextract/mkvpropedit exit with 0 on success, 1 when they
/// finished but emitted warnings (output is still written), and 2 on a real
/// error.
fn mkv_succeeded(status: &ExitStatus) -> bool {
    matches!(status.code(), Some(0) | Some(1))
}

/// mkvmerge reports errors on stdout, not stderr, so surface both.
fn mkv_error_text(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    format!("{}\n{}", stdout.trim(), stderr.trim())
        .trim()
        .to_string()
}

/// Maps a finished MKVToolNix process to `Ok(output)` or a `Failed` error.
fn check(tool: ToolId, output: Output) -> Result<Output, ToolError> {
    if mkv_succeeded(&output.status) {
        Ok(output)
    } else {
        Err(ToolError::failed(
            tool,
            &output.status,
            mkv_error_text(&output),
        ))
    }
}

pub fn enumerate_tracks(paths: &ToolPaths, file_path: &str) -> Result<MkvInfo, ToolError> {
    let mkvmerge = resolve(ToolId::Mkvmerge, paths)?;

    let output = std::process::Command::new(&mkvmerge.path)
        .hide_window()
        .args(["-J", file_path])
        .output()?;
    let output = check(ToolId::Mkvmerge, output)?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).map_err(|e| ToolError::Json(e.to_string()))
}

pub fn extract_subtitle_track(
    paths: &ToolPaths,
    file_path: &str,
    track_id: u64,
    output_path: &str,
) -> Result<String, ToolError> {
    let mkvextract = resolve(ToolId::Mkvextract, paths)?;

    let output = std::process::Command::new(&mkvextract.path)
        .hide_window()
        .args([
            "tracks",
            file_path,
            &format!("{}:{}", track_id, output_path),
        ])
        .output()?;
    check(ToolId::Mkvextract, output)?;

    Ok(output_path.to_string())
}

pub fn extract_all_subtitles(
    paths: &ToolPaths,
    file_path: &str,
    output_dir: &str,
) -> Result<Vec<String>, ToolError> {
    extract_subtitles_filtered(paths, file_path, output_dir, None)
}

/// Like `extract_all_subtitles`, but only extracts tracks whose language
/// (case-insensitive) is in `languages` — used by the batch language picker
/// so users can extract e.g. just English instead of every track.
pub fn extract_subtitles_by_language(
    paths: &ToolPaths,
    file_path: &str,
    output_dir: &str,
    languages: &[String],
) -> Result<Vec<String>, ToolError> {
    extract_subtitles_filtered(paths, file_path, output_dir, Some(languages))
}

fn track_lang(track: &Track) -> String {
    track
        .properties
        .as_ref()
        .and_then(|p| p.language.clone())
        .unwrap_or_else(|| "und".to_string())
}

fn extract_subtitles_filtered(
    paths: &ToolPaths,
    file_path: &str,
    output_dir: &str,
    languages: Option<&[String]>,
) -> Result<Vec<String>, ToolError> {
    let info = enumerate_tracks(paths, file_path)?;
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
                    let lang = track_lang(t);
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
        *group_counts
            .entry((track_lang(track), flavor_suffix(track)))
            .or_insert(0) += 1;
    }

    let mut seen: HashMap<(String, Option<&'static str>), u32> = HashMap::new();
    let mut extracted = Vec::new();

    for track in &subtitle_tracks {
        let lang = track_lang(track);
        let flavor = flavor_suffix(track);
        let key = (lang.clone(), flavor);
        let disambiguator = if group_counts.get(&key).copied().unwrap_or(0) > 1 {
            let n = seen.entry(key).or_insert(0);
            *n += 1;
            Some(*n)
        } else {
            None
        };
        let filename = filenaming::external_sub_filename_flavored(
            &video_stem,
            &lang,
            flavor,
            "srt",
            disambiguator,
        );
        let out = PathBuf::from(output_dir)
            .join(filename)
            .to_string_lossy()
            .to_string();
        extract_subtitle_track(paths, file_path, track.id, &out)?;
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

/// Arguments shared by the sync and async merge.
fn merge_args(
    video_path: &str,
    subtitles: &[SubtitleMergeSpec],
    output_path: &str,
    gui_mode: bool,
) -> Vec<String> {
    let mut args = Vec::new();
    if gui_mode {
        args.push("--gui-mode".to_string());
    }
    args.extend([
        "-o".to_string(),
        output_path.to_string(),
        video_path.to_string(),
    ]);

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

        // Real SDH flag (not just "(SDH)" in the name) so players and media
        // servers can offer/auto-select these tracks for hearing-impaired viewers.
        if sub.hearing_impaired {
            args.push("--hearing-impaired-flag".to_string());
            args.push("0:yes".to_string());
        }

        args.push(sub.path.clone());
    }
    args
}

pub fn merge_subtitles(
    paths: &ToolPaths,
    video_path: &str,
    subtitles: &[SubtitleMergeSpec],
    output_path: &str,
) -> Result<(), ToolError> {
    let mkvmerge = resolve(ToolId::Mkvmerge, paths)?;
    let args = merge_args(video_path, subtitles, output_path, false);
    let output = std::process::Command::new(&mkvmerge.path)
        .hide_window()
        .args(&args)
        .output()?;
    check(ToolId::Mkvmerge, output)?;
    Ok(())
}

pub fn remove_tracks(
    paths: &ToolPaths,
    video_path: &str,
    track_ids: &[u64],
    output_path: &str,
) -> Result<(), ToolError> {
    let mkvmerge = resolve(ToolId::Mkvmerge, paths)?;
    let info = enumerate_tracks(paths, video_path)?;
    let remove_set: HashSet<u64> = track_ids.iter().copied().collect();
    let mut args = vec!["-o".to_string(), output_path.to_string()];

    for (kind, keep_flag, none_flag) in [
        ("subtitles", "--subtitle-tracks", "--no-subtitles"),
        ("audio", "--audio-tracks", "--no-audio"),
    ] {
        let has_removal = info
            .tracks
            .iter()
            .any(|t| t.track_type == kind && remove_set.contains(&t.id));
        if !has_removal {
            continue;
        }
        let keep: Vec<String> = info
            .tracks
            .iter()
            .filter(|t| t.track_type == kind && !remove_set.contains(&t.id))
            .map(|t| t.id.to_string())
            .collect();
        if keep.is_empty() {
            args.push(none_flag.to_string());
        } else {
            args.push(keep_flag.to_string());
            args.push(keep.join(","));
        }
    }

    args.push(video_path.to_string());
    let output = std::process::Command::new(&mkvmerge.path)
        .hide_window()
        .args(&args)
        .output()?;
    check(ToolId::Mkvmerge, output)?;
    Ok(())
}

fn mkvmerge_simple(
    paths: &ToolPaths,
    flag: &str,
    video_path: &str,
    output_path: &str,
) -> Result<(), ToolError> {
    let mkvmerge = resolve(ToolId::Mkvmerge, paths)?;
    let output = std::process::Command::new(&mkvmerge.path)
        .hide_window()
        .args(["-o", output_path, flag, video_path])
        .output()?;
    check(ToolId::Mkvmerge, output)?;
    Ok(())
}

pub fn remove_all_subtitles(
    paths: &ToolPaths,
    video_path: &str,
    output_path: &str,
) -> Result<(), ToolError> {
    mkvmerge_simple(paths, "--no-subtitles", video_path, output_path)
}

pub fn remove_all_audio(
    paths: &ToolPaths,
    video_path: &str,
    output_path: &str,
) -> Result<(), ToolError> {
    mkvmerge_simple(paths, "--no-audio", video_path, output_path)
}

/// Sets track properties in place with `mkvpropedit`. `track_number_1based` is
/// mkvpropedit's `track:@N` selector (mkvmerge's 0-based track id plus one);
/// `props` are `(property, value)` pairs such as `("flag-default", "1")`.
pub fn set_track_flags(
    paths: &ToolPaths,
    file: &Path,
    track_number_1based: u32,
    props: &[(String, String)],
) -> Result<(), ToolError> {
    let mkvpropedit = resolve(ToolId::Mkvpropedit, paths)?;

    let mut args: Vec<std::ffi::OsString> = vec![
        file.as_os_str().to_owned(),
        "--edit".into(),
        format!("track:@{track_number_1based}").into(),
    ];
    for (key, value) in props {
        args.push("--set".into());
        args.push(format!("{key}={value}").into());
    }

    let output = std::process::Command::new(&mkvpropedit.path)
        .hide_window()
        .args(&args)
        .output()?;
    check(ToolId::Mkvpropedit, output)?;
    Ok(())
}

/// Parses a GUI progress line from mkvmerge --gui-mode output.
/// Returns Some(percentage) if line matches "#GUI#progress <N>%" format, None otherwise.
fn parse_gui_progress(line: &str) -> Option<f64> {
    let stripped = line.strip_prefix("#GUI#progress ")?;
    let percent_str = stripped.strip_suffix('%')?;
    let pct = percent_str.parse::<i32>().ok()?;
    Some(pct.clamp(0, 100) as f64)
}

/// Async version of merge_subtitles with progress callback and cancellation support.
/// Spawns mkvmerge with --gui-mode, reads stdout line-by-line for progress updates,
/// and checks the cancellation token every 200ms.
pub async fn merge_subtitles_async(
    paths: &ToolPaths,
    video_path: &str,
    subtitles: &[SubtitleMergeSpec],
    output_path: &str,
    cancel: Option<&CancellationToken>,
    mut on_progress: impl FnMut(f64) + Send,
) -> Result<(), ToolError> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};

    let mkvmerge = resolve(ToolId::Mkvmerge, paths)?;
    let args = merge_args(video_path, subtitles, output_path, true);

    let mut child = tokio::process::Command::new(&mkvmerge.path)
        .hide_window()
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ToolError::Io(std::io::Error::other("failed to open mkvmerge stdout")))?;

    // Drain stderr concurrently so a chatty child can't block on a full pipe.
    let stderr_task = child.stderr.take().map(|mut stderr| {
        tokio::spawn(async move {
            let mut buf = String::new();
            let _ = stderr.read_to_string(&mut buf).await;
            buf
        })
    });
    let mut stdout_tail: VecDeque<String> = VecDeque::new();

    let mut reader = tokio::io::BufReader::new(stdout).lines();

    // Read output line-by-line, checking for progress and cancellation
    loop {
        if cancel.is_some_and(|c| c.is_cancelled()) {
            let _ = child.kill().await;
            return Err(ToolError::Cancelled);
        }
        let timeout = tokio::time::sleep(std::time::Duration::from_millis(200));

        tokio::select! {
            line_result = reader.next_line() => {
                match line_result {
                    Ok(Some(line)) => {
                        if let Some(pct) = parse_gui_progress(&line) {
                            on_progress(pct);
                        } else if !line.trim().is_empty() {
                            if stdout_tail.len() >= 20 {
                                stdout_tail.pop_front();
                            }
                            stdout_tail.push_back(line);
                        }
                    }
                    Ok(None) | Err(_) => break,
                }
            }
            _ = timeout => {}
        }
    }

    let status = child.wait().await?;

    if !mkv_succeeded(&status) {
        let stderr = match stderr_task {
            Some(task) => task.await.unwrap_or_default(),
            None => String::new(),
        };
        let stdout_text = stdout_tail.into_iter().collect::<Vec<_>>().join("\n");
        let text = format!("{}\n{}", stdout_text.trim(), stderr.trim());
        return Err(ToolError::failed(ToolId::Mkvmerge, &status, text));
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
    fn reads_hearing_impaired_flag_under_either_mkvmerge_name() {
        for key in ["flag_hearing_impaired", "hearing_impaired_flag"] {
            let raw = format!(
                r#"{{"id":4,"type":"subtitles","codec":"SubRip/SRT","properties":{{"language":"eng","{key}":true}}}}"#
            );
            let track: Track = serde_json::from_str(&raw).unwrap();
            assert_eq!(
                track.properties.unwrap().hearing_impaired,
                Some(true),
                "{key}"
            );
        }
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
        assert_eq!(
            json.get("track_type").and_then(|v| v.as_str()),
            Some("subtitles")
        );
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
    fn handles_bounds() {
        assert_eq!(parse_gui_progress("#GUI#progress 0%"), Some(0.0));
        assert_eq!(parse_gui_progress("#GUI#progress 100%"), Some(100.0));
        assert_eq!(parse_gui_progress("#GUI#progress 140%"), Some(100.0));
    }
}
