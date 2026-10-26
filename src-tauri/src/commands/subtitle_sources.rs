use crate::config::AppConfig;
use crate::credentials::CredentialStore;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use subforge_subtitle_sources::{
    find_subtitle, OpenSubtitlesCredentials, SearchRequest, SubtitleSearchResult,
};
use tauri::State;

// Fallback only: a key compiled in at build time (release builds). Users can
// set their own key in Settings → Subtitle Sources, which takes precedence.
// Without either, Subsource search is skipped rather than failing the build.
const SUBSOURCE_API_KEY: &str = match option_env!("SUBFORGE_SUBSOURCE_API_KEY") {
    Some(key) => key,
    None => "",
};
const USER_AGENT: &str = concat!("SubForge v", env!("CARGO_PKG_VERSION"));

/// The user's Subsource key when set, otherwise the build-time fallback.
fn resolve_subsource_key() -> String {
    AppConfig::load()
        .subsource_api_key
        .filter(|k| !k.trim().is_empty())
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
        /// Instant timing verdict, so a mismatched release is obvious before
        /// it is used. Omitted when the check could not run.
        #[serde(skip_serializing_if = "Option::is_none")]
        sync_verdict: Option<String>,
    },
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
    let subsource_key = resolve_subsource_key();
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
        } => {
            let subtitle_path = write_subtitle_next_to_video(&video_path, &request.language, &text)
                .map_err(|e| e.to_string())?;
            let sync_verdict = sync_verdict_for(&video_path, Path::new(&subtitle_path));
            Ok(OnlineSubtitleSearchResponse::Found {
                subtitle_path,
                source,
                matched_via,
                sync_verdict,
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
    let default = dir.join(format!("{stem}.{lang}.srt"));
    // Never replace a subtitle the user already has: keep both, and say which
    // file was written in the response.
    let out_path = if default.exists() {
        dir.join(format!("{stem}.{lang}.found.srt"))
    } else {
        default
    };
    std::fs::write(&out_path, text)?;
    Ok(out_path.to_string_lossy().to_string())
}

/// Run the instant timing check on a downloaded subtitle. Any failure is
/// reported as "unknown" rather than failing the whole search.
fn sync_verdict_for(video: &Path, subtitle: &Path) -> Option<String> {
    let paths = crate::tools::tool_paths();
    subforge_core::subtitle::sync::quick_check(&paths, video, subtitle)
        .ok()
        .map(|check| format!("{:?}", check.verdict).to_lowercase())
}

/// Validates a form submission and keeps the stored password when the box was
/// left blank. Separate from the command so it can be tested without Tauri.
fn prepare_credentials(
    incoming: OpenSubtitlesCredentials,
    existing: Option<OpenSubtitlesCredentials>,
) -> Result<OpenSubtitlesCredentials, String> {
    // Clicking Save with an untouched (empty) form must never wipe a working
    // configuration.
    let api_key = incoming.api_key.trim().to_string();
    let username = incoming.username.trim().to_string();
    if api_key.is_empty() || username.is_empty() {
        return Err("Enter both the OpenSubtitles API key and username.".to_string());
    }

    // The password is write-only in the UI. A blank box means "keep the stored
    // one" so the form can be re-saved after editing the key or username.
    let password = if incoming.password.is_empty() {
        existing.map(|c| c.password).unwrap_or_default()
    } else {
        incoming.password
    };
    if password.is_empty() {
        return Err("Enter the OpenSubtitles password.".to_string());
    }

    Ok(OpenSubtitlesCredentials {
        api_key,
        username,
        password,
    })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn set_opensubtitles_credentials(
    credential_store: State<'_, Arc<dyn CredentialStore>>,
    credentials: OpenSubtitlesCredentials,
) -> Result<(), String> {
    let existing = credential_store.get().unwrap_or(None);
    let prepared = prepare_credentials(credentials, existing)?;
    credential_store.set(&prepared)
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
    /// Filled in so the Settings form shows what is stored. The password is
    /// never returned; `password_set` only says whether one exists.
    pub username: Option<String>,
    pub api_key: Option<String>,
    pub password_set: bool,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_opensubtitles_status(
    credential_store: State<'_, Arc<dyn CredentialStore>>,
) -> Result<OpenSubtitlesStatus, String> {
    // Credential-store read errors are treated as "not configured" to degrade gracefully (never fail loudly).
    let creds = credential_store.get().unwrap_or(None);
    Ok(match creds {
        Some(c) => OpenSubtitlesStatus {
            configured: true,
            username: Some(c.username),
            api_key: Some(c.api_key),
            password_set: !c.password.is_empty(),
        },
        None => OpenSubtitlesStatus {
            configured: false,
            username: None,
            api_key: None,
            password_set: false,
        },
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsourceStatus {
    pub configured: bool,
    pub reachable: bool,
    /// Machine-readable reason: `ok`, `rate_limited`, `invalid_key`,
    /// `unreachable` or `not_configured`. The UI maps it to a localised label.
    pub reason: String,
}

impl SubsourceStatus {
    fn new(configured: bool, reachable: bool, reason: &str) -> Self {
        Self {
            configured,
            reachable,
            reason: reason.to_string(),
        }
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn check_subsource_status() -> Result<SubsourceStatus, String> {
    let key = resolve_subsource_key();
    if key.is_empty() {
        // "Not configured" is a different thing from "the site is down", and
        // the Settings card must say which one it is.
        return Ok(SubsourceStatus::new(false, false, "not_configured"));
    }
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
    else {
        return Ok(SubsourceStatus::new(true, false, "unreachable"));
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
    Ok(match response {
        Ok(resp) if resp.status().is_success() => SubsourceStatus::new(true, true, "ok"),
        Ok(resp) if resp.status().as_u16() == 429 => {
            SubsourceStatus::new(true, true, "rate_limited")
        }
        Ok(resp) if matches!(resp.status().as_u16(), 401 | 403) => {
            SubsourceStatus::new(true, false, "invalid_key")
        }
        _ => SubsourceStatus::new(true, false, "unreachable"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds(api_key: &str, username: &str, password: &str) -> OpenSubtitlesCredentials {
        OpenSubtitlesCredentials {
            api_key: api_key.into(),
            username: username.into(),
            password: password.into(),
        }
    }

    #[test]
    fn empty_form_is_rejected_instead_of_clearing_credentials() {
        let err = prepare_credentials(creds("", "", ""), Some(creds("k", "u", "p"))).unwrap_err();
        assert!(err.contains("API key"), "{err}");
    }

    #[test]
    fn blank_password_keeps_the_stored_one() {
        let prepared =
            prepare_credentials(creds(" new-key ", " updullah ", ""), Some(creds("k", "u", "old")))
                .unwrap();
        assert_eq!(prepared.api_key, "new-key");
        assert_eq!(prepared.username, "updullah");
        assert_eq!(prepared.password, "old");
    }

    #[test]
    fn a_new_password_replaces_the_stored_one() {
        let prepared =
            prepare_credentials(creds("k", "u", "new"), Some(creds("k", "u", "old"))).unwrap();
        assert_eq!(prepared.password, "new");
    }

    #[test]
    fn first_time_save_requires_a_password() {
        let err = prepare_credentials(creds("k", "u", ""), None).unwrap_err();
        assert!(err.contains("password"), "{err}");
    }
}
