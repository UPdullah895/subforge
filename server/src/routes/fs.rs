//! Read-only directory listing used by the web UI's file picker (the browser
//! has no native dialog, and the files live on the server's machine).

use crate::error::blocking;
use axum::{extract::Query, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_ENTRIES: usize = 5000;

#[derive(Deserialize)]
pub struct ListQuery {
    /// Directory to list; defaults to the user's home directory.
    path: Option<String>,
    #[serde(default)]
    show_hidden: bool,
}

#[derive(Serialize)]
pub struct FsEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
}

#[derive(Serialize)]
pub struct FsListing {
    path: String,
    parent: Option<String>,
    entries: Vec<FsEntry>,
    /// Quick-jump locations (home, filesystem roots / drives).
    roots: Vec<FsRoot>,
    truncated: bool,
}

#[derive(Serialize)]
pub struct FsRoot {
    name: String,
    path: String,
}

fn roots() -> Vec<FsRoot> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(FsRoot { name: "Home".into(), path: home.to_string_lossy().into() });
    }
    #[cfg(windows)]
    for letter in b'A'..=b'Z' {
        let drive = format!("{}:\\", letter as char);
        if std::path::Path::new(&drive).exists() {
            roots.push(FsRoot { name: drive.clone(), path: drive });
        }
    }
    #[cfg(not(windows))]
    roots.push(FsRoot { name: "/".into(), path: "/".into() });
    roots
}

fn list(query: ListQuery) -> Result<FsListing, (StatusCode, String)> {
    let requested = query
        .path
        .filter(|p| !p.trim().is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("/"));

    // Fall back to the nearest existing ancestor so a stale or mistyped path
    // still opens somewhere useful instead of an error.
    let mut dir = requested.as_path();
    while !dir.is_dir() {
        match dir.parent() {
            Some(p) => dir = p,
            None => break,
        }
    }
    let dir = dir
        .canonicalize()
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let read = std::fs::read_dir(&dir).map_err(|e| {
        (StatusCode::FORBIDDEN, format!("Cannot open {}: {e}", dir.display()))
    })?;

    let mut entries: Vec<FsEntry> = Vec::new();
    let mut truncated = false;
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !query.show_hidden && name.starts_with('.') {
            continue;
        }
        // `metadata` follows symlinks; broken links are skipped.
        let Ok(meta) = std::fs::metadata(entry.path()) else { continue };
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        entries.push(FsEntry {
            name,
            path: entry.path().to_string_lossy().to_string(),
            is_dir: meta.is_dir(),
            size: if meta.is_dir() { 0 } else { meta.len() },
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(FsListing {
        path: dir.to_string_lossy().to_string(),
        parent: dir.parent().map(|p| p.to_string_lossy().to_string()),
        entries,
        roots: roots(),
        truncated,
    })
}

pub async fn list_dir(
    Query(query): Query<ListQuery>,
) -> Result<Json<FsListing>, (StatusCode, String)> {
    blocking(move || list(query))
        .await
        .map_err(|c| (c, "listing task failed".to_string()))?
        .map(Json)
}
