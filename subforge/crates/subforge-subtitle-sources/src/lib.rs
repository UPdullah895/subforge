pub mod opensubtitles;
pub mod subsource;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("rate limited")]
    RateLimited,
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("not found")]
    NotFound,
    #[error("file too small to hash")]
    FileTooSmall,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(String),
}

/// Already-resolved search metadata for one video — from the rename step's
/// TMDB/TVmaze match when available, or a best-effort filename-parser guess
/// otherwise. `language` is the ISO 639-1 target language code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub title: String,
    pub year: Option<u32>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenSubtitlesCredentials {
    pub api_key: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SubtitleSearchResult {
    Found {
        downloaded_path: String,
        source: String,
        matched_via: String,
    },
    NotFound,
}
