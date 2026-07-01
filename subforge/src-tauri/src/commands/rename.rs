use crate::config::AppConfig;
use subforge_metadata::{cache, matcher, parser, renamer, tmdb::TmdbClient, tvmaze::TvmazeClient};
use subforge_metadata::{Episode, RenamePreviewRow, SeriesMatch};

#[tauri::command]
pub async fn search_series(provider: String, query: String) -> Result<Vec<SeriesMatch>, String> {
    let results = match provider.as_str() {
        "tmdb" => {
            let config = AppConfig::load();
            let api_key = config
                .tmdb_api_key
                .filter(|k| !k.is_empty())
                .ok_or("TMDB API key not configured. Add one in Settings > API & Providers.")?;
            TmdbClient::new(api_key)
                .search_tv(&query)
                .await
                .map_err(|e| e.to_string())?
        }
        "tvmaze" => TvmazeClient::new()
            .search_shows(&query)
            .await
            .map_err(|e| e.to_string())?,
        other => return Err(format!("Unknown provider: {other}")),
    };
    Ok(matcher::rank_matches(&query, &results))
}

#[tauri::command]
pub async fn get_episodes(provider: String, series_id: String) -> Result<Vec<Episode>, String> {
    if let Some(cached) = cache::load(&provider, &series_id) {
        return Ok(cached);
    }

    let episodes = match provider.as_str() {
        "tmdb" => {
            let config = AppConfig::load();
            let api_key = config
                .tmdb_api_key
                .filter(|k| !k.is_empty())
                .ok_or("TMDB API key not configured. Add one in Settings > API & Providers.")?;
            TmdbClient::new(api_key)
                .get_all_episodes(&series_id)
                .await
                .map_err(|e| e.to_string())?
        }
        "tvmaze" => TvmazeClient::new()
            .get_episodes(&series_id)
            .await
            .map_err(|e| e.to_string())?,
        other => return Err(format!("Unknown provider: {other}")),
    };

    let _ = cache::save(&provider, &series_id, &episodes);
    Ok(episodes)
}

#[tauri::command]
pub fn preview_rename(
    files: Vec<String>,
    episodes: Vec<Episode>,
    series_title: String,
    pattern: String,
) -> Vec<RenamePreviewRow> {
    files
        .into_iter()
        .map(|path| {
            let (stem, ext) = split_stem_ext(&path);
            let parsed = parser::parse_filename(&stem);
            let matched_ep = parsed.season.zip(parsed.episode).and_then(|(s, e)| {
                episodes
                    .iter()
                    .find(|ep| ep.season == s && ep.episode == e)
            });

            match matched_ep {
                Some(ep) => RenamePreviewRow {
                    old_path: path,
                    new_name: Some(renamer::build_output_name(
                        &pattern,
                        &series_title,
                        ep,
                        &parsed.tags,
                        &ext,
                    )),
                    matched: true,
                    season: Some(ep.season),
                    episode: Some(ep.episode),
                },
                None => RenamePreviewRow {
                    old_path: path,
                    new_name: None,
                    matched: false,
                    season: parsed.season,
                    episode: parsed.episode,
                },
            }
        })
        .collect()
}

fn split_stem_ext(path: &str) -> (String, String) {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_string();
    match name.rfind('.') {
        Some(idx) if idx > 0 => (name[..idx].to_string(), name[idx + 1..].to_string()),
        _ => (name, String::new()),
    }
}

#[derive(serde::Deserialize)]
pub struct RenameMapping {
    pub old_path: String,
    pub new_path: String,
}

#[tauri::command]
pub fn apply_rename(mapping: Vec<RenameMapping>) -> Result<usize, String> {
    let mut renamed = 0;
    for m in &mapping {
        std::fs::rename(&m.old_path, &m.new_path)
            .map_err(|e| format!("Failed to rename {}: {e}", m.old_path))?;
        renamed += 1;
    }
    Ok(renamed)
}
