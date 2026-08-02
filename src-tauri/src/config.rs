use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_language() -> String {
    "en".to_string()
}

fn default_target_langs() -> Vec<String> {
    vec!["ar".to_string()]
}

fn default_search_online_before_translate() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub theme: String,
    // Falls back to "en" for config files saved before this field existed.
    #[serde(default = "default_language")]
    pub language: String,
    pub mkvmerge_path: Option<String>,
    pub mkvextract_path: Option<String>,
    pub ffmpeg_path: Option<String>,
    pub ffprobe_path: Option<String>,
    pub default_engine: Option<String>,
    pub default_source_lang: Option<String>,
    pub default_target_lang: Option<String>,
    // Remembers the last set of target languages used in the Auto-Translate
    // wizard's multi-language batch mode, separately from the single
    // `default_target_lang` used by the plain Translate page.
    #[serde(default = "default_target_langs")]
    pub default_target_langs: Vec<String>,
    pub default_output_dir: Option<String>,
    pub tmdb_api_key: Option<String>,
    pub deepl_api_key: Option<String>,
    #[serde(default)]
    pub deepl_pro: bool,
    pub openai_api_key: Option<String>,
    pub openai_model: Option<String>,
    pub google_translate_api_key: Option<String>,
    pub ollama_base_url: Option<String>,
    pub libretranslate_base_url: Option<String>,
    #[serde(default = "default_search_online_before_translate")]
    pub search_online_before_translate: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            language: default_language(),
            mkvmerge_path: None,
            mkvextract_path: None,
            ffmpeg_path: None,
            ffprobe_path: None,
            default_engine: Some("ollama".to_string()),
            default_source_lang: Some("en".to_string()),
            default_target_lang: Some("ar".to_string()),
            default_target_langs: default_target_langs(),
            default_output_dir: None,
            tmdb_api_key: None,
            deepl_api_key: None,
            deepl_pro: false,
            openai_api_key: None,
            openai_model: None,
            google_translate_api_key: None,
            ollama_base_url: None,
            libretranslate_base_url: None,
            search_online_before_translate: default_search_online_before_translate(),
        }
    }
}

impl AppConfig {
    pub fn config_dir() -> anyhow::Result<PathBuf> {
        let dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?
            .join("subforge");
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn config_path() -> anyhow::Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.json"))
    }

    pub fn load() -> Self {
        match Self::config_path() {
            Ok(path) => {
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    if let Ok(config) = serde_json::from_str(&contents) {
                        return config;
                    }
                }
            }
            Err(e) => eprintln!("Failed to get config path: {}", e),
        }
        Self::default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path()?;
        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }
}
