use crate::SourceError;
use serde::Deserialize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const CHUNK_SIZE: u64 = 65536; // 64 KiB

/// Computes OpenSubtitles' "moviehash"/OSHash: file size plus a little-endian
/// u64 checksum (wrapping add) of the first and last 64KiB of the file. Files
/// between 64KiB and 128KiB have the two windows overlap — that's expected
/// and still produces a valid hash, per the algorithm's spec.
pub fn compute_hash(path: &Path) -> Result<(String, u64), SourceError> {
    let mut file = File::open(path)?;
    let file_size = file.metadata()?.len();
    if file_size < CHUNK_SIZE {
        return Err(SourceError::FileTooSmall);
    }

    let mut hash = file_size;
    hash = hash.wrapping_add(sum_chunk(&mut file, 0)?);
    hash = hash.wrapping_add(sum_chunk(&mut file, file_size - CHUNK_SIZE)?);

    Ok((format!("{hash:016x}"), file_size))
}

fn sum_chunk(file: &mut File, offset: u64) -> Result<u64, SourceError> {
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = [0u8; CHUNK_SIZE as usize];
    file.read_exact(&mut buf)?;
    let mut sum: u64 = 0;
    for eight_bytes in buf.chunks_exact(8) {
        sum = sum.wrapping_add(u64::from_le_bytes(eight_bytes.try_into().unwrap()));
    }
    Ok(sum)
}

pub const DEFAULT_BASE_URL: &str = "https://api.opensubtitles.com/api/v1";

#[derive(Debug, Clone, Deserialize)]
pub struct LoginResult {
    pub token: String,
}

#[derive(Debug, Clone)]
pub struct SubtitleMatch {
    pub file_id: i64,
    pub file_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    data: Vec<SearchResultEntry>,
}

#[derive(Debug, Deserialize)]
struct SearchResultEntry {
    attributes: SearchResultAttributes,
}

#[derive(Debug, Deserialize)]
struct SearchResultAttributes {
    files: Vec<SearchResultFile>,
}

#[derive(Debug, Deserialize)]
struct SearchResultFile {
    file_id: i64,
    file_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DownloadResponse {
    link: String,
}

pub async fn login(
    client: &reqwest::Client,
    base_url: &str,
    creds: &crate::OpenSubtitlesCredentials,
    user_agent: &str,
) -> Result<LoginResult, SourceError> {
    let resp = client
        .post(format!("{base_url}/login"))
        .header("Api-Key", &creds.api_key)
        .header("User-Agent", user_agent)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "username": creds.username, "password": creds.password }))
        .send()
        .await?;
    check_status(&resp)?;
    Ok(resp.json::<LoginResult>().await?)
}

pub async fn search_by_hash(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    user_agent: &str,
    moviehash: &str,
    language: &str,
) -> Result<Vec<SubtitleMatch>, SourceError> {
    let resp = client
        .get(format!("{base_url}/subtitles"))
        .header("Api-Key", api_key)
        .header("User-Agent", user_agent)
        .query(&[("moviehash", moviehash), ("languages", language)])
        .send()
        .await?;
    parse_search_response(resp).await
}

#[allow(clippy::too_many_arguments)]
pub async fn search_by_title(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    user_agent: &str,
    query: &str,
    season: Option<u32>,
    episode: Option<u32>,
    year: Option<u32>,
    language: &str,
) -> Result<Vec<SubtitleMatch>, SourceError> {
    let mut params: Vec<(&str, String)> = vec![
        ("query", query.to_string()),
        ("languages", language.to_string()),
    ];
    if let Some(s) = season {
        params.push(("season_number", s.to_string()));
    }
    if let Some(e) = episode {
        params.push(("episode_number", e.to_string()));
    }
    if let Some(y) = year {
        params.push(("year", y.to_string()));
    }

    let resp = client
        .get(format!("{base_url}/subtitles"))
        .header("Api-Key", api_key)
        .header("User-Agent", user_agent)
        .query(&params)
        .send()
        .await?;
    parse_search_response(resp).await
}

pub async fn download(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    jwt: &str,
    user_agent: &str,
    file_id: i64,
) -> Result<String, SourceError> {
    let resp = client
        .post(format!("{base_url}/download"))
        .header("Api-Key", api_key)
        .header("Authorization", format!("Bearer {jwt}"))
        .header("User-Agent", user_agent)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "file_id": file_id }))
        .send()
        .await?;
    check_status(&resp)?;
    let body: DownloadResponse = resp.json().await?;

    let srt_resp = client.get(&body.link).send().await?;
    check_status(&srt_resp)?;
    Ok(srt_resp.text().await?)
}

fn check_status(resp: &reqwest::Response) -> Result<(), SourceError> {
    match resp.status() {
        s if s.is_success() => Ok(()),
        reqwest::StatusCode::TOO_MANY_REQUESTS => Err(SourceError::RateLimited),
        reqwest::StatusCode::UNAUTHORIZED => {
            Err(SourceError::Unauthorized("OpenSubtitles rejected the request".into()))
        }
        other => Err(SourceError::Unauthorized(format!("HTTP {other}"))),
    }
}

