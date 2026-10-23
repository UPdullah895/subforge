//! External tool (FFmpeg / MKVToolNix) resolution and error type.
//!
//! Resolution order: custom path (an error when invalid, never a silent
//! fallback) -> bundled directory -> system `PATH` / well-known locations.
//! This module never reads application config; callers pass [`ToolPaths`].

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use subforge_deps::{find_executable, tool_version, SearchEnv};

pub mod ffmpeg;
pub mod mkvtoolnix;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolId {
    Ffmpeg,
    Ffprobe,
    Mkvmerge,
    Mkvextract,
    Mkvpropedit,
}

impl ToolId {
    pub const ALL: [ToolId; 5] = [
        ToolId::Ffmpeg,
        ToolId::Ffprobe,
        ToolId::Mkvmerge,
        ToolId::Mkvextract,
        ToolId::Mkvpropedit,
    ];

    /// Plain tool name, e.g. `mkvmerge`.
    pub fn name(self) -> &'static str {
        match self {
            ToolId::Ffmpeg => "ffmpeg",
            ToolId::Ffprobe => "ffprobe",
            ToolId::Mkvmerge => "mkvmerge",
            ToolId::Mkvextract => "mkvextract",
            ToolId::Mkvpropedit => "mkvpropedit",
        }
    }

    /// File name of the executable on this platform (`.exe` appended on Windows).
    pub fn exe_name(self) -> &'static str {
        match self {
            ToolId::Ffmpeg => {
                if cfg!(windows) {
                    "ffmpeg.exe"
                } else {
                    "ffmpeg"
                }
            }
            ToolId::Ffprobe => {
                if cfg!(windows) {
                    "ffprobe.exe"
                } else {
                    "ffprobe"
                }
            }
            ToolId::Mkvmerge => {
                if cfg!(windows) {
                    "mkvmerge.exe"
                } else {
                    "mkvmerge"
                }
            }
            ToolId::Mkvextract => {
                if cfg!(windows) {
                    "mkvextract.exe"
                } else {
                    "mkvextract"
                }
            }
            ToolId::Mkvpropedit => {
                if cfg!(windows) {
                    "mkvpropedit.exe"
                } else {
                    "mkvpropedit"
                }
            }
        }
    }

    fn version_arg(self) -> &'static str {
        match self {
            ToolId::Ffmpeg | ToolId::Ffprobe => "-version",
            _ => "--version",
        }
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolSource {
    Custom,
    Bundled,
    System,
}

/// Where tools may come from. Built by the caller from its own config.
#[derive(Debug, Default, Clone)]
pub struct ToolPaths {
    pub custom: HashMap<ToolId, PathBuf>,
    pub bundled_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ResolvedTool {
    pub id: ToolId,
    pub path: PathBuf,
    pub source: ToolSource,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("{tool} path set in Settings → Tool Paths does not exist: {}. Fix or clear it.", path.display())]
    CustomPathInvalid { tool: ToolId, path: PathBuf },
    #[error("{0} was not found. Install it, set its path in Settings → Tool Paths, or reinstall SubForge.")]
    NotFound(ToolId),
    /// `summary` is a short human-readable reason; `details` the raw tool output.
    #[error("{tool} failed: {summary}")]
    Failed {
        tool: ToolId,
        summary: String,
        details: String,
    },
    #[error("I/O error while running an external tool: {0}")]
    Io(#[from] io::Error),
    #[error("Could not read the tool's JSON output: {0}")]
    Json(String),
    #[error("The operation was cancelled.")]
    Cancelled,
}

impl ToolError {
    /// Builds a [`ToolError::Failed`] from the raw output text (may be empty).
    pub(crate) fn failed(tool: ToolId, status: &std::process::ExitStatus, details: String) -> Self {
        let details = details.trim().to_string();
        let mut lines = details
            .lines()
            .filter(|l| !l.trim().is_empty() && !is_banner_line(l))
            .map(str::trim);
        // ffmpeg prints the actual error last; the MKVToolNix tools print it first.
        let first = if matches!(tool, ToolId::Ffmpeg | ToolId::Ffprobe) {
            lines.next_back()
        } else {
            lines.next()
        };
        let summary = match first {
            Some(l) => truncate(l, 300),
            None => format!("exited with {status}"),
        };
        ToolError::Failed {
            tool,
            summary,
            details,
        }
    }
}

/// ffmpeg/ffprobe start-up banner lines, which are never the reason for a failure.
fn is_banner_line(l: &str) -> bool {
    let t = l.trim_start();
    t.starts_with("ffmpeg version")
        || t.starts_with("ffprobe version")
        || t.starts_with("built with")
        || t.starts_with("configuration:")
        || (l.starts_with(' ') && t.starts_with("lib"))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push('…');
        t
    }
}

