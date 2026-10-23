use crate::config::AppConfig;
use subforge_core::translate as translate_tools;
use subforge_metadata::{tmdb::TmdbClient, tvmaze::TvmazeClient};

#[tauri::command]
pub async fn test_provider_config(
    credential_store: tauri::State<'_, std::sync::Arc<dyn crate::credentials::CredentialStore>>,
    provider: String,
) -> Result<(), String> {
    let config = AppConfig::load();
    match provider.as_str() {
        "tmdb" => {
            let key = config
                .tmdb_api_key
                .filter(|k| !k.is_empty())
                .ok_or_else(|| "No TMDB API key configured".to_string())?;
            TmdbClient::new(key).test_key().await.map_err(|e| e.to_string())
        }
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
        "deepl" => {
            let key = config
                .deepl_api_key
                .filter(|k| !k.is_empty())
                .ok_or_else(|| "No DeepL API key configured".to_string())?;
            translate_tools::test_deepl_key(&key).await.map_err(|e| e.to_string())
        }
        "openai" => {
            let key = config
                .openai_api_key
                .filter(|k| !k.is_empty())
                .ok_or_else(|| "No OpenAI API key configured".to_string())?;
            translate_tools::test_openai_key(&key).await.map_err(|e| e.to_string())
        }
        "google" => {
            let key = config
                .google_translate_api_key
                .filter(|k| !k.is_empty())
                .ok_or_else(|| "No Google Translate API key configured".to_string())?;
            translate_tools::test_google_key(&key).await.map_err(|e| e.to_string())
        }
        "opensubtitles" => {
            let creds = credential_store
                .get()?
                .ok_or_else(|| "No OpenSubtitles credentials configured".to_string())?;
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
        other => Err(format!("Unknown provider: {other}")),
    }
}
