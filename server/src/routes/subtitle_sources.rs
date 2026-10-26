use crate::config::AppConfig;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use subforge_subtitle_sources::{
    find_subtitle, OpenSubtitlesCredentials, SearchRequest, SubtitleSearchResult,
};

// Not set at build time = Subsource search is silently disabled (falls through
// to translate) rather than failing the whole build. OpenSubtitles.com is
// unaffected — it always uses the user's own credentials, never this key.
const SUBSOURCE_API_KEY: &str = match option_env!("SUBFORGE_SUBSOURCE_API_KEY") {
    Some(key) => key,
    None => "",
};
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
    Found {
        subtitle_path: String,
        source: String,
        matched_via: String,
    },
    NotFound,
}

pub async fn search_online_subtitle(
    Json(request): Json<OnlineSubtitleSearchRequest>,
) -> Json<OnlineSubtitleSearchResponse> {
    let config = AppConfig::load();
    let opensubtitles_creds = match (
        config.opensubtitles_api_key,
        config.opensubtitles_username,
        config.opensubtitles_password,
    ) {
        (Some(api_key), Some(username), Some(password)) => Some(OpenSubtitlesCredentials {
            api_key,
            username,
            password,
        }),
        _ => None,
    };

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
        SubtitleSearchResult::Found {
            downloaded_path: text,
            source,
            matched_via,
        } => match write_subtitle_next_to_video(&video_path, &request.language, &text) {
            Ok(subtitle_path) => Json(OnlineSubtitleSearchResponse::Found {
                subtitle_path,
                source,
                matched_via,
            }),
            Err(_) => Json(OnlineSubtitleSearchResponse::NotFound),
        },
        SubtitleSearchResult::NotFound => Json(OnlineSubtitleSearchResponse::NotFound),
    }
}

fn write_subtitle_next_to_video(
    video_path: &Path,
    lang: &str,
    text: &str,
) -> std::io::Result<String> {
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("subtitle");
    let dir = video_path.parent().unwrap_or_else(|| Path::new("."));
    let out_path = dir.join(format!("{stem}.{lang}.srt"));
    std::fs::write(&out_path, text)?;
    Ok(out_path.to_string_lossy().to_string())
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetOpenSubtitlesCredentialsRequest {
    pub credentials: OpenSubtitlesCredentials,
}

pub async fn set_opensubtitles_credentials(
    Json(body): Json<SetOpenSubtitlesCredentialsRequest>,
) -> Json<()> {
    let mut config = AppConfig::load();
    config.opensubtitles_api_key = Some(body.credentials.api_key);
    config.opensubtitles_username = Some(body.credentials.username);
    config.opensubtitles_password = Some(body.credentials.password);
    let _ = config.save();
    Json(())
}

pub async fn clear_opensubtitles_credentials() -> Json<()> {
    let mut config = AppConfig::load();
    config.opensubtitles_api_key = None;
    config.opensubtitles_username = None;
    config.opensubtitles_password = None;
    let _ = config.save();
    Json(())
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenSubtitlesStatus {
    pub configured: bool,
}

pub async fn get_opensubtitles_status() -> Json<OpenSubtitlesStatus> {
    let config = AppConfig::load();
    let configured = config.opensubtitles_api_key.is_some()
        && config.opensubtitles_username.is_some()
        && config.opensubtitles_password.is_some();
    Json(OpenSubtitlesStatus { configured })
}

#[derive(Debug, Clone, Serialize)]
pub struct SubsourceStatus {
    pub reachable: bool,
}

pub async fn check_subsource_status() -> Json<SubsourceStatus> {
    if SUBSOURCE_API_KEY.is_empty() {
        return Json(SubsourceStatus { reachable: false });
    }
    let client = reqwest::Client::new();
    let reachable = client
        .get(format!(
            "{}/movies/search",
            subforge_subtitle_sources::subsource::DEFAULT_BASE_URL
        ))
        .header("X-API-Key", SUBSOURCE_API_KEY)
        .query(&[("searchType", "text"), ("q", "ping")])
        .send()
        .await
        .map(|resp| resp.status().is_success() || resp.status().as_u16() == 429)
        .unwrap_or(false);
    Json(SubsourceStatus { reachable })
}
