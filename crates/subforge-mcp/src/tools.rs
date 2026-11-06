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

/// Runs one tool. `Ok` is the data to hand back (serialised as JSON text), `Err` a message
/// for the agent — a tool failure is not a protocol error.
pub fn call(name: &str, args: &Value) -> Result<Value, String> {
    match name {
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
