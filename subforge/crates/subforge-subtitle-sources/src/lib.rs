pub mod opensubtitles;
pub mod subsource;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("rate limited")]
    RateLimited,
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("not found")]
    NotFound,
    #[error("file too small to hash")]
    FileTooSmall,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(String),
}

/// Already-resolved search metadata for one video — from the rename step's
/// TMDB/TVmaze match when available, or a best-effort filename-parser guess
/// otherwise. `language` is the ISO 639-1 target language code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub title: String,
    pub year: Option<u32>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenSubtitlesCredentials {
    pub api_key: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SubtitleSearchResult {
    Found {
        downloaded_path: String,
        source: String,
        matched_via: String,
    },
    NotFound,
}

/// Public entry point — uses the real OpenSubtitles/Subsource hosts.
pub async fn find_subtitle(
    request: &SearchRequest,
    video_path: &std::path::Path,
    opensubtitles_creds: Option<&OpenSubtitlesCredentials>,
    subsource_api_key: &str,
    user_agent: &str,
) -> SubtitleSearchResult {
    find_subtitle_with_base_urls(
        request,
        video_path,
        opensubtitles_creds,
        subsource_api_key,
        user_agent,
        opensubtitles::DEFAULT_BASE_URL,
        subsource::DEFAULT_BASE_URL,
    )
    .await
}

/// Same as `find_subtitle` but with injectable base URLs, so tests can point
/// at a mock server instead of the real APIs.
#[allow(clippy::too_many_arguments)]
async fn find_subtitle_with_base_urls(
    request: &SearchRequest,
    video_path: &std::path::Path,
    opensubtitles_creds: Option<&OpenSubtitlesCredentials>,
    subsource_api_key: &str,
    user_agent: &str,
    opensubtitles_base_url: &str,
    subsource_base_url: &str,
) -> SubtitleSearchResult {
    let client = reqwest::Client::new();

    if let Some(creds) = opensubtitles_creds {
        if let Some(found) =
            try_opensubtitles(&client, request, video_path, creds, user_agent, opensubtitles_base_url).await
        {
            return found;
        }
    }

    if let Some(found) =
        try_subsource(&client, request, subsource_api_key, subsource_base_url).await
    {
        return found;
    }

    SubtitleSearchResult::NotFound
}

async fn try_opensubtitles(
    client: &reqwest::Client,
    request: &SearchRequest,
    video_path: &std::path::Path,
    creds: &OpenSubtitlesCredentials,
    user_agent: &str,
    base_url: &str,
) -> Option<SubtitleSearchResult> {
    let login = opensubtitles::login(client, base_url, creds, user_agent).await.ok()?;

    let hash_match = match opensubtitles::compute_hash(video_path) {
        Ok((hash, _)) => opensubtitles::search_by_hash(
            client,
            base_url,
            &creds.api_key,
            user_agent,
            &hash,
            &request.language,
        )
        .await
        .ok()
        .and_then(|matches| matches.into_iter().next())
        .map(|m| (m, "hash")),
        Err(_) => None,
    };

    let (matched, matched_via) = match hash_match {
        Some((m, via)) => (m, via),
        None => {
            let title_matches = opensubtitles::search_by_title(
                client,
                base_url,
                &creds.api_key,
                user_agent,
                &request.title,
                request.season,
                request.episode,
                request.year,
                &request.language,
            )
            .await
            .ok()?;
            (title_matches.into_iter().next()?, "title")
        }
    };

    let text = opensubtitles::download(
        client,
        base_url,
        &creds.api_key,
        &login.token,
        user_agent,
        matched.file_id,
    )
    .await
    .ok()?;

    Some(SubtitleSearchResult::Found {
        downloaded_path: text,
        source: "opensubtitles".to_string(),
        matched_via: matched_via.to_string(),
    })
}

async fn try_subsource(
    client: &reqwest::Client,
    request: &SearchRequest,
    api_key: &str,
    base_url: &str,
) -> Option<SubtitleSearchResult> {
    let movies = subsource::search_movies(
        client,
        base_url,
        api_key,
        &request.title,
        request.year,
        request.season,
    )
    .await
    .ok()?;
    let movie = movies.into_iter().next()?;

    let subtitles =
        subsource::search_subtitles(client, base_url, api_key, movie.movie_id, &request.language)
            .await
            .ok()?;
    let subtitle = subtitles.into_iter().next()?;

    let text = subsource::download(client, base_url, api_key, subtitle.subtitle_id)
        .await
        .ok()?;

    Some(SubtitleSearchResult::Found {
        downloaded_path: text,
        source: "subsource".to_string(),
        matched_via: "title".to_string(),
    })
}

#[cfg(test)]
mod orchestration_tests {
    use super::*;
    use std::io::Write;

    fn test_request() -> SearchRequest {
        SearchRequest {
            title: "Example Show".into(),
            year: Some(2020),
            season: Some(1),
            episode: Some(2),
            language: "ar".into(),
        }
    }

    fn make_video_file(size: usize) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(&vec![0u8; size]).unwrap();
        f
    }

    #[tokio::test]
    async fn no_opensubtitles_creds_uses_subsource_only() {
        let mut server = mockito::Server::new_async().await;
        let _movies = server
            .mock("GET", "/movies/search")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"success":true,"data":[{"movieId":1}]}"#)
            .create_async()
            .await;
        let _subs = server
            .mock("GET", "/subtitles")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"success":true,"data":[{"subtitleId":42}]}"#)
            .create_async()
            .await;
        let mut zip_bytes = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_bytes));
            writer.start_file::<_, ()>("a.srt", zip::write::FileOptions::default()).unwrap();
            writer.write_all(b"hi").unwrap();
            writer.finish().unwrap();
        }
        let _download = server
            .mock("GET", "/subtitles/42/download")
            .with_status(200)
            .with_body(zip_bytes)
            .create_async()
            .await;

        let video = make_video_file(200_000);
        let result = find_subtitle_with_base_urls(
            &test_request(),
            video.path(),
            None,
            "subsource-key",
            "SubForge v0.1",
            "http://unused.invalid",
            &server.url(),
        )
        .await;

        match result {
            SubtitleSearchResult::Found { source, matched_via, .. } => {
                assert_eq!(source, "subsource");
                assert_eq!(matched_via, "title");
            }
            SubtitleSearchResult::NotFound => panic!("expected a match"),
        }
    }

    #[tokio::test]
    async fn both_sources_miss_falls_through_to_not_found() {
        let mut server = mockito::Server::new_async().await;
        let _movies = server
            .mock("GET", "/movies/search")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"success":true,"data":[]}"#)
            .create_async()
            .await;

        let video = make_video_file(200_000);
        let result = find_subtitle_with_base_urls(
            &test_request(),
            video.path(),
            None,
            "subsource-key",
            "SubForge v0.1",
            "http://unused.invalid",
            &server.url(),
        )
        .await;

        assert!(matches!(result, SubtitleSearchResult::NotFound));
    }

    #[tokio::test]
    async fn subsource_network_error_falls_through_to_not_found() {
        // Deliberately point at a port nothing listens on.
        let video = make_video_file(200_000);
        let result = find_subtitle_with_base_urls(
            &test_request(),
            video.path(),
            None,
            "subsource-key",
            "SubForge v0.1",
            "http://unused.invalid",
            "http://127.0.0.1:1",
        )
        .await;

        assert!(matches!(result, SubtitleSearchResult::NotFound));
    }
}
