use crate::config::AppConfig;
use subforge_metadata::{cache, matcher, parser, renamer, tmdb::TmdbClient, tvmaze::TvmazeClient};
use subforge_metadata::{Episode, ParsedFilename, RenamePreviewRow, SeriesMatch};

// These commands take multi-word snake_case parameters that must match the
// Axum server's request struct field names exactly (no camelCase rename
// there). `rename_all = "snake_case"` keeps the Tauri IPC argument names
// consistent with that instead of Tauri's default camelCase conversion,
// so the same frontend call sites work unmodified on both backends.
#[tauri::command(rename_all = "snake_case")]
pub fn parse_filename(name: String) -> ParsedFilename {
    let (stem, _ext) = split_stem_ext(&name);
    parser::parse_filename(&stem)
}

#[tauri::command(rename_all = "snake_case")]
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

#[tauri::command(rename_all = "snake_case")]
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

#[tauri::command(rename_all = "snake_case")]
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
            let matched_ep = parsed
                .season
                .zip(parsed.episode)
                .and_then(|(s, e)| episodes.iter().find(|ep| ep.season == s && ep.episode == e));

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
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_string();
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

#[tauri::command(rename_all = "snake_case")]
pub fn apply_rename(mapping: Vec<RenameMapping>) -> Result<usize, String> {
    // Validates first (no overwrites, no folder escapes), then renames.
    let mapping: Vec<(String, String)> = mapping
        .into_iter()
        .map(|m| (m.old_path, m.new_path))
        .collect();
    subforge_metadata::renamer::apply_rename_mapping(&mapping)
}
