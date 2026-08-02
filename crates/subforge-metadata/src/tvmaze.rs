use crate::{urlencoding_query, Episode, MetadataError, Provider, SeriesMatch};
use serde::Deserialize;

const BASE_URL: &str = "https://api.tvmaze.com";

/// TVmaze requires no API key at all — the Settings UI should show that
/// explicitly instead of a key field.
pub struct TvmazeClient {
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct SearchHit {
    show: ShowResult,
}

#[derive(Deserialize)]
struct ShowResult {
    id: i64,
    name: String,
    premiered: Option<String>,
    image: Option<ShowImage>,
}

#[derive(Deserialize)]
struct ShowImage {
    medium: Option<String>,
}

#[derive(Deserialize)]
struct EpisodeResult {
    season: u32,
    number: u32,
    name: String,
}

impl Default for TvmazeClient {
    fn default() -> Self {
        Self::new()
    }
}

impl TvmazeClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub async fn search_shows(&self, query: &str) -> Result<Vec<SeriesMatch>, MetadataError> {
        let url = format!("{BASE_URL}/search/shows?q={}", urlencoding_query(query));
        let resp = self.client.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(MetadataError::Api(format!("TVmaze returned {}", resp.status())));
        }
        let hits: Vec<SearchHit> = resp.json().await?;
        Ok(hits
            .into_iter()
            .map(|h| SeriesMatch {
                id: h.show.id.to_string(),
                name: h.show.name,
                year: h
                    .show
                    .premiered
                    .and_then(|d| d.split('-').next().and_then(|y| y.parse().ok())),
                poster_url: h.show.image.and_then(|i| i.medium),
                provider: Provider::Tvmaze,
            })
            .collect())
    }

    pub async fn get_episodes(&self, show_id: &str) -> Result<Vec<Episode>, MetadataError> {
        let url = format!("{BASE_URL}/shows/{show_id}/episodes");
        let resp = self.client.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(MetadataError::Api(format!("TVmaze returned {}", resp.status())));
        }
        let episodes: Vec<EpisodeResult> = resp.json().await?;
        Ok(episodes
            .into_iter()
            .map(|e| Episode {
                season: e.season,
                episode: e.number,
                title: e.name,
            })
            .collect())
    }
}
