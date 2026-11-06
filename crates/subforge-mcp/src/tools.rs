//! The tool table: what an agent can ask SubForge to do, and how each call maps onto the
//! shared core (never onto a shell string).
//!
//! Everything here is synchronous and cheap: inspect, extract, mux, convert, retime. The
//! heavy operations (transcription, translation, vision) belong on the server's scheduler
//! and will arrive as job-returning tools once they can be queued.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{json, Value};
use subforge_core::subtitle::Format;
use subforge_core::tools::mkvtoolnix::{self, SubtitleMergeSpec};
use subforge_core::tools::{ffmpeg, ToolPaths};
use subforge_pipeline::{plan_file, Policy, RuleProfile};

/// Tools shipped next to the binary (`<exe>/bin`) or pointed at with `SUBFORGE_BUNDLED_DIR`.
fn tool_paths() -> ToolPaths {
    let bundled = std::env::var("SUBFORGE_BUNDLED_DIR")
        .ok()
        .map(|dir| dir.trim().to_string())
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let bin = std::env::current_exe().ok()?.parent()?.join("bin");
            bin.is_dir().then_some(bin)
        });
    ToolPaths {
        custom: HashMap::new(),
        bundled_dir: bundled,
    }
}

fn require_str(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("`{key}` is required and must be a string"))
}

fn optional_str_list(args: &Value, key: &str) -> Result<Vec<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("`{key}` must be an array of strings"))
            })
            .collect(),
        Some(_) => Err(format!("`{key}` must be an array of strings")),
    }
}

/// The `tools/list` payload.
pub fn specs() -> Value {
    json!({ "tools": [
        {
            "name": "list_tracks",
            "description": "List every track in a media file with its type, codec, language, name and flags.",
            "inputSchema": {
                "type": "object",
                "properties": { "file": { "type": "string", "description": "Path to the media file" } },
                "required": ["file"]
            }
        },
        {
            "name": "find_missing_languages",
            "description": "Report which of the wanted subtitle languages a file does not have yet.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string" },
                    "languages": { "type": "array", "items": { "type": "string" }, "description": "ISO codes, e.g. [\"ar\",\"en\"]" }
                },
                "required": ["file", "languages"]
            }
        },
        {
            "name": "extract_subtitles",
            "description": "Extract embedded subtitles to files. Without filters it extracts every subtitle track.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string" },
                    "output_dir": { "type": "string" },
                    "languages": { "type": "array", "items": { "type": "string" } },
                    "track_ids": { "type": "array", "items": { "type": "integer" } }
                },
                "required": ["file", "output_dir"]
            }
        },
        {
            "name": "merge_subtitles",
            "description": "Mux external subtitle files into a video, setting language and flags on each new track.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "video": { "type": "string" },
                    "output": { "type": "string" },
                    "subtitles": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" },
                                "language": { "type": "string" },
                                "track_name": { "type": "string" },
                                "hearing_impaired": { "type": "boolean" },
                                "make_default": { "type": "boolean" }
                            },
                            "required": ["path"]
                        }
                    }
                },
                "required": ["video", "output", "subtitles"]
            }
        },
        {
            "name": "convert_subtitle",
            "description": "Convert a subtitle file between SRT, ASS, SSA and VTT.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string" },
                    "output": { "type": "string" },
                    "format": { "type": "string", "enum": ["srt", "ass", "ssa", "vtt"] }
                },
                "required": ["input", "output", "format"]
            }
        },
        {
            "name": "shift_subtitle",
            "description": "Move every cue in a subtitle file by a number of milliseconds (negative is earlier).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string" },
                    "output": { "type": "string" },
                    "offset_ms": { "type": "integer" }
                },
                "required": ["input", "output", "offset_ms"]
            }
        },
        {
            "name": "plan_policy",
            "description": "Say what an unattended run would do to this file for the given languages — the ordered steps (search, transcribe, translate, lint, mux, verify, cleanup) — without touching it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string" },
                    "languages": {
                        "description": "Target languages, ISO 639-1, e.g. [\"ar\"]",
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "rule_profile": {
                        "description": "Readability rules: off, netflix_adult, netflix_kids, bbc",
                        "type": "string"
                    },
                    "delete_source_after_mux": { "type": "boolean" }
                },
                "required": ["file", "languages"]
            }
        },
        {
            "name": "library_audit",
            "description": "Walk a folder and report which files are missing which languages, so an agent can decide what to hand to plan_policy.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dir": { "type": "string" },
                    "languages": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "recursive": { "type": "boolean" }
                },
                "required": ["dir", "languages"]
            }
        },
        {
            "name": "lint_subtitle",
            "description": "Check a subtitle against a readability rule-set (42 chars/line, 2 lines, 20 cps, 5/6–7 s for Netflix) and optionally rewrite it to fit. Reports what is still wrong afterwards.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string" },
                    "output": {
                        "description": "Where the fixed file goes. Without it the input is left untouched.",
                        "type": "string"
                    },
                    "profile": {
                        "description": "off, netflix_adult, netflix_kids, bbc",
                        "type": "string"
                    },
                    "fix": {
                        "description": "Rewrite the subtitle instead of only reporting",
                        "type": "boolean"
                    }
                },
                "required": ["input"]
            }
        },
        {
            "name": "probe_media",
            "description": "Read a file's container duration and frame rate with ffprobe.",
            "inputSchema": {
                "type": "object",
                "properties": { "file": { "type": "string" } },
                "required": ["file"]
            }
        }
    ] })
}