/// Resolves `id` using the process `PATH` (plus well-known install dirs).
pub fn resolve(id: ToolId, paths: &ToolPaths) -> Result<ResolvedTool, ToolError> {
    resolve_with_env(id, paths, &SearchEnv::from_process())
}

/// Like [`resolve`] with an explicit search environment (used by tests so they
/// need not mutate the process `PATH`).
#[doc(hidden)]
pub fn resolve_with_env(
    id: ToolId,
    paths: &ToolPaths,
    env: &SearchEnv,
) -> Result<ResolvedTool, ToolError> {
    if let Some(custom) = paths.custom.get(&id) {
        return if custom.is_file() {
            Ok(ResolvedTool {
                id,
                path: custom.clone(),
                source: ToolSource::Custom,
            })
        } else {
            Err(ToolError::CustomPathInvalid {
                tool: id,
                path: custom.clone(),
            })
        };
    }
    if let Some(dir) = &paths.bundled_dir {
        let p = dir.join(id.exe_name());
        if p.is_file() {
            return Ok(ResolvedTool {
                id,
                path: p,
                source: ToolSource::Bundled,
            });
        }
    }
    if let Some(p) = find_executable(id.name(), env) {
        return Ok(ResolvedTool {
            id,
            path: p,
            source: ToolSource::System,
        });
    }
    Err(ToolError::NotFound(id))
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolStatus {
    pub id: ToolId,
    pub source: Option<ToolSource>,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
    pub error: Option<String>,
}

impl ToolId {
    fn from_dep(id: subforge_deps::DepId) -> Self {
        use subforge_deps::DepId;
        match id {
            DepId::Ffmpeg => ToolId::Ffmpeg,
            DepId::Ffprobe => ToolId::Ffprobe,
            DepId::Mkvmerge => ToolId::Mkvmerge,
            DepId::Mkvextract => ToolId::Mkvextract,
            DepId::Mkvpropedit => ToolId::Mkvpropedit,
        }
    }
}

/// The startup dependency report, using the same resolution as every tool
/// call (custom -> bundled -> system). A tool is OK when it resolves from any
/// source; a broken custom path is reported as [`subforge_deps::DepState::Invalid`].
pub fn dependency_report(paths: &ToolPaths) -> subforge_deps::DepReport {
    dependency_report_with(paths, &subforge_deps::Context::from_process())
}

/// Like [`dependency_report`] with an explicit host context (used by tests).
pub fn dependency_report_with(
    paths: &ToolPaths,
    ctx: &subforge_deps::Context,
) -> subforge_deps::DepReport {
    use subforge_deps::{DepSource, Resolution};
    subforge_deps::check_all_with(ctx, |dep| {
        match resolve_with_env(ToolId::from_dep(dep), paths, &ctx.env) {
            Ok(r) => Resolution::Found {
                path: r.path,
                source: match r.source {
                    ToolSource::Custom => DepSource::Custom,
                    ToolSource::Bundled => DepSource::Bundled,
                    ToolSource::System => DepSource::System,
                },
            },
            Err(ToolError::CustomPathInvalid { tool, path }) => Resolution::Invalid {
                message: ToolError::CustomPathInvalid {
                    tool,
                    path: path.clone(),
                }
                .to_string(),
                path,
            },
            Err(_) => Resolution::Missing,
        }
    })
}

/// Resolution result and version for every tool, for the Settings screen.
pub fn tool_status(paths: &ToolPaths) -> Vec<ToolStatus> {
    ToolId::ALL
        .iter()
        .map(|&id| match resolve(id, paths) {
            Ok(r) => ToolStatus {
                id,
                source: Some(r.source),
                version: tool_version(&r.path, id.version_arg()),
                path: Some(r.path),
                error: None,
            },
            Err(e) => ToolStatus {
                id,
                source: None,
                path: None,
                version: None,
                error: Some(e.to_string()),
            },
        })
        .collect()
}
