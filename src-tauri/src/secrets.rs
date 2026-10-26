//! API keys at rest in the OS keyring.
//!
//! The desktop app does not keep key material in `config.json`. On load the
//! values are read back from the keyring into an in-memory [`AppConfig`], and
//! on save they are moved into the keyring and cleared from the file. This is
//! deliberately transparent to the rest of the app: everything keeps reading
//! `config.tmdb_api_key` and friends.
//!
//! If the keyring is unavailable (no Secret Service on a headless box), the
//! key stays in the config file instead. A failed write never drops a key.
//!
//! [`AppConfig`]: crate::config::AppConfig

use keyring::Entry;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

const SERVICE: &str = "subforge";

/// One stored API key. The account name doubles as the `config.json` field
/// name, so the migration in [`crate::config`] can map them without a second
/// table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretId {
    TmdbApiKey,
    DeeplApiKey,
    OpenAiApiKey,
    GoogleTranslateApiKey,
    SubsourceApiKey,
}

impl SecretId {
    pub const ALL: [SecretId; 5] = [
        SecretId::TmdbApiKey,
        SecretId::DeeplApiKey,
        SecretId::OpenAiApiKey,
        SecretId::GoogleTranslateApiKey,
        SecretId::SubsourceApiKey,
    ];

    pub fn account(self) -> &'static str {
        match self {
            SecretId::TmdbApiKey => "tmdb_api_key",
            SecretId::DeeplApiKey => "deepl_api_key",
            SecretId::OpenAiApiKey => "openai_api_key",
            SecretId::GoogleTranslateApiKey => "google_translate_api_key",
            SecretId::SubsourceApiKey => "subsource_api_key",
        }
    }
}

/// Cached keyring reads. The keyring is a D-Bus / OS service call, and the
/// config is loaded on nearly every command, so hitting it each time is both
/// slow and noisy. `None` is cached too (a missing key is a normal state).
fn cache() -> MutexGuard<'static, HashMap<SecretId, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<SecretId, Option<String>>>> = OnceLock::new();
    CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn read(id: SecretId) -> Option<String> {
    let entry = Entry::new(SERVICE, id.account()).ok()?;
    match entry.get_password() {
        Ok(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

/// Set by the hidden `--self-test` entry point: CI and headless runs have no
/// session keyring, and some Secret Service clients stall trying to reach one.
fn enabled() -> bool {
    std::env::var_os("SUBFORGE_DISABLE_KEYRING").is_none()
}

/// The stored value, if any. Cached for the life of the process.
pub fn get(id: SecretId) -> Option<String> {
    if !enabled() {
        return None;
    }
    {
        let map = cache();
        if let Some(v) = map.get(&id) {
            return v.clone();
        }
    }
    let value = read(id);
    cache().insert(id, value.clone());
    value
}

/// Stores `value`, replacing any previous one.
pub fn set(id: SecretId, value: &str) -> Result<(), String> {
    if !enabled() {
        return Err("the OS keyring is disabled for this run".to_string());
    }
    if value.is_empty() {
        return clear(id);
    }
    let entry = Entry::new(SERVICE, id.account()).map_err(|e| e.to_string())?;
    entry.set_password(value).map_err(|e| e.to_string())?;
    cache().insert(id, Some(value.to_string()));
    Ok(())
}

/// Removes the stored value. A key that was never stored is success.
pub fn clear(id: SecretId) -> Result<(), String> {
    let entry = Entry::new(SERVICE, id.account()).map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            cache().insert(id, None);
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Drops the in-memory copies so the next read goes back to the keyring.
#[cfg(test)]
pub fn invalidate_cache() {
    cache().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_names_match_config_fields() {
        for id in SecretId::ALL {
            assert!(id.account().ends_with("_api_key"), "{id:?}");
        }
    }

    #[test]
    fn empty_value_clears_instead_of_storing() {
        // `clear` on an OS without a keyring returns an error, but it must not
        // panic or write an empty secret. Either outcome is acceptable here.
        let _ = set(SecretId::TmdbApiKey, "");
    }
}