fn rule_profile(args: &Value, default: RuleProfile) -> Result<RuleProfile, String> {
    match args.get("rule_profile").or_else(|| args.get("profile")) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::String(name)) => match name.as_str() {
            "off" => Ok(RuleProfile::Off),
            "netflix_adult" => Ok(RuleProfile::NetflixAdult),
            "netflix_kids" => Ok(RuleProfile::NetflixKids),
            "bbc" => Ok(RuleProfile::Bbc),
            other => Err(format!(
                "unknown rule profile `{other}` (off, netflix_adult, netflix_kids, bbc)"
            )),
        },
        Some(_) => Err("`profile` must be a string".to_string()),
    }
}

fn flag(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn is_video(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("mkv" | "mp4" | "m4v" | "avi" | "mov" | "ts" | "webm")
    )
}

fn collect_videos(dir: &std::path::Path, recursive: bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                collect_videos(&path, recursive, out);
            }
        } else if is_video(&path) {
            out.push(path);
        }
    }
}

/// Runs one tool. `Ok` is the data to hand back (serialised as JSON text), `Err` a message
/// for the agent — a tool failure is not a protocol error.
pub fn call(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "plan_policy" => {
            let file = require_str(args, "file")?;
            let languages = optional_str_list(args, "languages")?;
            if languages.is_empty() {
                return Err("`languages` must name at least one target language".to_string());
            }
            let policy = Policy {
                target_languages: languages,
                rule_profile: rule_profile(args, RuleProfile::Off)?,
                delete_source_after_mux: flag(args, "delete_source_after_mux", false),
                ..Policy::default()
            };
            let plan = plan_file(&tool_paths(), &file, &policy).map_err(err)?;
            serde_json::to_value(&plan).map_err(|error| error.to_string())
        }

        "library_audit" => {
            let dir = require_str(args, "dir")?;
            let languages = optional_str_list(args, "languages")?;
            if languages.is_empty() {
                return Err("`languages` must name at least one target language".to_string());
            }
            let recursive = flag(args, "recursive", true);
            let root = PathBuf::from(&dir);
            if !root.is_dir() {
                return Err(format!("`{dir}` is not a directory"));
            }
            let mut videos = Vec::new();
            collect_videos(&root, recursive, &mut videos);
            let policy = Policy {
                target_languages: languages,
                ..Policy::default()
            };
            let paths = tool_paths();
            let mut complete = 0usize;
            let mut unreadable = Vec::new();
            let mut missing = Vec::new();
            for video in &videos {
                let file = video.to_string_lossy().to_string();
                match plan_file(&paths, &file, &policy) {
                    Ok(plan) if plan.is_noop() => complete += 1,
                    Ok(plan) => missing.push(json!({
                        "file": file,
                        "present": plan.present,
                        "missing": plan.missing,
                    })),
                    Err(error) => unreadable.push(json!({ "file": file, "error": error })),
                }
            }
            Ok(json!({
                "dir": dir,
                "checked": videos.len(),
                "complete": complete,
                "missing": missing,
                "unreadable": unreadable,
            }))
        }

        "lint_subtitle" => {
            let input = require_str(args, "input")?;
            let profile = rule_profile(args, RuleProfile::NetflixAdult)?;
            let fix = flag(args, "fix", false);
            let output = args
                .get("output")
                .and_then(Value::as_str)
                .map(str::to_string);
            if fix {
                // Never rewrite the input unless it was asked for explicitly: a tool call is
                // cheap, a lost subtitle is not.
                let target = output.clone().unwrap_or_else(|| input.clone());
                if target != input {
                    std::fs::copy(&input, &target)
                        .map_err(|error| format!("cannot copy to {target}: {error}"))?;
                }
                let report =
                    subforge_pipeline::rules::lint_file(std::path::Path::new(&target), profile)
                        .map_err(err)?;
                return Ok(json!({ "output": target, "report": report }));
            }
            let report = subforge_pipeline::rules::inspect(std::path::Path::new(&input), profile)
                .map_err(err)?;
            Ok(json!({ "input": input, "report": report }))
        }

        "list_tracks" => {
            let file = require_str(args, "file")?;
            let info = mkvtoolnix::enumerate_tracks(&tool_paths(), &file).map_err(err)?;
            let tracks: Vec<Value> = info
                .tracks
                .iter()
                .map(|track| {
                    let props = track.properties.as_ref();
                    json!({
                        "id": track.id,
                        "type": track.track_type,
                        "codec": track.codec,
                        "language": props.and_then(|p| p.language.clone()),
                        "name": props.and_then(|p| p.track_name.clone()),
                        "codec_id": props.and_then(|p| p.codec_id.clone()),
                        "default": props.and_then(|p| p.default_track),
                        "forced": props.and_then(|p| p.forced_track),
                        "hearing_impaired": props.and_then(|p| p.hearing_impaired),
                    })
                })
                .collect();
            Ok(json!({ "file": file, "count": tracks.len(), "tracks": tracks }))
        }

        "find_missing_languages" => {
            let file = require_str(args, "file")?;
            let wanted = optional_str_list(args, "languages")?;
            if wanted.is_empty() {
                return Err("`languages` must list at least one ISO code".to_string());
            }
            let info = mkvtoolnix::enumerate_tracks(&tool_paths(), &file).map_err(err)?;
            let present: Vec<String> = info
                .tracks
                .iter()
                .filter(|t| t.track_type == "subtitles")
                .filter_map(|t| t.properties.as_ref()?.language.clone())
                .map(|l| l.to_ascii_lowercase())
                .collect();
            let missing: Vec<String> = wanted
                .iter()
                .filter(|want| {
                    let want = want.to_ascii_lowercase();
                    !present
                        .iter()
                        .any(|have| *have == want || have.starts_with(&want))
                })
                .cloned()
                .collect();
            Ok(json!({
                "file": file,
                "present": present,
                "missing": missing,
                "complete": missing.is_empty(),
            }))
        }

        "extract_subtitles" => {
            let file = require_str(args, "file")?;
            let output_dir = require_str(args, "output_dir")?;
            let languages = optional_str_list(args, "languages")?;
            let track_ids: Vec<u64> = match args.get("track_ids") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|item| {
                        item.as_u64()
                            .ok_or_else(|| "`track_ids` must be an array of integers".to_string())
                    })
                    .collect::<Result<_, _>>()?,
                Some(_) => return Err("`track_ids` must be an array of integers".to_string()),
            };

            let paths = tool_paths();
            let written: Vec<String> = if !track_ids.is_empty() {
                let mut out = Vec::new();
                for id in &track_ids {
                    let target = std::path::Path::new(&output_dir)
                        .join(format!("track{id}.srt"))
                        .to_string_lossy()
                        .to_string();
                    out.push(
                        mkvtoolnix::extract_subtitle_track(&paths, &file, *id, &target)
                            .map_err(err)?,
                    );
                }
                out
            } else if !languages.is_empty() {
                mkvtoolnix::extract_subtitles_by_language(&paths, &file, &output_dir, &languages)
                    .map_err(err)?
            } else {
                mkvtoolnix::extract_all_subtitles(&paths, &file, &output_dir).map_err(err)?
            };
            Ok(json!({ "file": file, "count": written.len(), "files": written }))
        }

        "merge_subtitles" => {
            let video = require_str(args, "video")?;
            let output = require_str(args, "output")?;
            let items = args
                .get("subtitles")
                .and_then(Value::as_array)
                .ok_or_else(|| "`subtitles` must be a non-empty array".to_string())?;
            if items.is_empty() {
                return Err("`subtitles` must be a non-empty array".to_string());
            }
            let specs: Vec<SubtitleMergeSpec> = items
                .iter()
                .map(|item| {
                    Ok(SubtitleMergeSpec {
                        path: require_str(item, "path")?,
                        language: item
                            .get("language")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        track_name: item
                            .get("track_name")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        hearing_impaired: item
                            .get("hearing_impaired")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        make_default: item
                            .get("make_default")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    })
                })
                .collect::<Result<_, String>>()?;
            mkvtoolnix::merge_subtitles(&tool_paths(), &video, &specs, &output).map_err(err)?;
            Ok(json!({ "output": output, "merged": specs.len() }))
        }

        "convert_subtitle" => {
            let input = require_str(args, "input")?;
            let output = require_str(args, "output")?;
            let format = require_str(args, "format")?;
            let format = Format::from_extension(&format)
                .ok_or_else(|| "`format` must be one of srt, ass, ssa, vtt".to_string())?;
            subforge_core::subtitle::convert_file(
                std::path::Path::new(&input),
                std::path::Path::new(&output),
                format,
            )
            .map_err(|e| e.to_string())?;
            Ok(json!({ "output": output }))
        }

        "shift_subtitle" => {
            let input = require_str(args, "input")?;
            let output = require_str(args, "output")?;
            let offset = args
                .get("offset_ms")
                .and_then(Value::as_i64)
                .ok_or_else(|| "`offset_ms` must be an integer".to_string())?;
            subforge_core::subtitle::shift_file(
                std::path::Path::new(&input),
                std::path::Path::new(&output),
                offset,
            )
            .map_err(|e| e.to_string())?;
            Ok(json!({ "output": output, "offset_ms": offset }))
        }

        "probe_media" => {
            let file = require_str(args, "file")?;
            let info = ffmpeg::probe_media(&tool_paths(), &file).map_err(err)?;
            Ok(json!({ "file": file, "duration_ms": info.duration_ms, "fps": info.fps }))
        }

        other => Err(format!("unknown tool: {other}")),
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_name_description_and_object_schema() {
        let specs = specs();
        let tools = specs["tools"].as_array().unwrap();
        assert!(tools.len() >= 7);
        for tool in tools {
            assert!(tool["name"].as_str().is_some_and(|n| !n.is_empty()));
            assert!(tool["description"].as_str().is_some_and(|d| d.len() > 10));
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert!(tool["inputSchema"]["required"].is_array());
        }
    }

    #[test]
    fn an_unknown_tool_is_reported_not_crashed() {
        let error = call("not_a_tool", &json!({})).unwrap_err();
        assert!(error.contains("unknown tool"));
    }

    #[test]
    fn plan_policy_needs_a_language_and_a_file() {
        let error = call("plan_policy", &json!({"file": "/x.mkv", "languages": []})).unwrap_err();
        assert!(error.contains("at least one"), "{error}");
        let error = call("plan_policy", &json!({"languages": ["ar"]})).unwrap_err();
        assert!(error.contains("`file`"), "{error}");
        // A file that is not there is a tool error naming the file, not a panic.
        let error = call(
            "plan_policy",
            &json!({"file": "/definitely/missing.mkv", "languages": ["ar"]}),
        )
        .unwrap_err();
        assert!(!error.is_empty());
    }

    #[test]
    fn lint_subtitle_reports_and_only_rewrites_when_asked() {
        let dir = std::env::temp_dir().join(format!("sf_mcp_lint_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("messy.srt");
        let original = format!(
            "1\n00:00:01,000 --> 00:00:02,000\n{}\n\n",
            "A line far too long for any subtitle profile, which must be re-wrapped before anyone reads it."
        );
        std::fs::write(&input, &original).unwrap();
        let input_str = input.to_string_lossy().to_string();

        // Report only: nothing on disk changes.
        let report = call(
            "lint_subtitle",
            &json!({"input": input_str, "profile": "netflix_adult"}),
        )
        .unwrap();
        assert!(!report["report"]["found"].as_array().unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(&input).unwrap(), original);

        // Fix into another file: the input is still byte-identical.
        let output = dir.join("fixed.srt");
        let fixed = call(
            "lint_subtitle",
            &json!({
                "input": input_str,
                "output": output.to_string_lossy(),
                "profile": "netflix_adult",
                "fix": true
            }),
        )
        .unwrap();
        assert_eq!(fixed["report"]["written"], true);
        assert_ne!(std::fs::read_to_string(&output).unwrap(), original);
        assert_eq!(std::fs::read_to_string(&input).unwrap(), original);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn library_audit_counts_only_video_files() {
        let dir = std::env::temp_dir().join(format!("sf_mcp_audit_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("notes.txt"), "not a video").unwrap();
        std::fs::write(dir.join("sub/poster.jpg"), "not a video").unwrap();

        let audit = call(
            "library_audit",
            &json!({"dir": dir.to_string_lossy(), "languages": ["ar"]}),
        )
        .unwrap();
        assert_eq!(audit["checked"], 0);
        assert_eq!(audit["complete"], 0);

        let error = call(
            "library_audit",
            &json!({"dir": "/nope", "languages": ["ar"]}),
        )
        .unwrap_err();
        assert!(error.contains("not a directory"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_rule_profile_lists_the_known_ones() {
        let error = call(
            "lint_subtitle",
            &json!({"input": "/x.srt", "profile": "netflix_ultra"}),
        )
        .unwrap_err();
        assert!(error.contains("netflix_adult"), "{error}");
    }

    #[test]
    fn missing_arguments_name_the_field() {
        let error = call("list_tracks", &json!({})).unwrap_err();
        assert!(error.contains("file"));
    }

    #[test]
    fn merge_rejects_an_empty_subtitle_list() {
        let error = call(
            "merge_subtitles",
            &json!({ "video": "/x.mkv", "output": "/y.mkv", "subtitles": [] }),
        )
        .unwrap_err();
        assert!(error.contains("non-empty"));
    }

    #[test]
    fn an_unsupported_format_lists_the_allowed_ones() {
        let error = call(
            "convert_subtitle",
            &json!({ "input": "/a.srt", "output": "/b.txt", "format": "txt" }),
        )
        .unwrap_err();
        assert!(error.contains("srt, ass, ssa, vtt"));
    }
}
