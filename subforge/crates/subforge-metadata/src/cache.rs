use crate::{Episode, MetadataError};
use std::path::PathBuf;

fn cache_dir() -> PathBuf {
    dirs::cache_dir()
        .map(|d| d.join("subforge").join("metadata-cache"))
        .unwrap_or_else(|| PathBuf::from(".cache/subforge/metadata-cache"))
}

fn cache_path(provider: &str, series_id: &str) -> PathBuf {
    cache_dir().join(format!("{provider}-{series_id}.json"))
}

/// Load a previously-cached episode list for (provider, series_id), if any.
/// Returns `None` on any read/parse failure so callers just fall back to
/// re-fetching rather than treating a stale/corrupt cache file as fatal.
pub fn load(provider: &str, series_id: &str) -> Option<Vec<Episode>> {
    let path = cache_path(provider, series_id);
    let contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

pub fn save(provider: &str, series_id: &str, episodes: &[Episode]) -> Result<(), MetadataError> {
    let dir = cache_dir();
    std::fs::create_dir_all(&dir)?;
    let path = cache_path(provider, series_id);
    let contents = serde_json::to_string(episodes)?;
    std::fs::write(path, contents)?;
    Ok(())
}
