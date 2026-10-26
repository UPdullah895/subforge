use crate::config::AppConfig;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use subforge_subtitle_sources::{
    find_subtitle, OpenSubtitlesCredentials, SearchRequest, SubtitleSearchResult,
};

// Fallback key compiled in at build time (release builds). A key set through the
// server API takes precedence. Without either, Subsource search is skipped
// rather than failing the build. OpenSubtitles.com always uses the user's own
// credentials, never this key.
const SUBSOURCE_API_KEY: &str = match option_env!("SUBFORGE_SUBSOURCE_API_KEY") {
    Some(key) => key,
    None => "",
};
const USER_AGENT: &str = concat!("SubForge v", env!("CARGO_PKG_VERSION"));

/// The configured Subsource key when set, otherwise the build-time fallback.
fn resolve_subsource_key(config: &AppConfig) -> String {
    config
        .subsource_api_key
        .as_deref()
        .filter(|k| !k.trim().is_empty())
        .map(str::to_string)
        .or_else(|| (!SUBSOURCE_API_KEY.is_empty()).then(|| SUBSOURCE_API_KEY.to_string()))
        .unwrap_or_default()
}

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
    let subsource_key = resolve_subsource_key(&config);
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
        &subsource_key,
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
) -> Result<Json<()>, (axum::http::StatusCode, String)> {
    let mut config = AppConfig::load();
    let api_key = body.credentials.api_key.trim().to_string();
    let username = body.credentials.username.trim().to_string();
    if api_key.is_empty() || username.is_empty() {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "Enter both the OpenSubtitles API key and username.".to_string(),
        ));
    }
    // Blank password means "keep the stored one" (same as the desktop app).
    let password = if body.credentials.password.is_empty() {
        config.opensubtitles_password.clone().unwrap_or_default()
    } else {
        body.credentials.password
    };
    if password.is_empty() {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "Enter the OpenSubtitles password.".to_string(),
        ));
    }
    config.opensubtitles_api_key = Some(api_key);
    config.opensubtitles_username = Some(username);
    config.opensubtitles_password = Some(password);
    config
        .save()
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(()))
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
    /// Non-secret: lets the form show the account it is using.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_set: Option<bool>,
}

pub async fn get_opensubtitles_status() -> Json<OpenSubtitlesStatus> {
    let config = AppConfig::load();
    let configured = config.opensubtitles_api_key.is_some()
        && config.opensubtitles_username.is_some()
        && config.opensubtitles_password.is_some();
    // Never return key material over HTTP; the desktop app reads the keyring
    // directly instead.
    Json(OpenSubtitlesStatus {
        configured,
        username: config.opensubtitles_username.clone(),
        password_set: Some(config.opensubtitles_password.is_some()),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct SubsourceStatus {
    pub configured: bool,
    pub reachable: bool,
    /// `ok`, `rate_limited`, `invalid_key`, `unreachable` or `not_configured`.
    pub reason: String,
}

fn subsource_status(configured: bool, reachable: bool, reason: &str) -> SubsourceStatus {
    SubsourceStatus {
        configured,
        reachable,
        reason: reason.to_string(),
    }
}

pub async fn check_subsource_status() -> Json<SubsourceStatus> {
    let key = resolve_subsource_key(&AppConfig::load());
    if key.is_empty() {
        return Json(subsource_status(false, false, "not_configured"));
    }
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
    else {
        return Json(subsource_status(true, false, "unreachable"));
    };
    let response = client
        .get(format!(
            "{}/movies/search",
            subforge_subtitle_sources::subsource::DEFAULT_BASE_URL
        ))
        .header("X-API-Key", &key)
        .query(&[("searchType", "text"), ("q", "ping")])
        .send()
        .await;
    Json(match response {
        Ok(resp) if resp.status().is_success() => subsource_status(true, true, "ok"),
        Ok(resp) if resp.status().as_u16() == 429 => {
            subsource_status(true, true, "rate_limited")
        }
        Ok(resp) if matches!(resp.status().as_u16(), 401 | 403) => {
            subsource_status(true, false, "invalid_key")
        }
        _ => subsource_status(true, false, "unreachable"),
    })
}
