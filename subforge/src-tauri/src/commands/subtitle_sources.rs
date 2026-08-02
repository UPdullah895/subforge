use crate::credentials::CredentialStore;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use subforge_subtitle_sources::{find_subtitle, OpenSubtitlesCredentials, SearchRequest, SubtitleSearchResult};
use tauri::State;

const SUBSOURCE_API_KEY: &str = env!(
    "SUBFORGE_SUBSOURCE_API_KEY",
    "Set SUBFORGE_SUBSOURCE_API_KEY at build time (see README) to the app-level Subsource API key."
);
const USER_AGENT: &str = concat!("SubForge v", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineSubtitleSearchRequest {
    pub video_path: String,
    pub title: String,
    pub year: Option<u32>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OnlineSubtitleSearchResponse {
    Found { subtitle_path: String, source: String, matched_via: String },
    NotFound,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn search_online_subtitle(
    credential_store: State<'_, Arc<dyn CredentialStore>>,
    request: OnlineSubtitleSearchRequest,
) -> Result<OnlineSubtitleSearchResponse, String> {
    // Credential-store read errors are treated as "not configured" to degrade gracefully (never fail loudly).
    let opensubtitles_creds: Option<OpenSubtitlesCredentials> =
        credential_store.get().unwrap_or(None);

    let search_request = SearchRequest {
        title: request.title,
        year: request.year,
        season: request.season,
        episode: request.episode,
        language: request.language.clone(),
    };

    let video_path = PathBuf::from(&request.video_path);
    let result = find_subtitle(
        &search_request,
        &video_path,
        opensubtitles_creds.as_ref(),
        SUBSOURCE_API_KEY,
        USER_AGENT,
    )
    .await;

    match result {
        SubtitleSearchResult::Found { downloaded_path: text, source, matched_via } => {
            let subtitle_path = write_subtitle_next_to_video(&video_path, &request.language, &text)
                .map_err(|e| e.to_string())?;
            Ok(OnlineSubtitleSearchResponse::Found {
                subtitle_path,
                source,
                matched_via,
            })
        }
        SubtitleSearchResult::NotFound => Ok(OnlineSubtitleSearchResponse::NotFound),
    }
}

/// Writes downloaded subtitle text next to the video using the same
/// `{video_stem}.{lang}.{ext}` convention `extract_all_subtitles` already
/// writes and `detect_external_subs` (`commands/batch.rs`) already reads
/// back, so a subtitle found online is indistinguishable from one extracted
/// locally on a later scan.
fn write_subtitle_next_to_video(video_path: &Path, lang: &str, text: &str) -> std::io::Result<String> {
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("subtitle");
    let dir = video_path.parent().unwrap_or_else(|| Path::new("."));
    let out_path = dir.join(format!("{stem}.{lang}.srt"));
    std::fs::write(&out_path, text)?;
    Ok(out_path.to_string_lossy().to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn set_opensubtitles_credentials(
    credential_store: State<'_, Arc<dyn CredentialStore>>,
    credentials: OpenSubtitlesCredentials,
) -> Result<(), String> {
    credential_store.set(&credentials)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn clear_opensubtitles_credentials(
    credential_store: State<'_, Arc<dyn CredentialStore>>,
) -> Result<(), String> {
    credential_store.clear()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenSubtitlesStatus {
    pub configured: bool,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_opensubtitles_status(
    credential_store: State<'_, Arc<dyn CredentialStore>>,
) -> Result<OpenSubtitlesStatus, String> {
    // Credential-store read errors are treated as "not configured" to degrade gracefully (never fail loudly).
    let configured = credential_store.get().unwrap_or(None).is_some();
    Ok(OpenSubtitlesStatus { configured })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsourceStatus {
    pub reachable: bool,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn check_subsource_status() -> Result<SubsourceStatus, String> {
    let client = reqwest::Client::new();
    let reachable = client
        .get(format!("{}/movies/search", subforge_subtitle_sources::subsource::DEFAULT_BASE_URL))
        .header("X-API-Key", SUBSOURCE_API_KEY)
        .query(&[("searchType", "text"), ("q", "ping")])
        .send()
        .await
        .map(|resp| resp.status().is_success() || resp.status().as_u16() == 429)
        .unwrap_or(false);
    Ok(SubsourceStatus { reachable })
}
