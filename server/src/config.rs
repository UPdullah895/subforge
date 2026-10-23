use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_language() -> String {
    "en".to_string()
}

fn default_target_langs() -> Vec<String> {
    vec!["ar".to_string()]
}

fn default_true() -> bool {
    true
}

fn default_search_online_before_translate() -> bool {
    true
}

/// Version written into freshly created configs; older files deserialise as 0.
pub const CURRENT_CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub theme: String,
    // Falls back to "en" for config files saved before this field existed.
    // This struct shares config.json on disk with src-tauri's AppConfig
    // (both resolve to ~/.config/subforge/config.json), so its shape must
    // stay a superset-compatible subset to avoid dropping fields on save.
    #[serde(default = "default_language")]
    pub language: String,
    pub mkvmerge_path: Option<String>,
    pub mkvextract_path: Option<String>,
    pub ffmpeg_path: Option<String>,
    pub ffprobe_path: Option<String>,
    #[serde(default)]
    pub mkvpropedit_path: Option<String>,
    /// Bumped by one-time config migrations (see src-tauri `config::migrate`).
    #[serde(default)]
    pub config_version: u32,
    pub default_engine: Option<String>,
    pub default_source_lang: Option<String>,
    pub default_target_lang: Option<String>,
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
    // Wrap right-to-left (Arabic/Hebrew/Persian) subtitle lines in an RTL
    // embedding so players put punctuation on the correct side.
    #[serde(default = "default_true")]
    pub fix_rtl_punctuation: bool,
    pub opensubtitles_api_key: Option<String>,
    pub opensubtitles_username: Option<String>,
    pub opensubtitles_password: Option<String>,
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
            mkvpropedit_path: None,
            config_version: CURRENT_CONFIG_VERSION,
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
            fix_rtl_punctuation: true,
            opensubtitles_api_key: None,
            opensubtitles_username: None,
            opensubtitles_password: None,
        }
    }
}

pub const SECRET_MASK: &str = "********";

impl AppConfig {
    pub fn is_mask(value: &str) -> bool {
        value == SECRET_MASK
    }

    fn secrets_mut(&mut self) -> [&mut Option<String>; 5] {
        [
            &mut self.tmdb_api_key,
            &mut self.deepl_api_key,
            &mut self.openai_api_key,
            &mut self.google_translate_api_key,
            &mut self.opensubtitles_password,
        ]
    }

    /// Copy for the HTTP API: non-empty secrets are replaced with a mask so
    /// stored keys are never sent back over the wire.
    pub fn masked(mut self) -> Self {
        for s in self.secrets_mut() {
            if s.as_deref().is_some_and(|v| !v.is_empty()) {
                *s = Some(SECRET_MASK.to_string());
            }
        }
        // The API key is a credential too.
        if self
            .opensubtitles_api_key
            .as_deref()
            .is_some_and(|v| !v.is_empty())
        {
            self.opensubtitles_api_key = Some(SECRET_MASK.to_string());
        }
        self
    }

    /// Inverse of `masked`: any secret still equal to the mask keeps the
    /// value already stored on disk.
    pub fn restore_masked(&mut self, mut existing: AppConfig) {
        let mut old = existing.secrets_mut().map(|s| s.take());
        for (s, o) in self.secrets_mut().into_iter().zip(old.iter_mut()) {
            if s.as_deref().is_some_and(Self::is_mask) {
                *s = o.take();
            }
        }
        if self
            .opensubtitles_api_key
            .as_deref()
            .is_some_and(Self::is_mask)
        {
            self.opensubtitles_api_key = existing.opensubtitles_api_key.take();
        }
    }

    pub fn config_dir() -> anyhow::Result<PathBuf> {
        // `SUBFORGE_CONFIG_DIR` relocates the config (containers, tests).
        let dir = match std::env::var_os("SUBFORGE_CONFIG_DIR").filter(|d| !d.is_empty()) {
            Some(dir) => PathBuf::from(dir),
            None => dirs::config_dir()
                .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?
                .join("subforge"),
        };
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

/// Points `SUBFORGE_CONFIG_DIR` at a fresh temp dir for the guard's lifetime.
/// Holding the guard also serialises tests that touch the process environment.
#[cfg(test)]
pub(crate) struct TestConfigDir {
    dir: PathBuf,
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl TestConfigDir {
    pub(crate) fn new() -> Self {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("subforge_cfg_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("SUBFORGE_CONFIG_DIR", &dir);
        Self { dir, _lock: lock }
    }
}

#[cfg(test)]
impl Drop for TestConfigDir {
    fn drop(&mut self) {
        std::env::remove_var("SUBFORGE_CONFIG_DIR");
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_secrets() -> AppConfig {
        AppConfig {
            deepl_api_key: Some("real-deepl".into()),
            openai_api_key: Some(String::new()),
            opensubtitles_password: Some("pw".into()),
            opensubtitles_api_key: Some("osk".into()),
            ..AppConfig::default()
        }
    }

    #[test]
    fn masked_hides_non_empty_secrets_only() {
        let m = with_secrets().masked();
        assert_eq!(m.deepl_api_key.as_deref(), Some(SECRET_MASK));
        assert_eq!(m.opensubtitles_password.as_deref(), Some(SECRET_MASK));
        assert_eq!(m.opensubtitles_api_key.as_deref(), Some(SECRET_MASK));
        assert_eq!(m.openai_api_key.as_deref(), Some(""));
        assert_eq!(m.tmdb_api_key, None);
    }

    #[test]
    fn restore_masked_keeps_stored_values_and_accepts_new_ones() {
        let stored = with_secrets();
        let mut incoming = stored.clone().masked();
        incoming.tmdb_api_key = Some("new-tmdb".into());
        incoming.restore_masked(stored);
        assert_eq!(incoming.deepl_api_key.as_deref(), Some("real-deepl"));
        assert_eq!(incoming.opensubtitles_password.as_deref(), Some("pw"));
        assert_eq!(incoming.opensubtitles_api_key.as_deref(), Some("osk"));
        assert_eq!(incoming.tmdb_api_key.as_deref(), Some("new-tmdb"));
    }

    #[test]
    fn cleared_secret_stays_cleared() {
        let stored = with_secrets();
        let mut incoming = stored.clone().masked();
        incoming.deepl_api_key = Some(String::new());
        incoming.restore_masked(stored);
        assert_eq!(incoming.deepl_api_key.as_deref(), Some(""));
    }
}
