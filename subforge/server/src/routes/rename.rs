use crate::config::AppConfig;
use axum::{http::StatusCode, Json};
use serde::Deserialize;
use subforge_metadata::{cache, matcher, parser, renamer, tmdb::TmdbClient, tvmaze::TvmazeClient};
use subforge_metadata::{Episode, RenamePreviewRow, SeriesMatch};

#[derive(Deserialize)]
pub struct SearchRequest {
    provider: String,
    query: String,
}

pub async fn search_series(
    Json(req): Json<SearchRequest>,
) -> Result<Json<Vec<SeriesMatch>>, (StatusCode, String)> {
    let results = match req.provider.as_str() {
        "tmdb" => {
            let config = AppConfig::load();
            let api_key = config
                .tmdb_api_key
                .filter(|k| !k.is_empty())
                .ok_or((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "TMDB API key not configured. Add one in Settings > API & Providers."
                        .to_string(),
                ))?;
            TmdbClient::new(api_key)
                .search_tv(&req.query)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?
        }
        "tvmaze" => TvmazeClient::new()
            .search_shows(&req.query)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?,
        other => return Err((StatusCode::BAD_REQUEST, format!("Unknown provider: {other}"))),
    };
    Ok(Json(matcher::rank_matches(&req.query, &results)))
}

#[derive(Deserialize)]
pub struct EpisodesRequest {
    provider: String,
    series_id: String,
}

pub async fn get_episodes(
    Json(req): Json<EpisodesRequest>,
) -> Result<Json<Vec<Episode>>, (StatusCode, String)> {
    if let Some(cached) = cache::load(&req.provider, &req.series_id) {
        return Ok(Json(cached));
    }

    let episodes = match req.provider.as_str() {
        "tmdb" => {
            let config = AppConfig::load();
            let api_key = config
                .tmdb_api_key
                .filter(|k| !k.is_empty())
                .ok_or((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "TMDB API key not configured. Add one in Settings > API & Providers."
                        .to_string(),
                ))?;
            TmdbClient::new(api_key)
                .get_all_episodes(&req.series_id)
                .await
                .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?
        }
        "tvmaze" => TvmazeClient::new()
            .get_episodes(&req.series_id)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?,
        other => return Err((StatusCode::BAD_REQUEST, format!("Unknown provider: {other}"))),
    };

    let _ = cache::save(&req.provider, &req.series_id, &episodes);
    Ok(Json(episodes))
}

#[derive(Deserialize)]
pub struct PreviewRequest {
    files: Vec<String>,
    episodes: Vec<Episode>,
    series_title: String,
    pattern: String,
}

pub async fn preview_rename(Json(req): Json<PreviewRequest>) -> Json<Vec<RenamePreviewRow>> {
    let rows = req
        .files
        .into_iter()
        .map(|path| {
            let (stem, ext) = split_stem_ext(&path);
            let parsed = parser::parse_filename(&stem);
            let matched_ep = parsed.season.zip(parsed.episode).and_then(|(s, e)| {
                req.episodes
                    .iter()
                    .find(|ep| ep.season == s && ep.episode == e)
            });

            match matched_ep {
                Some(ep) => RenamePreviewRow {
                    old_path: path,
                    new_name: Some(renamer::build_output_name(
                        &req.pattern,
                        &req.series_title,
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
        .collect();
    Json(rows)
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

#[derive(Deserialize)]
pub struct RenameMapping {
    old_path: String,
    new_path: String,
}

#[derive(Deserialize)]
pub struct ApplyRenameRequest {
    mapping: Vec<RenameMapping>,
}

pub async fn apply_rename(
    Json(req): Json<ApplyRenameRequest>,
) -> Result<Json<usize>, (StatusCode, String)> {
    let mut renamed = 0;
    for m in &req.mapping {
        std::fs::rename(&m.old_path, &m.new_path).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to rename {}: {e}", m.old_path),
            )
        })?;
        renamed += 1;
    }
    Ok(Json(renamed))
}