async fn parse_search_response(resp: reqwest::Response) -> Result<Vec<SubtitleMatch>, SourceError> {
    check_status(&resp)?;
    let body: SearchResponse = resp.json().await?;
    Ok(body
        .data
        .into_iter()
        .flat_map(|entry| entry.attributes.files)
        .map(|f| SubtitleMatch {
            file_id: f.file_id,
            file_name: f.file_name,
        })
        .collect())
}

#[cfg(test)]
mod client_tests {
    use super::*;

    fn test_creds() -> crate::OpenSubtitlesCredentials {
        crate::OpenSubtitlesCredentials {
            api_key: "test-key".into(),
            username: "user".into(),
            password: "pass".into(),
        }
    }

    #[tokio::test]
    async fn login_returns_token_on_success() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("POST", "/login")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"token":"jwt123","base_url":"api.opensubtitles.com","user":{"allowed_downloads":20,"allowed_translations":0,"level":"Free","user_id":1,"ext_installed":false,"vip":false},"status":200}"#)
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let result = login(&client, &server.url(), &test_creds(), "SubForge v0.1").await.unwrap();
        assert_eq!(result.token, "jwt123");
    }

    #[tokio::test]
    async fn login_maps_401_to_unauthorized() {
        let mut server = mockito::Server::new_async().await;
        let _m = server.mock("POST", "/login").with_status(401).create_async().await;

        let client = reqwest::Client::new();
        let result = login(&client, &server.url(), &test_creds(), "SubForge v0.1").await;
        assert!(matches!(result, Err(SourceError::Unauthorized(_))));
    }

    #[tokio::test]
    async fn login_maps_429_to_rate_limited() {
        let mut server = mockito::Server::new_async().await;
        let _m = server.mock("POST", "/login").with_status(429).create_async().await;

        let client = reqwest::Client::new();
        let result = login(&client, &server.url(), &test_creds(), "SubForge v0.1").await;
        assert!(matches!(result, Err(SourceError::RateLimited)));
    }

    #[tokio::test]
    async fn search_by_hash_returns_matches() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/subtitles")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("moviehash".into(), "abc123".into()),
                mockito::Matcher::UrlEncoded("languages".into(), "ar".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"total_pages":1,"total_count":1,"per_page":50,"page":1,"data":[{"id":"1","type":"subtitle","attributes":{"language":"ar","files":[{"file_id":999,"file_name":"show.s01e01.srt"}]}}]}"#)
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let matches = search_by_hash(&client, &server.url(), "test-key", "SubForge v0.1", "abc123", "ar")
            .await
            .unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].file_id, 999);
    }

    #[tokio::test]
    async fn search_by_title_sends_season_and_episode() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/subtitles")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("query".into(), "Example Show".into()),
                mockito::Matcher::UrlEncoded("season_number".into(), "1".into()),
                mockito::Matcher::UrlEncoded("episode_number".into(), "2".into()),
                mockito::Matcher::UrlEncoded("languages".into(), "ar".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"total_pages":1,"total_count":0,"per_page":50,"page":1,"data":[]}"#)
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let matches = search_by_title(
            &client,
            &server.url(),
            "test-key",
            "SubForge v0.1",
            "Example Show",
            Some(1),
            Some(2),
            None,
            "ar",
        )
        .await
        .unwrap();
        assert!(matches.is_empty());
    }

    #[tokio::test]
    async fn download_fetches_the_temporary_link_body() {
        let mut server = mockito::Server::new_async().await;
        let link_path = "/subfile/example.srt";
        let _download_mock = server
            .mock("POST", "/download")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{"link":"{}{}","file_name":"example.srt","requests":1,"remaining":19,"message":"ok","reset_time":"1h","reset_time_utc":"2026-01-01T00:00:00Z"}}"#,
                server.url(),
                link_path
            ))
            .create_async()
            .await;
        let _srt_mock = server
            .mock("GET", link_path)
            .with_status(200)
            .with_body("1\n00:00:01,000 --> 00:00:02,000\nHello\n")
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let text = download(&client, &server.url(), "test-key", "jwt123", "SubForge v0.1", 999)
            .await
            .unwrap();
        assert!(text.contains("Hello"));
    }
}

#[cfg(test)]
mod hash_tests {
    use super::*;
    use std::io::Write;

    fn write_temp_file(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(bytes).unwrap();
        f
    }

    #[test]
    fn hashes_a_file_of_exactly_two_chunks() {
        // 128 KiB of zero bytes: head chunk sum = 0, tail chunk sum = 0,
        // so hash == file size, formatted as 16-digit zero-padded hex.
        let bytes = vec![0u8; 131072];
        let f = write_temp_file(&bytes);
        let (hash, size) = compute_hash(f.path()).unwrap();
        assert_eq!(size, 131072);
        assert_eq!(hash, format!("{:016x}", 131072u64));
    }

    #[test]
    fn hashes_a_file_smaller_than_128kib_with_overlapping_regions() {
        // 100 KiB: head and tail 64KiB windows overlap, but the hash must
        // still be well-defined (both windows are valid byte ranges).
        let bytes = vec![1u8; 100 * 1024];
        let f = write_temp_file(&bytes);
        let result = compute_hash(f.path());
        assert!(result.is_ok());
    }

    #[test]
    fn rejects_files_smaller_than_64kib() {
        let bytes = vec![0u8; 1000];
        let f = write_temp_file(&bytes);
        let result = compute_hash(f.path());
        assert!(matches!(result, Err(crate::SourceError::FileTooSmall)));
    }
}
