use crate::SourceError;
use serde::Deserialize;
use std::io::{Cursor, Read};

pub const DEFAULT_BASE_URL: &str = "https://api.subsource.net/api/v1";

#[derive(Debug, Clone)]
pub struct MovieMatch {
    pub movie_id: i64,
}

#[derive(Debug, Clone)]
pub struct SubtitleEntry {
    pub subtitle_id: i64,
}

#[derive(Debug, Deserialize)]
struct MovieSearchResponse {
    data: Vec<MovieSearchEntry>,
}

#[derive(Debug, Deserialize)]
struct MovieSearchEntry {
    #[serde(rename = "movieId")]
    movie_id: i64,
}

#[derive(Debug, Deserialize)]
struct SubtitleSearchResponse {
    data: Vec<SubtitleSearchEntry>,
}

#[derive(Debug, Deserialize)]
struct SubtitleSearchEntry {
    #[serde(rename = "subtitleId")]
    subtitle_id: i64,
}

pub async fn search_movies(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    query: &str,
    year: Option<u32>,
    season: Option<u32>,
) -> Result<Vec<MovieMatch>, SourceError> {
    let mut params: Vec<(&str, String)> = vec![
        ("searchType", "text".to_string()),
        ("q", query.to_string()),
    ];
    if let Some(y) = year {
        params.push(("year", y.to_string()));
    }
    if let Some(s) = season {
        params.push(("season", s.to_string()));
        params.push(("type", "series".to_string()));
    }

    let resp = client
        .get(format!("{base_url}/movies/search"))
        .header("X-API-Key", api_key)
        .query(&params)
        .send()
        .await?;
    check_status(&resp)?;
    let body: MovieSearchResponse = resp.json().await?;
    Ok(body.data.into_iter().map(|e| MovieMatch { movie_id: e.movie_id }).collect())
}

/// `language_code` is an ISO 639-1 code (e.g. "ar"); Subsource's `language`
/// filter takes a full English language name (e.g. "arabic"), so this
/// reuses `subforge_metadata::languages::english_name` (already used for
/// track-naming elsewhere) instead of a second mapping table.
pub async fn search_subtitles(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    movie_id: i64,
    language_code: &str,
) -> Result<Vec<SubtitleEntry>, SourceError> {
    let language_name = subforge_metadata::languages::english_name(language_code)
        .unwrap_or(language_code)
        .to_lowercase();

    let resp = client
        .get(format!("{base_url}/subtitles"))
        .header("X-API-Key", api_key)
        .query(&[
            ("movieId", movie_id.to_string()),
            ("language", language_name),
            ("sort", "rating".to_string()),
        ])
        .send()
        .await?;
    check_status(&resp)?;
    let body: SubtitleSearchResponse = resp.json().await?;
    Ok(body
        .data
        .into_iter()
        .map(|e| SubtitleEntry { subtitle_id: e.subtitle_id })
        .collect())
}

pub async fn download(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    subtitle_id: i64,
) -> Result<String, SourceError> {
    let resp = client
        .get(format!("{base_url}/subtitles/{subtitle_id}/download"))
        .header("X-API-Key", api_key)
        .send()
        .await?;
    check_status(&resp)?;
    let zip_bytes = resp.bytes().await?;
    extract_srt_from_zip(&zip_bytes)
}

fn extract_srt_from_zip(zip_bytes: &[u8]) -> Result<String, SourceError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(zip_bytes)).map_err(|e| SourceError::Zip(e.to_string()))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| SourceError::Zip(e.to_string()))?;
        if file.name().to_lowercase().ends_with(".srt") {
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            return Ok(contents);
        }
    }
    Err(SourceError::NotFound)
}

fn check_status(resp: &reqwest::Response) -> Result<(), SourceError> {
    match resp.status() {
        s if s.is_success() => Ok(()),
        reqwest::StatusCode::TOO_MANY_REQUESTS => Err(SourceError::RateLimited),
        other => Err(SourceError::Unauthorized(format!("HTTP {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn search_movies_parses_results() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/movies/search")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("searchType".into(), "text".into()),
                mockito::Matcher::UrlEncoded("q".into(), "Example Show".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"success":true,"data":[{"movieId":12345,"title":"Example Show","alternateTitle":null,"type":"tvseries","releaseYear":2020,"imdbId":"tt1234567","tmdbId":"1234","season":1,"subtitleCount":50,"posters":null}]}"#)
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let results = search_movies(&client, &server.url(), "test-key", "Example Show", None, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].movie_id, 12345);
    }

    #[tokio::test]
    async fn search_subtitles_parses_results() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/subtitles")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("movieId".into(), "12345".into()),
                mockito::Matcher::UrlEncoded("language".into(), "arabic".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"success":true,"data":[{"subtitleId":999,"movieId":12345,"language":"arabic","releaseInfo":["WEB-DL"],"files":1,"size":1000,"hearingImpaired":false,"foreignParts":false,"framerate":null,"productionType":null,"releaseType":null,"downloads":10,"comments":0,"rating":{"good":1,"bad":0,"total":1},"preview":"","uploaderId":1,"createdAt":"2023-01-01T00:00:00Z","contributors":[]}]}"#)
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let results = search_subtitles(&client, &server.url(), "test-key", 12345, "ar")
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].subtitle_id, 999);
    }

    #[tokio::test]
    async fn download_extracts_srt_from_zip() {
        use std::io::Write;
        let mut zip_bytes = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_bytes));
            writer
                .start_file::<_, ()>("show.s01e01.srt", zip::write::FileOptions::default())
                .unwrap();
            writer.write_all(b"1\n00:00:01,000 --> 00:00:02,000\nHi\n").unwrap();
            writer.finish().unwrap();
        }

        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/subtitles/999/download")
            .with_status(200)
            .with_header("content-type", "application/zip")
            .with_body(zip_bytes)
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let text = download(&client, &server.url(), "test-key", 999).await.unwrap();
        assert!(text.contains("Hi"));
    }

    #[tokio::test]
    async fn maps_429_to_rate_limited() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/movies/search")
            .match_query(mockito::Matcher::Any)
            .with_status(429)
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let result = search_movies(&client, &server.url(), "test-key", "x", None, None).await;
        assert!(matches!(result, Err(SourceError::RateLimited)));
    }
}
