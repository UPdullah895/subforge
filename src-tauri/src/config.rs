use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use subforge_deps::{find_executable, SearchEnv};

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
    // Wrap right-to-left (Arabic/Hebrew/Persian) subtitle lines in an RTL
    // embedding so players put punctuation on the correct side.
    #[serde(default = "default_true")]
    pub fix_rtl_punctuation: bool,
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

    /// The saved config, or `None` when it is missing or unreadable.
    pub fn try_load() -> Option<Self> {
        match Self::config_path() {
            Ok(path) => {
                let contents = std::fs::read_to_string(&path).ok()?;
                serde_json::from_str(&contents).ok()
            }
            Err(e) => {
                eprintln!("Failed to get config path: {}", e);
                None
            }
        }
    }

    pub fn load() -> Self {
        Self::try_load().unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path()?;
        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }
}

/// One-time migration (`config_version` 0 -> 1). Earlier versions stored the
/// auto-detected tool locations in the config; those would now shadow the
/// bundled copies, so any stored path that is just what a `PATH` search finds
/// is cleared. Hand-picked paths are kept. Returns whether anything changed.
pub fn migrate(cfg: &mut AppConfig, env: &SearchEnv) -> bool {
    if cfg.config_version >= 1 {
        return false;
    }
    let slots: [(&str, &mut Option<String>); 5] = [
        ("mkvmerge", &mut cfg.mkvmerge_path),
        ("mkvextract", &mut cfg.mkvextract_path),
        ("mkvpropedit", &mut cfg.mkvpropedit_path),
        ("ffmpeg", &mut cfg.ffmpeg_path),
        ("ffprobe", &mut cfg.ffprobe_path),
    ];
    for (name, slot) in slots {
        let Some(stored) = slot.as_deref().filter(|p| !p.is_empty()) else {
            continue;
        };
        if find_executable(name, env).is_some_and(|found| same_file(Path::new(stored), &found)) {
            *slot = None;
        }
    }
    cfg.config_version = 1;
    true
}

fn same_file(a: &Path, b: &Path) -> bool {
    let a = std::fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let b = std::fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// Startup hook: migrates the saved config in place. A missing or corrupt
/// config is left alone (never overwritten with defaults).
pub fn migrate_saved_config() {
    if let Some(mut cfg) = AppConfig::try_load() {
        if migrate(&mut cfg, &SearchEnv::from_process()) {
            if let Err(e) = cfg.save() {
                eprintln!("Failed to save migrated config: {e}");
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use subforge_deps::Os;

    fn temp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("subforge_cfg_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn fake_exe(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn env_with(dir: &Path) -> SearchEnv {
        SearchEnv {
            dirs: vec![dir.to_path_buf()],
            os: Os::current(),
        }
    }

    fn legacy() -> AppConfig {
        AppConfig {
            config_version: 0,
            ..AppConfig::default()
        }
    }

    #[test]
    fn migrate_clears_autodetected_paths() {
        let sys = temp_dir();
        let found = fake_exe(&sys, "mkvmerge");
        // Same file reached through a symlink: canonicalisation must see through it.
        let alias_dir = temp_dir();
        let alias = alias_dir.join("mkvmerge-link");
        std::os::unix::fs::symlink(&found, &alias).unwrap();
        fake_exe(&sys, "ffmpeg");

        let mut cfg = legacy();
        cfg.mkvmerge_path = Some(alias.to_string_lossy().into());
        cfg.ffmpeg_path = Some(sys.join("ffmpeg").to_string_lossy().into());

        assert!(migrate(&mut cfg, &env_with(&sys)));
        assert_eq!(cfg.mkvmerge_path, None);
        assert_eq!(cfg.ffmpeg_path, None);
        assert_eq!(cfg.config_version, 1);
    }

    #[test]
    fn migrate_keeps_hand_picked_paths() {
        let sys = temp_dir();
        fake_exe(&sys, "mkvmerge");
        let mine = temp_dir();
        let picked = fake_exe(&mine, "mkvmerge");
        // Nothing on the search path for ffprobe, so its stored path is hand-picked too.
        let other = fake_exe(&mine, "ffprobe");

        let mut cfg = legacy();
        cfg.mkvmerge_path = Some(picked.to_string_lossy().into());
        cfg.ffprobe_path = Some(other.to_string_lossy().into());

        assert!(migrate(&mut cfg, &env_with(&sys)));
        assert_eq!(cfg.mkvmerge_path.as_deref(), Some(picked.to_str().unwrap()));
        assert_eq!(cfg.ffprobe_path.as_deref(), Some(other.to_str().unwrap()));
        assert_eq!(cfg.config_version, 1);
    }

    #[test]
    fn migrate_runs_once() {
        let sys = temp_dir();
        let found = fake_exe(&sys, "mkvmerge");

        let mut cfg = legacy();
        assert!(migrate(&mut cfg, &env_with(&sys)));
        assert_eq!(cfg.config_version, 1);

        // A path chosen after the migration is never touched, even if it equals PATH's.
        cfg.mkvmerge_path = Some(found.to_string_lossy().into());
        assert!(!migrate(&mut cfg, &env_with(&sys)));
        assert!(cfg.mkvmerge_path.is_some());
    }

    #[test]
    fn old_config_without_new_fields_loads_as_version_zero() {
        let cfg: AppConfig = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(cfg.config_version, 0);
        assert_eq!(cfg.mkvpropedit_path, None);
    }
}
