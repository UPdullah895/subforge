use keyring::Entry;
use subforge_subtitle_sources::OpenSubtitlesCredentials;

const SERVICE: &str = "subforge";
const ACCOUNT: &str = "opensubtitles";

pub trait CredentialStore: Send + Sync {
    fn get(&self) -> Result<Option<OpenSubtitlesCredentials>, String>;
    fn set(&self, creds: &OpenSubtitlesCredentials) -> Result<(), String>;
    fn clear(&self) -> Result<(), String>;
}

pub struct KeyringCredentialStore;

impl CredentialStore for KeyringCredentialStore {
    fn get(&self) -> Result<Option<OpenSubtitlesCredentials>, String> {
        let entry = Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|e| e.to_string()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, creds: &OpenSubtitlesCredentials) -> Result<(), String> {
        let entry = Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())?;
        let json = serde_json::to_string(creds).map_err(|e| e.to_string())?;
        entry.set_password(&json).map_err(|e| e.to_string())
    }

    fn clear(&self) -> Result<(), String> {
        let entry = Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct MockCredentialStore {
        stored: Mutex<Option<OpenSubtitlesCredentials>>,
    }

    impl MockCredentialStore {
        fn new() -> Self {
            Self {
                stored: Mutex::new(None),
            }
        }
    }

    impl CredentialStore for MockCredentialStore {
        fn get(&self) -> Result<Option<OpenSubtitlesCredentials>, String> {
            Ok(self.stored.lock().unwrap().clone())
        }
        fn set(&self, creds: &OpenSubtitlesCredentials) -> Result<(), String> {
            *self.stored.lock().unwrap() = Some(creds.clone());
            Ok(())
        }
        fn clear(&self) -> Result<(), String> {
            *self.stored.lock().unwrap() = None;
            Ok(())
        }
    }

    fn sample_creds() -> OpenSubtitlesCredentials {
        OpenSubtitlesCredentials {
            api_key: "key".into(),
            username: "user".into(),
            password: "pass".into(),
        }
    }

    #[test]
    fn starts_empty() {
        let store = MockCredentialStore::new();
        assert!(store.get().unwrap().is_none());
    }

    #[test]
    fn set_then_get_round_trips() {
        let store = MockCredentialStore::new();
        store.set(&sample_creds()).unwrap();
        let got = store.get().unwrap().unwrap();
        assert_eq!(got.username, "user");
    }

    #[test]
    fn clear_removes_stored_credentials() {
        let store = MockCredentialStore::new();
        store.set(&sample_creds()).unwrap();
        store.clear().unwrap();
        assert!(store.get().unwrap().is_none());
    }
}
