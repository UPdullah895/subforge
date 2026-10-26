use crate::config::AppConfig;
use axum::{http::StatusCode, Json};
use serde::Deserialize;
use subforge_core::translate as translate_tools;
use subforge_metadata::{tmdb::TmdbClient, tvmaze::TvmazeClient};

#[derive(Deserialize)]
pub struct TestProviderRequest {
    provider: String,
}

pub async fn test_provider_config(
    Json(req): Json<TestProviderRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let config = AppConfig::load();
    let result: Result<(), String> = match req.provider.as_str() {
        "tmdb" => match config.tmdb_api_key.filter(|k| !k.is_empty()) {
            Some(key) => TmdbClient::new(key)
                .test_key()
                .await
                .map_err(|e| e.to_string()),
            None => Err("No TMDB API key configured".to_string()),
        },
        "tvmaze" => TvmazeClient::new()
            .search_shows("test")
            .await
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "ollama" => {
            let host = config
                .ollama_base_url
                .filter(|h| !h.is_empty())
                .unwrap_or_else(|| "http://localhost:11434".to_string());
            translate_tools::detect_ollama(&host)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        "libretranslate" => {
            let host = config
                .libretranslate_base_url
                .filter(|h| !h.is_empty())
                .unwrap_or_else(|| "http://localhost:5000".to_string());
            translate_tools::detect_libretranslate(&host)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        "deepl" => match config.deepl_api_key.filter(|k| !k.is_empty()) {
            Some(key) => translate_tools::test_deepl_key(&key)
                .await
                .map_err(|e| e.to_string()),
            None => Err("No DeepL API key configured".to_string()),
        },
        "openai" => match config.openai_api_key.filter(|k| !k.is_empty()) {
            Some(key) => translate_tools::test_openai_key(&key)
                .await
                .map_err(|e| e.to_string()),
            None => Err("No OpenAI API key configured".to_string()),
        },
        "google" => match config.google_translate_api_key.filter(|k| !k.is_empty()) {
            Some(key) => translate_tools::test_google_key(&key)
                .await
                .map_err(|e| e.to_string()),
            None => Err("No Google Translate API key configured".to_string()),
        },
        "opensubtitles" => {
            match (
                config.opensubtitles_api_key.clone(),
                config.opensubtitles_username.clone(),
                config.opensubtitles_password.clone(),
            ) {
                (Some(api_key), Some(username), Some(password)) => {
                    let creds = subforge_subtitle_sources::OpenSubtitlesCredentials {
                        api_key,
                        username,
                        password,
                    };
                    let client = reqwest::Client::new();
                    subforge_subtitle_sources::opensubtitles::login(
                        &client,
                        subforge_subtitle_sources::opensubtitles::DEFAULT_BASE_URL,
                        &creds,
                        &format!("SubForge v{}", env!("CARGO_PKG_VERSION")),
                    )
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
                }
                _ => Err("No OpenSubtitles credentials configured".to_string()),
            }
        }
        other => Err(format!("Unknown provider: {other}")),
    };

    result
        .map(|_| StatusCode::OK)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}
