use crate::{urlencoding_query, Episode, MetadataError, Provider, SeriesMatch};
use serde::Deserialize;

const BASE_URL: &str = "https://api.themoviedb.org/3";

pub struct TmdbClient {
    api_key: String,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct SearchResponse {
    results: Vec<SearchResult>,
}

#[derive(Deserialize)]
struct SearchResult {
    id: i64,
    name: String,
    first_air_date: Option<String>,
    poster_path: Option<String>,
}

#[derive(Deserialize)]
struct TvDetails {
    number_of_seasons: u32,
}

#[derive(Deserialize)]
struct SeasonResponse {
    episodes: Vec<SeasonEpisode>,
}

#[derive(Deserialize)]
struct SeasonEpisode {
    season_number: u32,
    episode_number: u32,
    name: String,
}

impl TmdbClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    /// Lightweight call used by the Settings "Test" button to validate a key.
    pub async fn test_key(&self) -> Result<(), MetadataError> {
        let url = format!("{BASE_URL}/authentication?api_key={}", self.api_key);
        let resp = self.client.get(&url).send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(MetadataError::Api(format!("TMDB returned {}", resp.status())))
        }
    }

    pub async fn search_tv(&self, query: &str) -> Result<Vec<SeriesMatch>, MetadataError> {
        let url = format!(
            "{BASE_URL}/search/tv?api_key={}&language=en-US&query={}",
            self.api_key,
            urlencoding_query(query)
        );
        let resp = self.client.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(MetadataError::Api(format!("TMDB returned {}", resp.status())));
        }
        let body: SearchResponse = resp.json().await?;
        Ok(body
            .results
            .into_iter()
            .map(|r| SeriesMatch {
                id: r.id.to_string(),
                name: r.name,
                year: r
                    .first_air_date
                    .and_then(|d| d.split('-').next().and_then(|y| y.parse().ok())),
                poster_url: r
                    .poster_path
                    .map(|p| format!("https://image.tmdb.org/t/p/w200{p}")),
                provider: Provider::Tmdb,
            })
            .collect())
    }

    /// Fetch every episode across all seasons for a series (used to populate
    /// the rename preview cache — TMDB has no single "all episodes" endpoint).
    pub async fn get_all_episodes(&self, series_id: &str) -> Result<Vec<Episode>, MetadataError> {
        let details_url = format!("{BASE_URL}/tv/{series_id}?api_key={}&language=en-US", self.api_key);
        let details: TvDetails = self.client.get(&details_url).send().await?.json().await?;

        let mut episodes = Vec::new();
        for season in 1..=details.number_of_seasons {
            let url = format!(
                "{BASE_URL}/tv/{series_id}/season/{season}?api_key={}&language=en-US",
                self.api_key
            );
            let resp = self.client.get(&url).send().await?;
            if !resp.status().is_success() {
                continue;
            }
            let season_data: SeasonResponse = resp.json().await?;
            episodes.extend(season_data.episodes.into_iter().map(|e| Episode {
                season: e.season_number,
                episode: e.episode_number,
                title: e.name,
            }));
        }
        Ok(episodes)
    }
}
