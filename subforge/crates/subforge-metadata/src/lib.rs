pub mod cache;
pub mod filenaming;
pub mod matcher;
pub mod parser;
pub mod renamer;
pub mod tmdb;
pub mod tvmaze;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API error: {0}")]
    Api(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("No API key configured for {0}")]
    MissingKey(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Tmdb,
    Tvmaze,
}

impl Provider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::Tmdb => "tmdb",
            Provider::Tvmaze => "tvmaze",
        }
    }

    pub fn parse(s: &str) -> Result<Self, MetadataError> {
        match s {
            "tmdb" => Ok(Provider::Tmdb),
            "tvmaze" => Ok(Provider::Tvmaze),
            other => Err(MetadataError::Api(format!("Unknown provider: {other}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesMatch {
    pub id: String,
    pub name: String,
    pub year: Option<u32>,
    pub poster_url: Option<String>,
    pub provider: Provider,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub season: u32,
    pub episode: u32,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedFilename {
    pub series_guess: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenamePreviewRow {
    pub old_path: String,
    pub new_name: Option<String>,
    pub matched: bool,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

pub(crate) fn urlencoding_query(q: &str) -> String {
    let mut out = String::with_capacity(q.len());
    for byte in q.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
