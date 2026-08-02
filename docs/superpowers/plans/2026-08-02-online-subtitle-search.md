# Online Subtitle Search Before Auto-Translation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Before SubForge's auto-translate pipeline runs, search Subsource (always) and OpenSubtitles.com (if the user configured their own credentials) for an existing subtitle in the target language; download and use it instead of translating when found, otherwise fall through to the existing translate flow unchanged — on both the Tauri desktop app and the Axum server backend.

**Architecture:** A new shared crate `subforge-subtitle-sources` (same pattern as the existing `subforge-metadata` crate) holds the Subsource client, OpenSubtitles client, and fallback orchestration. Both `src-tauri` and `server` get thin command/route wrappers over it. The Tauri app stores OpenSubtitles credentials in the OS keychain via the `keyring` crate; the server stores them in its existing plaintext `config.json` (with env var overrides), since a headless process has no keychain. Frontend changes are written once (Pinia store + Vue components) and work against both backends through the existing `apiInvoke` abstraction in `src/api/client.ts`.

**Tech Stack:** Rust (reqwest, keyring v3, zip, mockito for tests), Vue 3 + Pinia + TypeScript, Tauri v2, Axum.

## Global Constraints

- Subsource: base URL `https://api.subsource.net/api/v1`, header `X-API-Key`, rate limits 60/min·1800/hr·7200/day, 429 on exceed.
- OpenSubtitles: base URL `https://api.opensubtitles.com/api/v1`, headers `Api-Key` + `User-Agent` always, `Authorization: Bearer <JWT>` once logged in (JWT from `POST /login`, valid 24h, login itself rate-limited to 1/sec·10/min·30/hr — never loop-retry a login).
- OpenSubtitles moviehash = `filesize + sum_u64_le(first 64KiB) + sum_u64_le(last 64KiB)`, wrapping u64 arithmetic, formatted as 16-char zero-padded lowercase hex. Files under 64KiB (65536 bytes) can't be hashed.
- No source call may ever panic, block the UI/request thread, or propagate an error that fails the translate pipeline — every failure (network, 429, missing/invalid credentials) must degrade to "not found," falling through to auto-translate.
- SubForge stays fully open source: **no OpenSubtitles credential is embedded** in the app — every user supplies their own API key + username + password. Subsource **does** use one app-embedded API key (existing product decision, unrelated to this constraint).
- Every new Tauri command must have a matching entry in `src/api/client.ts`'s `cmdToEndpoint` map and a matching Axum route in `server/`, so the same frontend code works against both backends.
- Reference spec: `docs/superpowers/specs/2026-08-02-online-subtitle-search-design.md`.

---

### Task 1: Shared crate skeleton — `subforge-subtitle-sources`

**Files:**
- Create: `subforge/crates/subforge-subtitle-sources/Cargo.toml`
- Create: `subforge/crates/subforge-subtitle-sources/src/lib.rs`
- Modify: `subforge/src-tauri/Cargo.toml`
- Modify: `subforge/server/Cargo.toml`

**Interfaces:**
- Produces: `subforge_subtitle_sources::{SourceError, SearchRequest, OpenSubtitlesCredentials, SubtitleSearchResult}` — the shared types every later task builds on.

- [ ] **Step 1: Create the crate directory and Cargo.toml**

```toml
[package]
name = "subforge-subtitle-sources"
version = "0.1.0"
edition = "2021"

[dependencies]
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
zip = "2"
subforge-metadata = { path = "../subforge-metadata" }

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt"] }
mockito = "1"
```

- [ ] **Step 2: Write `src/lib.rs` with the shared types (no logic yet)**

```rust
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
```

- [ ] **Step 3: Add empty placeholder modules so the crate compiles**

Create `subforge/crates/subforge-subtitle-sources/src/opensubtitles.rs`:

```rust
// Filled in by Task 3.
```

Create `subforge/crates/subforge-subtitle-sources/src/subsource.rs`:

```rust
// Filled in by Task 4.
```

- [ ] **Step 4: Verify the new crate builds standalone**

Run: `cargo check` (from `subforge/crates/subforge-subtitle-sources/`)
Expected: compiles with no errors (warnings about unused code in the empty modules are fine — they'll be gone once Tasks 3–4 fill them in).

- [ ] **Step 5: Add the crate as a path dependency to both binaries**

In `subforge/src-tauri/Cargo.toml`, add after the existing `subforge-metadata` line:

```toml
subforge-subtitle-sources = { path = "../crates/subforge-subtitle-sources" }
```

In `subforge/server/Cargo.toml`, add after the existing `subforge-metadata` line:

```toml
subforge-subtitle-sources = { path = "../crates/subforge-subtitle-sources" }
```

- [ ] **Step 6: Verify both binaries still build**

Run: `cargo check` (from `subforge/src-tauri/`)
Run: `cargo check` (from `subforge/server/`)
Expected: both compile with no errors.

- [ ] **Step 7: Commit**

```bash
git add subforge/crates/subforge-subtitle-sources subforge/src-tauri/Cargo.toml subforge/src-tauri/Cargo.lock subforge/server/Cargo.toml subforge/server/Cargo.lock
git commit -m "Add subforge-subtitle-sources crate skeleton"
```

---

### Task 2: OpenSubtitles moviehash algorithm

**Files:**
- Modify: `subforge/crates/subforge-subtitle-sources/src/opensubtitles.rs`

**Interfaces:**
- Consumes: `crate::SourceError` (Task 1).
- Produces: `pub fn compute_hash(path: &std::path::Path) -> Result<(String, u64), SourceError>` — returns `(hex_hash, file_size_bytes)`. Later tasks (search-by-hash) use this.

- [ ] **Step 1: Write the failing tests**

```rust
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
        assert!(matches!(result, Err(SourceError::FileTooSmall)));
    }
}
```

Add `tempfile = "3"` to the `[dev-dependencies]` section of
`subforge/crates/subforge-subtitle-sources/Cargo.toml`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test hash_tests --package subforge-subtitle-sources`
Expected: FAIL with "cannot find function `compute_hash`"

- [ ] **Step 3: Implement the hash algorithm**

Add to `subforge/crates/subforge-subtitle-sources/src/opensubtitles.rs` (above the test module):

```rust
use crate::SourceError;
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test hash_tests --package subforge-subtitle-sources`
Expected: PASS (3 tests)

- [ ] **Step 5: Commit**

```bash
git add subforge/crates/subforge-subtitle-sources/src/opensubtitles.rs subforge/crates/subforge-subtitle-sources/Cargo.toml subforge/crates/subforge-subtitle-sources/Cargo.lock
git commit -m "Implement OpenSubtitles moviehash algorithm"
```

---

### Task 3: OpenSubtitles REST client (login, search, download)

**Files:**
- Modify: `subforge/crates/subforge-subtitle-sources/src/opensubtitles.rs`

**Interfaces:**
- Consumes: `crate::{SourceError, OpenSubtitlesCredentials}` (Task 1), `compute_hash` (Task 2).
- Produces:
  - `pub async fn login(client: &reqwest::Client, base_url: &str, creds: &OpenSubtitlesCredentials, user_agent: &str) -> Result<LoginResult, SourceError>` where `LoginResult { token: String }`.
  - `pub async fn search_by_hash(client: &reqwest::Client, base_url: &str, api_key: &str, user_agent: &str, moviehash: &str, language: &str) -> Result<Vec<SubtitleMatch>, SourceError>`.
  - `pub async fn search_by_title(client: &reqwest::Client, base_url: &str, api_key: &str, user_agent: &str, query: &str, season: Option<u32>, episode: Option<u32>, year: Option<u32>, language: &str) -> Result<Vec<SubtitleMatch>, SourceError>`.
  - `pub async fn download(client: &reqwest::Client, base_url: &str, api_key: &str, jwt: &str, user_agent: &str, file_id: i64) -> Result<String, SourceError>` — returns the subtitle text directly (fetches the temporary link internally).
  - `pub struct SubtitleMatch { pub file_id: i64, pub file_name: Option<String> }` — later tasks pick the first match.

- [ ] **Step 1: Write the failing tests using a mock HTTP server**

Append to `subforge/crates/subforge-subtitle-sources/src/opensubtitles.rs`:

```rust
#[cfg(test)]
mod client_tests {
    use super::*;

    fn test_creds() -> OpenSubtitlesCredentials {
        OpenSubtitlesCredentials {
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test client_tests --package subforge-subtitle-sources`
Expected: FAIL with "cannot find function `login`" (and similarly for the others)

- [ ] **Step 3: Implement the client functions**

Insert above the `#[cfg(test)] mod hash_tests` block in
`subforge/crates/subforge-subtitle-sources/src/opensubtitles.rs`:

```rust
use serde::{Deserialize, Serialize};

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
    creds: &OpenSubtitlesCredentials,
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
```

Note this file now has three consumers of `Read`/`Seek`/`File`/`Path` imports from Task 2's hash code plus these new imports — keep both `use` blocks (the hash-specific ones stay where Task 2 put them at the top of the file; add `use serde::{Deserialize, Serialize}` alongside them).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package subforge-subtitle-sources`
Expected: PASS (all hash + client tests)

- [ ] **Step 5: Commit**

```bash
git add subforge/crates/subforge-subtitle-sources/src/opensubtitles.rs subforge/crates/subforge-subtitle-sources/Cargo.toml subforge/crates/subforge-subtitle-sources/Cargo.lock
git commit -m "Implement OpenSubtitles REST client (login, search, download)"
```

---

### Task 4: Subsource REST client

**Files:**
- Modify: `subforge/crates/subforge-subtitle-sources/src/subsource.rs`

**Interfaces:**
- Consumes: `crate::SourceError` (Task 1), `subforge_metadata::languages::english_name` (existing).
- Produces:
  - `pub async fn search_movies(client: &reqwest::Client, base_url: &str, api_key: &str, query: &str, year: Option<u32>, season: Option<u32>) -> Result<Vec<MovieMatch>, SourceError>` where `MovieMatch { movie_id: i64 }`.
  - `pub async fn search_subtitles(client: &reqwest::Client, base_url: &str, api_key: &str, movie_id: i64, language_code: &str) -> Result<Vec<SubtitleEntry>, SourceError>` where `SubtitleEntry { subtitle_id: i64 }`.
  - `pub async fn download(client: &reqwest::Client, base_url: &str, api_key: &str, subtitle_id: i64) -> Result<String, SourceError>` — downloads the zip and returns the extracted `.srt` text.

- [ ] **Step 1: Write the failing tests**

```rust
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
            .with_status(429)
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let result = search_movies(&client, &server.url(), "test-key", "x", None, None).await;
        assert!(matches!(result, Err(SourceError::RateLimited)));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package subforge-subtitle-sources subsource`
Expected: FAIL with "cannot find function `search_movies`"

- [ ] **Step 3: Implement the client**

Write `subforge/crates/subforge-subtitle-sources/src/subsource.rs` (above the test module):

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package subforge-subtitle-sources subsource`
Expected: PASS (4 tests)

- [ ] **Step 5: Commit**

```bash
git add subforge/crates/subforge-subtitle-sources/src/subsource.rs subforge/crates/subforge-subtitle-sources/Cargo.toml subforge/crates/subforge-subtitle-sources/Cargo.lock
git commit -m "Implement Subsource REST client"
```

---

### Task 5: Fallback orchestration

**Files:**
- Modify: `subforge/crates/subforge-subtitle-sources/src/lib.rs`

**Interfaces:**
- Consumes: everything from Tasks 1–4 (`opensubtitles::*`, `subsource::*`, `compute_hash`).
- Produces: `pub async fn find_subtitle(request: &SearchRequest, video_path: &std::path::Path, opensubtitles_creds: Option<&OpenSubtitlesCredentials>, subsource_api_key: &str, user_agent: &str) -> SubtitleSearchResult` — the single entry point Tauri commands and Axum routes call (Tasks 7–8).

- [ ] **Step 1: Write the failing tests**

Append to `subforge/crates/subforge-subtitle-sources/src/lib.rs`:

```rust
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
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"success":true,"data":[{"movieId":1}]}"#)
            .create_async()
            .await;
        let _subs = server
            .mock("GET", "/subtitles")
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
```

Add `tempfile = "3"` to `[dev-dependencies]` in
`subforge/crates/subforge-subtitle-sources/Cargo.toml` if not already added
by Task 2 (it was — no change needed if so).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --package subforge-subtitle-sources orchestration`
Expected: FAIL with "cannot find function `find_subtitle_with_base_urls`"

- [ ] **Step 3: Implement the orchestration**

Append to `subforge/crates/subforge-subtitle-sources/src/lib.rs` (above the
`#[cfg(test)] mod orchestration_tests` block):

```rust
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

    let hash_match = match compute_hash(video_path) {
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
```

Note: `SubtitleSearchResult::Found.downloaded_path` temporarily holds the raw
subtitle **text**, not a filesystem path, at this layer — Task 7/8's command
wrappers write it to disk (using the existing `{video_stem}.{lang}.{ext}`
convention) and replace it with the real path before returning to the
frontend. This keeps the shared crate free of any Tauri/Axum-specific
filesystem-naming logic.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --package subforge-subtitle-sources`
Expected: PASS (all tests across the crate)

- [ ] **Step 5: Commit**

```bash
git add subforge/crates/subforge-subtitle-sources/src/lib.rs subforge/crates/subforge-subtitle-sources/Cargo.toml subforge/crates/subforge-subtitle-sources/Cargo.lock
git commit -m "Implement online-subtitle fallback orchestration"
```

---

### Task 6: Tauri credential storage (OS keychain)

**Files:**
- Create: `subforge/src-tauri/src/credentials.rs`
- Modify: `subforge/src-tauri/src/lib.rs`
- Modify: `subforge/src-tauri/Cargo.toml`

**Interfaces:**
- Consumes: `subforge_subtitle_sources::OpenSubtitlesCredentials` (Task 1).
- Produces: `pub trait CredentialStore: Send + Sync { fn get(&self) -> Result<Option<OpenSubtitlesCredentials>, String>; fn set(&self, creds: &OpenSubtitlesCredentials) -> Result<(), String>; fn clear(&self) -> Result<(), String>; }`, `pub struct KeyringCredentialStore;` (implements it), registered as `tauri::State<Arc<dyn CredentialStore>>` for Task 7's commands.

- [ ] **Step 1: Add the `keyring` dependency**

In `subforge/src-tauri/Cargo.toml`, add to `[dependencies]`:

```toml
keyring = { version = "3", features = ["apple-native", "windows-native", "sync-secret-service"] }
```

- [ ] **Step 2: Write the failing tests (using an in-memory mock, not the real OS keychain)**

Create `subforge/src-tauri/src/credentials.rs`:

```rust
use subforge_subtitle_sources::OpenSubtitlesCredentials;

pub trait CredentialStore: Send + Sync {
    fn get(&self) -> Result<Option<OpenSubtitlesCredentials>, String>;
    fn set(&self, creds: &OpenSubtitlesCredentials) -> Result<(), String>;
    fn clear(&self) -> Result<(), String>;
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
            Self { stored: Mutex::new(None) }
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
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --package subforge_lib credentials` (from `subforge/src-tauri/`)
Expected: FAIL — `CredentialStore` trait exists but nothing exercises it yet is fine; actually since the trait has no implementers required for these mock-only tests, this step instead confirms the module doesn't compile yet because `credentials` isn't wired into `lib.rs`. Expected: FAIL with "unresolved module `credentials`" until Step 5.

- [ ] **Step 4: Implement the real keyring-backed store**

Add to `subforge/src-tauri/src/credentials.rs`, above the `#[cfg(test)]` block:

```rust
use keyring::Entry;

const SERVICE: &str = "subforge";
const ACCOUNT: &str = "opensubtitles";

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
        match entry.delete_password() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}
```

- [ ] **Step 5: Wire the module into `lib.rs` and manage it as Tauri state**

In `subforge/src-tauri/src/lib.rs`, add `pub mod credentials;` next to the
existing `pub mod commands;` etc., and in `run()`:

```rust
use credentials::{CredentialStore, KeyringCredentialStore};
use std::sync::Arc;
```

(add alongside the existing `use` lines), then before `tauri::Builder::default()`:

```rust
let credential_store: Arc<dyn CredentialStore> = Arc::new(KeyringCredentialStore);
```

and add `.manage(credential_store)` to the builder chain (alongside the
existing `.manage(job_queue)` / `.manage(job_controls)` calls).

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo check` (from `subforge/src-tauri/`) — confirms it compiles with the new dependency and module wiring.
Run: `cargo test credentials` (from `subforge/src-tauri/`)
Expected: PASS (3 tests, all against `MockCredentialStore` — never the real OS keychain)

- [ ] **Step 7: Commit**

```bash
git add subforge/src-tauri/src/credentials.rs subforge/src-tauri/src/lib.rs subforge/src-tauri/Cargo.toml subforge/src-tauri/Cargo.lock
git commit -m "Add OS-keychain-backed OpenSubtitles credential storage"
```

---

### Task 7: Tauri commands for subtitle search + credentials

**Files:**
- Create: `subforge/src-tauri/src/commands/subtitle_sources.rs`
- Modify: `subforge/src-tauri/src/commands/mod.rs`
- Modify: `subforge/src-tauri/src/commands/providers.rs`
- Modify: `subforge/src-tauri/src/lib.rs`
- Modify: `subforge/src-tauri/src/config.rs`

**Interfaces:**
- Consumes: `subforge_subtitle_sources::{find_subtitle, SearchRequest, SubtitleSearchResult}` (Task 5), `credentials::CredentialStore` (Task 6), `crate::config::AppConfig` (existing — extended here with the Subsource key + toggle).
- Produces Tauri commands: `search_online_subtitle`, `set_opensubtitles_credentials`, `clear_opensubtitles_credentials`, `get_opensubtitles_status`, `check_subsource_status`. These are the exact names Task 11's frontend store calls.

- [ ] **Step 1: Add the app-embedded Subsource key and the search-toggle field to `AppConfig`**

In `subforge/src-tauri/src/config.rs`, add two fields to the `AppConfig`
struct (after `libretranslate_base_url`):

```rust
    #[serde(default = "default_search_online_before_translate")]
    pub search_online_before_translate: bool,
```

Add the default function near the other `default_*` functions at the top of
the file:

```rust
fn default_search_online_before_translate() -> bool {
    true
}
```

Add the corresponding field to the `Default for AppConfig` impl:

```rust
            search_online_before_translate: default_search_online_before_translate(),
```

The Subsource app-level API key is **not** stored in `AppConfig` (it's
compiled in, not user-configurable) — see Step 2.

- [ ] **Step 2: Add a build-time-embedded Subsource key constant**

Create the constant at the top of the new
`subforge/src-tauri/src/commands/subtitle_sources.rs` (Step 4 below):

```rust
const SUBSOURCE_API_KEY: &str = env!(
    "SUBFORGE_SUBSOURCE_API_KEY",
    "Set SUBFORGE_SUBSOURCE_API_KEY at build time (see README) to the app-level Subsource API key."
);
```

This makes the build fail loudly if the key isn't provided, rather than
silently shipping a broken Subsource integration. Document in
`subforge/README.md`'s Prerequisites section (add one line): "Building
requires `SUBFORGE_SUBSOURCE_API_KEY` set in the environment (a Subsource
API key created from your Subsource profile) — see
`docs/superpowers/specs/2026-08-02-online-subtitle-search-design.md`."

- [ ] **Step 3: Write the request/response types and the search command**

Create `subforge/src-tauri/src/commands/subtitle_sources.rs`:

```rust
use crate::credentials::CredentialStore;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use subforge_subtitle_sources::{find_subtitle, OpenSubtitlesCredentials, SearchRequest, SubtitleSearchResult};
use tauri::State;

const SUBSOURCE_API_KEY: &str = env!(
    "SUBFORGE_SUBSOURCE_API_KEY",
    "Set SUBFORGE_SUBSOURCE_API_KEY at build time (see README) to the app-level Subsource API key."
);
const USER_AGENT: &str = concat!("SubForge v", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineSubtitleSearchRequest {
    pub video_path: String,
    pub title: String,
    pub year: Option<u32>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OnlineSubtitleSearchResponse {
    Found { subtitle_path: String, source: String, matched_via: String },
    NotFound,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn search_online_subtitle(
    credential_store: State<'_, Arc<dyn CredentialStore>>,
    request: OnlineSubtitleSearchRequest,
) -> Result<OnlineSubtitleSearchResponse, String> {
    let opensubtitles_creds: Option<OpenSubtitlesCredentials> =
        credential_store.get().unwrap_or(None);

    let search_request = SearchRequest {
        title: request.title,
        year: request.year,
        season: request.season,
        episode: request.episode,
        language: request.language.clone(),
    };

    let video_path = PathBuf::from(&request.video_path);
    let result = find_subtitle(
        &search_request,
        &video_path,
        opensubtitles_creds.as_ref(),
        SUBSOURCE_API_KEY,
        USER_AGENT,
    )
    .await;

    match result {
        SubtitleSearchResult::Found { downloaded_path: text, source, matched_via } => {
            let subtitle_path = write_subtitle_next_to_video(&video_path, &request.language, &text)
                .map_err(|e| e.to_string())?;
            Ok(OnlineSubtitleSearchResponse::Found {
                subtitle_path,
                source,
                matched_via,
            })
        }
        SubtitleSearchResult::NotFound => Ok(OnlineSubtitleSearchResponse::NotFound),
    }
}

/// Writes downloaded subtitle text next to the video using the same
/// `{video_stem}.{lang}.{ext}` convention `extract_all_subtitles` already
/// writes and `detect_external_subs` (`commands/batch.rs`) already reads
/// back, so a subtitle found online is indistinguishable from one extracted
/// locally on a later scan.
fn write_subtitle_next_to_video(video_path: &Path, lang: &str, text: &str) -> std::io::Result<String> {
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("subtitle");
    let dir = video_path.parent().unwrap_or_else(|| Path::new("."));
    let out_path = dir.join(format!("{stem}.{lang}.srt"));
    std::fs::write(&out_path, text)?;
    Ok(out_path.to_string_lossy().to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn set_opensubtitles_credentials(
    credential_store: State<'_, Arc<dyn CredentialStore>>,
    credentials: OpenSubtitlesCredentials,
) -> Result<(), String> {
    credential_store.set(&credentials)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn clear_opensubtitles_credentials(
    credential_store: State<'_, Arc<dyn CredentialStore>>,
) -> Result<(), String> {
    credential_store.clear()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenSubtitlesStatus {
    pub configured: bool,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_opensubtitles_status(
    credential_store: State<'_, Arc<dyn CredentialStore>>,
) -> Result<OpenSubtitlesStatus, String> {
    let configured = credential_store.get()?.is_some();
    Ok(OpenSubtitlesStatus { configured })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsourceStatus {
    pub reachable: bool,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn check_subsource_status() -> Result<SubsourceStatus, String> {
    let client = reqwest::Client::new();
    let reachable = client
        .get(format!("{}/movies/search", subforge_subtitle_sources::subsource::DEFAULT_BASE_URL))
        .header("X-API-Key", SUBSOURCE_API_KEY)
        .query(&[("searchType", "text"), ("q", "ping")])
        .send()
        .await
        .map(|resp| resp.status().is_success() || resp.status().as_u16() == 429)
        .unwrap_or(false);
    Ok(SubsourceStatus { reachable })
}
```

The `search_online_before_translate` toggle from Step 1 isn't referenced in
this file — it's a plain `AppConfig` field read/written through the
existing `load_config`/`save_config` commands (`commands/subtitle.rs`),
which Task 11's frontend store reads directly off `appStore.config`. No
new command is needed for it.

- [ ] **Step 4: Register the module and commands**

In `subforge/src-tauri/src/commands/mod.rs`, add:

```rust
pub mod subtitle_sources;
```

In `subforge/src-tauri/src/lib.rs`, add `subtitle_sources as subtitle_sources_cmds` to
the existing `use commands::{...}` import list, and add these lines inside
`tauri::generate_handler![...]` (alongside the existing `rename_cmds::*`
entries):

```rust
            subtitle_sources_cmds::search_online_subtitle,
            subtitle_sources_cmds::set_opensubtitles_credentials,
            subtitle_sources_cmds::clear_opensubtitles_credentials,
            subtitle_sources_cmds::get_opensubtitles_status,
            subtitle_sources_cmds::check_subsource_status,
```

- [ ] **Step 5: Add the `"opensubtitles"` arm to `test_provider_config`**

In `subforge/src-tauri/src/commands/providers.rs`, the `"opensubtitles"`
arm needs the credential store, which means the function's signature must
grow a `State` parameter. Replace the existing signature line:

```rust
#[tauri::command]
pub async fn test_provider_config(provider: String) -> Result<(), String> {
```

with:

```rust
#[tauri::command]
pub async fn test_provider_config(
    credential_store: tauri::State<'_, std::sync::Arc<dyn crate::credentials::CredentialStore>>,
    provider: String,
) -> Result<(), String> {
```

Then add this arm to the existing `match provider.as_str() { ... }` block,
before the `other => Err(...)` catch-all:

```rust
        "opensubtitles" => {
            let creds = credential_store
                .get()?
                .ok_or_else(|| "No OpenSubtitles credentials configured".to_string())?;
            let client = reqwest::Client::new();
            subforge_subtitle_sources::opensubtitles::login(
                &client,
                subforge_subtitle_sources::opensubtitles::DEFAULT_BASE_URL,
                &creds,
                &format!("SubForge v{}", env!("CARGO_PKG_VERSION")),
            )
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
        }
```

Every existing call site of `test_provider_config` in this file already
returns `Result<(), String>`, so this signature change is additive — Tauri
injects `State` automatically, and the frontend call in Task 12
(`apiInvoke("test_provider_config", { provider })`) doesn't need to change
since Tauri state isn't part of the JS-visible argument list.

- [ ] **Step 6: Verify it builds**

Run: `SUBFORGE_SUBSOURCE_API_KEY=test-build-key cargo check` (from `subforge/src-tauri/`)
Expected: compiles with no errors. (A real key isn't needed for `cargo
check`/`cargo build` — any non-empty string satisfies the `env!` macro at
compile time; using a placeholder here is fine for this verification step
specifically because it's not being used to make live requests.)

- [ ] **Step 7: Commit**

```bash
git add subforge/src-tauri/src/commands/subtitle_sources.rs subforge/src-tauri/src/commands/mod.rs subforge/src-tauri/src/commands/providers.rs subforge/src-tauri/src/lib.rs subforge/src-tauri/src/config.rs subforge/README.md
git commit -m "Add Tauri commands for online subtitle search and OpenSubtitles credentials"
```

---

### Task 8: Server (Axum) routes for subtitle search + credentials

**Files:**
- Create: `subforge/server/src/routes/subtitle_sources.rs`
- Modify: `subforge/server/src/routes/mod.rs`
- Modify: `subforge/server/src/routes/providers.rs` (create if it doesn't exist — check first)
- Modify: `subforge/server/src/main.rs`
- Modify: `subforge/server/src/config.rs`

**Interfaces:**
- Consumes: `subforge_subtitle_sources::{find_subtitle, SearchRequest, SubtitleSearchResult}` (Task 5).
- Produces Axum routes matching Task 7's Tauri commands 1:1 in request/response JSON shape, registered at `/api/subtitle-sources/*`.

- [ ] **Step 1: Confirm `server/src/routes/providers.rs`'s current contents**

This file exists today with a `test_provider_config(Json(req):
Json<TestProviderRequest>) -> Result<StatusCode, (StatusCode, String)>`
handler containing a `match req.provider.as_str() { "tmdb" => ..., "tvmaze"
=> ..., ..., other => Err(format!("Unknown provider: {other}")) }` block —
Step 6 below adds an `"opensubtitles"` arm to that same `match`. Run `cat
subforge/server/src/routes/providers.rs` to confirm it still matches this
shape before editing (in case it changed since this plan was written).

- [ ] **Step 2: Add the Subsource key/toggle to the server's `AppConfig`**

In `subforge/server/src/config.rs`, mirror Task 7 Step 1 exactly (same field
name, same default function) — add:

```rust
    #[serde(default = "default_search_online_before_translate")]
    pub search_online_before_translate: bool,
```

with the same `default_search_online_before_translate() -> bool { true }`
function and `Default` impl field, plus these three new fields for the
per-user OpenSubtitles credentials (server mode has no keychain — see the
design spec's credential-storage-fork section):

```rust
    pub opensubtitles_api_key: Option<String>,
    pub opensubtitles_username: Option<String>,
    pub opensubtitles_password: Option<String>,
```

with `None` defaults in the `Default` impl (same pattern as
`tmdb_api_key: None` there today).

- [ ] **Step 3: Add env var overrides, read once at startup**

In `subforge/server/src/main.rs`, inside `main()`, before `let state = AppState { ... }`, add:

```rust
    let mut config = crate::config::AppConfig::load();
    if let Ok(key) = std::env::var("OPENSUBTITLES_API_KEY") {
        config.opensubtitles_api_key = Some(key);
    }
    if let Ok(username) = std::env::var("OPENSUBTITLES_USERNAME") {
        config.opensubtitles_username = Some(username);
    }
    if let Ok(password) = std::env::var("OPENSUBTITLES_PASSWORD") {
        config.opensubtitles_password = Some(password);
    }
    let _ = config.save();
```

This persists the env-var-sourced values into `config.json` on startup so
subsequent reads (via the existing `/config` route) see them too, matching
how the rest of `AppConfig` is read/written today (there's no separate
in-memory config layer — every route calls `AppConfig::load()` fresh).

- [ ] **Step 4: Write the route handlers**

Create `subforge/server/src/routes/subtitle_sources.rs`:

```rust
use crate::config::AppConfig;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use subforge_subtitle_sources::{find_subtitle, OpenSubtitlesCredentials, SearchRequest, SubtitleSearchResult};

const SUBSOURCE_API_KEY: &str = env!(
    "SUBFORGE_SUBSOURCE_API_KEY",
    "Set SUBFORGE_SUBSOURCE_API_KEY at build time (see README) to the app-level Subsource API key."
);
const USER_AGENT: &str = concat!("SubForge v", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineSubtitleSearchRequest {
    pub video_path: String,
    pub title: String,
    pub year: Option<u32>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OnlineSubtitleSearchResponse {
    Found { subtitle_path: String, source: String, matched_via: String },
    NotFound,
}

pub async fn search_online_subtitle(
    Json(request): Json<OnlineSubtitleSearchRequest>,
) -> Json<OnlineSubtitleSearchResponse> {
    let config = AppConfig::load();
    let opensubtitles_creds = match (
        config.opensubtitles_api_key,
        config.opensubtitles_username,
        config.opensubtitles_password,
    ) {
        (Some(api_key), Some(username), Some(password)) => {
            Some(OpenSubtitlesCredentials { api_key, username, password })
        }
        _ => None,
    };

    let search_request = SearchRequest {
        title: request.title,
        year: request.year,
        season: request.season,
        episode: request.episode,
        language: request.language.clone(),
    };

    let video_path = PathBuf::from(&request.video_path);
    let result = find_subtitle(
        &search_request,
        &video_path,
        opensubtitles_creds.as_ref(),
        SUBSOURCE_API_KEY,
        USER_AGENT,
    )
    .await;

    match result {
        SubtitleSearchResult::Found { downloaded_path: text, source, matched_via } => {
            match write_subtitle_next_to_video(&video_path, &request.language, &text) {
                Ok(subtitle_path) => {
                    Json(OnlineSubtitleSearchResponse::Found { subtitle_path, source, matched_via })
                }
                Err(_) => Json(OnlineSubtitleSearchResponse::NotFound),
            }
        }
        SubtitleSearchResult::NotFound => Json(OnlineSubtitleSearchResponse::NotFound),
    }
}

fn write_subtitle_next_to_video(video_path: &Path, lang: &str, text: &str) -> std::io::Result<String> {
    let stem = video_path.file_stem().and_then(|s| s.to_str()).unwrap_or("subtitle");
    let dir = video_path.parent().unwrap_or_else(|| Path::new("."));
    let out_path = dir.join(format!("{stem}.{lang}.srt"));
    std::fs::write(&out_path, text)?;
    Ok(out_path.to_string_lossy().to_string())
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetOpenSubtitlesCredentialsRequest {
    pub credentials: OpenSubtitlesCredentials,
}

pub async fn set_opensubtitles_credentials(
    Json(body): Json<SetOpenSubtitlesCredentialsRequest>,
) -> Json<()> {
    let mut config = AppConfig::load();
    config.opensubtitles_api_key = Some(body.credentials.api_key);
    config.opensubtitles_username = Some(body.credentials.username);
    config.opensubtitles_password = Some(body.credentials.password);
    let _ = config.save();
    Json(())
}

pub async fn clear_opensubtitles_credentials() -> Json<()> {
    let mut config = AppConfig::load();
    config.opensubtitles_api_key = None;
    config.opensubtitles_username = None;
    config.opensubtitles_password = None;
    let _ = config.save();
    Json(())
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenSubtitlesStatus {
    pub configured: bool,
}

pub async fn get_opensubtitles_status() -> Json<OpenSubtitlesStatus> {
    let config = AppConfig::load();
    let configured = config.opensubtitles_api_key.is_some()
        && config.opensubtitles_username.is_some()
        && config.opensubtitles_password.is_some();
    Json(OpenSubtitlesStatus { configured })
}

#[derive(Debug, Clone, Serialize)]
pub struct SubsourceStatus {
    pub reachable: bool,
}

pub async fn check_subsource_status() -> Json<SubsourceStatus> {
    let client = reqwest::Client::new();
    let reachable = client
        .get(format!("{}/movies/search", subforge_subtitle_sources::subsource::DEFAULT_BASE_URL))
        .header("X-API-Key", SUBSOURCE_API_KEY)
        .query(&[("searchType", "text"), ("q", "ping")])
        .send()
        .await
        .map(|resp| resp.status().is_success() || resp.status().as_u16() == 429)
        .unwrap_or(false);
    Json(SubsourceStatus { reachable })
}
```

- [ ] **Step 5: Register the module and routes**

In `subforge/server/src/routes/mod.rs`, add:

```rust
pub mod subtitle_sources;
```

In `subforge/server/src/main.rs`'s `api_routes()`, add a new group after the
existing `// ── Rename ...` block:

```rust
        // ── Online subtitle sources ─────────────────────────────────────────
        .route(
            "/subtitle-sources/search",
            post(routes::subtitle_sources::search_online_subtitle),
        )
        .route(
            "/subtitle-sources/opensubtitles/credentials",
            post(routes::subtitle_sources::set_opensubtitles_credentials),
        )
        .route(
            "/subtitle-sources/opensubtitles/clear",
            post(routes::subtitle_sources::clear_opensubtitles_credentials),
        )
        .route(
            "/subtitle-sources/opensubtitles/status",
            post(routes::subtitle_sources::get_opensubtitles_status),
        )
        .route(
            "/subtitle-sources/subsource/status",
            post(routes::subtitle_sources::check_subsource_status),
        )
```

- [ ] **Step 6: Add the `"opensubtitles"` arm to the server's provider-test route**

In `subforge/server/src/routes/providers.rs`, add this arm to the existing
`match req.provider.as_str() { ... }` block, before the
`other => Err(format!("Unknown provider: {other}"))` catch-all:

```rust
        "opensubtitles" => {
            match (
                config.opensubtitles_api_key.clone(),
                config.opensubtitles_username.clone(),
                config.opensubtitles_password.clone(),
            ) {
                (Some(api_key), Some(username), Some(password)) => {
                    let creds = subforge_subtitle_sources::OpenSubtitlesCredentials {
                        api_key,
                        username,
                        password,
                    };
                    let client = reqwest::Client::new();
                    subforge_subtitle_sources::opensubtitles::login(
                        &client,
                        subforge_subtitle_sources::opensubtitles::DEFAULT_BASE_URL,
                        &creds,
                        &format!("SubForge v{}", env!("CARGO_PKG_VERSION")),
                    )
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
                }
                _ => Err("No OpenSubtitles credentials configured".to_string()),
            }
        }
```

No signature change is needed here — `config` is already loaded via
`AppConfig::load()` at the top of the existing function and is in scope for
every arm, unlike Task 7's Tauri version which needed a new `State`
parameter because Tauri commands don't have an equivalent ambient `config`
load.

- [ ] **Step 7: Verify it builds**

Run: `SUBFORGE_SUBSOURCE_API_KEY=test-build-key cargo check` (from `subforge/server/`)
Expected: compiles with no errors.

- [ ] **Step 8: Commit**

```bash
git add subforge/server/src/routes/subtitle_sources.rs subforge/server/src/routes/mod.rs subforge/server/src/routes/providers.rs subforge/server/src/main.rs subforge/server/src/config.rs
git commit -m "Add Axum routes for online subtitle search and OpenSubtitles credentials"
```

---

### Task 9: CI updates for the new build requirements

**Files:**
- Modify: `subforge/.github/workflows/ci.yml`
- Modify: `subforge/.github/workflows/release.yml`

**Interfaces:**
- Consumes: nothing from other tasks — purely CI config.
- Produces: green CI for Tasks 1–8's changes.

- [ ] **Step 1: Add `libdbus-1-dev` and the build-time Subsource key to the Linux Tauri CI job**

In `subforge/.github/workflows/ci.yml`, in the `rust-tauri` job's "Install
Tauri Linux deps" step, add `libdbus-1-dev` to the `apt-get install` list
(alongside `libwebkit2gtk-4.1-dev` etc.):

```yaml
      - name: Install Tauri Linux deps
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            libwebkit2gtk-4.1-dev \
            libappindicator3-dev \
            librsvg2-dev \
            libdbus-1-dev \
            patchelf
```

Add an env var to the `cargo check` step for `src-tauri` (this job's last
step):

```yaml
      - run: cargo check
        working-directory: subforge/src-tauri
        env:
          SUBFORGE_SUBSOURCE_API_KEY: ${{ secrets.SUBFORGE_SUBSOURCE_API_KEY || 'ci-placeholder-key' }}
```

- [ ] **Step 2: Add the same env var to the `rust-server` job**

```yaml
      - run: cargo check
        working-directory: subforge/server
        env:
          SUBFORGE_SUBSOURCE_API_KEY: ${{ secrets.SUBFORGE_SUBSOURCE_API_KEY || 'ci-placeholder-key' }}
```

- [ ] **Step 3: Mirror both changes in `release.yml`**

In `subforge/.github/workflows/release.yml`, add `libdbus-1-dev` to the
Linux deps step (same list as Step 1), and add the
`SUBFORGE_SUBSOURCE_API_KEY` env var to the `tauri-apps/tauri-action@v0`
step:

```yaml
      - uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          SUBFORGE_SUBSOURCE_API_KEY: ${{ secrets.SUBFORGE_SUBSOURCE_API_KEY }}
```

Note: unlike the CI placeholder fallback, `release.yml` should **not** fall
back to a placeholder — a real release build needs the real key. If
`secrets.SUBFORGE_SUBSOURCE_API_KEY` isn't set in the repo yet, the release
build will fail at compile time with the `env!` macro's error message from
Task 7 Step 2, which is the intended signal to register the secret before
shipping a release (see the Global Constraints note: registering this key
is the user's manual step, not part of this plan's automated work).

- [ ] **Step 4: Verify the YAML is well-formed**

Run: `python3 -c "import yaml; yaml.safe_load(open('subforge/.github/workflows/ci.yml'))"`
Run: `python3 -c "import yaml; yaml.safe_load(open('subforge/.github/workflows/release.yml'))"`
Expected: no errors from either.

- [ ] **Step 5: Commit**

```bash
git add subforge/.github/workflows/ci.yml subforge/.github/workflows/release.yml
git commit -m "Add libdbus-1-dev and Subsource build key to CI/release workflows"
```

---

### Task 10: Persist rename-resolved metadata for the search step

**Files:**
- Modify: `subforge/src/stores/batchStore.ts`
- Modify: `subforge/src/stores/renameStore.ts`

**Interfaces:**
- Consumes: `renameStore.selectedSeries` and `renameStore.previewRows` (existing).
- Produces: `batchStore.resolvedMetadata: Record<string, ResolvedMetadata>` where `ResolvedMetadata { title: string; year?: number; season?: number; episode?: number; provider: "tmdb" | "tvmaze"; seriesId: string }` — Task 13's `AutoTranslateWizard.vue` reads this.

- [ ] **Step 1: Add the `resolvedMetadata` state and a setter to `batchStore.ts`**

In `subforge/src/stores/batchStore.ts`, add near the top (after the
`ExternalSub`/`VideoFileInfo` interfaces):

```typescript
export interface ResolvedMetadata {
  title: string;
  year?: number;
  season?: number;
  episode?: number;
  provider: "tmdb" | "tvmaze";
  seriesId: string;
}
```

Inside `useBatchStore`'s `defineStore` setup function, add:

```typescript
  const resolvedMetadata = ref<Record<string, ResolvedMetadata>>({});

  function setResolvedMetadata(path: string, metadata: ResolvedMetadata) {
    resolvedMetadata.value[path] = metadata;
  }
```

Add both `resolvedMetadata` and `setResolvedMetadata` to the store's
`return { ... }` object.

- [ ] **Step 2: Write `apply()` in `renameStore.ts` to populate it**

In `subforge/src/stores/renameStore.ts`, `apply()` currently only calls
`apiInvoke<number>("apply_rename", { mapping })`. Change it to also record
metadata for each successfully-renamed row, keyed by the file's **new**
path. Replace the function:

```typescript
  async function apply(): Promise<number> {
    const rows = previewRows.value.filter((r) => r.new_name);
    const mapping = rows.map((r) => ({
      old_path: r.old_path,
      new_path: `${fileDir(r.old_path)}${r.new_name}`,
    }));
    if (mapping.length === 0) return 0;

    const renamed = await apiInvoke<number>("apply_rename", { mapping });

    if (mode.value !== "manual" && selectedSeries.value) {
      const { name, year, provider, id } = selectedSeries.value;
      for (const row of rows) {
        if (row.season == null || row.episode == null) continue;
        const newPath = `${fileDir(row.old_path)}${row.new_name}`;
        useBatchStore().setResolvedMetadata(newPath, {
          title: name,
          year,
          season: row.season,
          episode: row.episode,
          provider,
          seriesId: id,
        });
      }
    }

    return renamed;
  }
```

Add the import at the top of the file:

```typescript
import { useBatchStore } from "./batchStore";
```

- [ ] **Step 3: Write a store test for the new behavior**

Read `subforge/src/stores/batchStore.test.ts` first to match its existing
setup pattern (Pinia test app instance, mocked `apiInvoke`, etc.), then add:

```typescript
it("records resolved metadata for a path", () => {
  const store = useBatchStore();
  store.setResolvedMetadata("/videos/Show - S01E02.mkv", {
    title: "Show",
    year: 2020,
    season: 1,
    episode: 2,
    provider: "tmdb",
    seriesId: "123",
  });
  expect(store.resolvedMetadata["/videos/Show - S01E02.mkv"]).toEqual({
    title: "Show",
    year: 2020,
    season: 1,
    episode: 2,
    provider: "tmdb",
    seriesId: "123",
  });
});
```

- [ ] **Step 4: Run the tests**

Run: `npm test` (from `subforge/`)
Expected: PASS, including the new test.

- [ ] **Step 5: Commit**

```bash
git add subforge/src/stores/batchStore.ts subforge/src/stores/renameStore.ts subforge/src/stores/batchStore.test.ts
git commit -m "Persist TMDB/TVmaze-resolved metadata after rename for subtitle search"
```

---

### Task 11: `subtitleSourcesStore.ts` and API wiring

**Files:**
- Create: `subforge/src/stores/subtitleSourcesStore.ts`
- Modify: `subforge/src/api/client.ts`
- Modify: `subforge/src/api/schemas.ts`

**Interfaces:**
- Consumes: `apiInvoke` (existing), Task 7/8's command names.
- Produces: `useSubtitleSourcesStore()` with `{ searchEnabled, opensubtitlesConfigured, subsourceReachable, loadStatus, setOpenSubtitlesCredentials, clearOpenSubtitlesCredentials, testOpenSubtitlesConnection, searchOnlineSubtitle }` — Tasks 12–13 consume this.

- [ ] **Step 1: Add `cmdToEndpoint` entries**

In `subforge/src/api/client.ts`'s `cmdToEndpoint` map, add:

```typescript
    search_online_subtitle: "/api/subtitle-sources/search",
    set_opensubtitles_credentials: "/api/subtitle-sources/opensubtitles/credentials",
    clear_opensubtitles_credentials: "/api/subtitle-sources/opensubtitles/clear",
    get_opensubtitles_status: "/api/subtitle-sources/opensubtitles/status",
    check_subsource_status: "/api/subtitle-sources/subsource/status",
```

- [ ] **Step 2: Add response schemas**

In `subforge/src/api/schemas.ts`, add to `commandSchemas`:

```typescript
  get_opensubtitles_status: z.object({ configured: z.boolean() }),

  check_subsource_status: z.object({ reachable: z.boolean() }),

  search_online_subtitle: z.discriminatedUnion("status", [
    z.object({
      status: z.literal("found"),
      subtitle_path: z.string(),
      source: z.string(),
      matched_via: z.string(),
    }),
    z.object({ status: z.literal("not_found") }),
  ]),
```

- [ ] **Step 3: Write the store**

Create `subforge/src/stores/subtitleSourcesStore.ts`:

```typescript
import { defineStore } from "pinia";
import { ref } from "vue";
import { apiInvoke } from "../api/client";
import { useAppStore } from "./appStore";

export interface OpenSubtitlesCredentials {
  api_key: string;
  username: string;
  password: string;
}

export interface OnlineSubtitleSearchRequest {
  video_path: string;
  title: string;
  year?: number;
  season?: number;
  episode?: number;
  language: string;
}

export type OnlineSubtitleSearchResult =
  | { status: "found"; subtitle_path: string; source: string; matched_via: string }
  | { status: "not_found" };

export const useSubtitleSourcesStore = defineStore("subtitleSources", () => {
  const opensubtitlesConfigured = ref(false);
  const subsourceReachable = ref<boolean | null>(null);
  const testStatus = ref<"idle" | "testing" | "success" | "error">("idle");
  const testError = ref<string | null>(null);

  const appStore = useAppStore();

  function searchEnabled(): boolean {
    return appStore.config?.search_online_before_translate ?? true;
  }

  async function setSearchEnabled(value: boolean) {
    if (!appStore.config) return;
    appStore.config.search_online_before_translate = value;
    await appStore.saveConfig();
  }

  async function loadStatus() {
    try {
      const status = await apiInvoke<{ configured: boolean }>("get_opensubtitles_status");
      opensubtitlesConfigured.value = status.configured;
    } catch {
      opensubtitlesConfigured.value = false;
    }
    try {
      const status = await apiInvoke<{ reachable: boolean }>("check_subsource_status");
      subsourceReachable.value = status.reachable;
    } catch {
      subsourceReachable.value = false;
    }
  }

  async function setOpenSubtitlesCredentials(credentials: OpenSubtitlesCredentials) {
    await apiInvoke("set_opensubtitles_credentials", { credentials });
    opensubtitlesConfigured.value = true;
  }

  async function clearOpenSubtitlesCredentials() {
    await apiInvoke("clear_opensubtitles_credentials");
    opensubtitlesConfigured.value = false;
  }

  async function testOpenSubtitlesConnection() {
    testStatus.value = "testing";
    testError.value = null;
    try {
      await apiInvoke("test_provider_config", { provider: "opensubtitles" });
      testStatus.value = "success";
    } catch (e) {
      testStatus.value = "error";
      testError.value = e instanceof Error ? e.message : String(e);
    }
  }

  async function searchOnlineSubtitle(
    request: OnlineSubtitleSearchRequest
  ): Promise<OnlineSubtitleSearchResult> {
    return apiInvoke<OnlineSubtitleSearchResult>("search_online_subtitle", { request });
  }

  return {
    opensubtitlesConfigured,
    subsourceReachable,
    testStatus,
    testError,
    searchEnabled,
    setSearchEnabled,
    loadStatus,
    setOpenSubtitlesCredentials,
    clearOpenSubtitlesCredentials,
    testOpenSubtitlesConnection,
    searchOnlineSubtitle,
  };
});
```

Note `search_online_before_translate` must also be added to
`load_config`'s zod schema in `subforge/src/api/schemas.ts` as
`.optional()` (it's a boolean with a server-side default, same treatment as
the existing optional fields there) — add
`search_online_before_translate: z.boolean().optional(),` to the
`load_config` schema object.

- [ ] **Step 4: Verify it type-checks**

Run: `npx vue-tsc --noEmit` (from `subforge/`)
Expected: no new errors.

- [ ] **Step 5: Commit**

```bash
git add subforge/src/stores/subtitleSourcesStore.ts subforge/src/api/client.ts subforge/src/api/schemas.ts
git commit -m "Add subtitleSourcesStore and API wiring for both backends"
```

---

### Task 12: Settings UI — Subtitle Sources section

**Files:**
- Modify: `subforge/src/views/SettingsView.vue`
- Modify: `subforge/src/i18n/en.json`
- Modify: `subforge/src/i18n/ar.json`

**Interfaces:**
- Consumes: `useSubtitleSourcesStore()` (Task 11).
- Produces: nothing consumed by later tasks — this is a leaf UI addition.

- [ ] **Step 1: Add i18n strings**

In `subforge/src/i18n/en.json`, inside the `"settings"` object (after the
existing `"apiProviders"`/`"apiProvidersDesc"` keys, matching the file's
existing flat-key style for this section), add:

```json
  "subtitleSources": "Subtitle Sources",
  "subtitleSourcesDesc": "Search for existing subtitles online before translating.",
  "searchOnlineToggle": "Search for existing subtitles before translating",
  "subsourceStatus": "Subsource",
  "subsourceReachable": "Reachable",
  "subsourceUnreachable": "Unreachable",
  "openSubtitlesTitle": "OpenSubtitles.com",
  "openSubtitlesHelp": "SubForge doesn't ship a shared OpenSubtitles key — register your own free account and API key to enable file-hash matching, the most accurate way to find a subtitle for your exact release.",
  "openSubtitlesRegisterLink": "Create an OpenSubtitles.com account",
  "openSubtitlesDocsLink": "API documentation",
  "openSubtitlesApiKey": "API Key",
  "openSubtitlesUsername": "Username",
  "openSubtitlesPassword": "Password",
  "openSubtitlesSave": "Save",
  "openSubtitlesClear": "Clear",
  "openSubtitlesConfigured": "Configured"
```

Add the same keys with Arabic values to `subforge/src/i18n/ar.json` at the
matching position (same line-order as `en.json`, per the file's existing
convention):

```json
  "subtitleSources": "مصادر الترجمة",
  "subtitleSourcesDesc": "البحث عن ترجمات موجودة عبر الإنترنت قبل الترجمة الآلية.",
  "searchOnlineToggle": "البحث عن ترجمات موجودة قبل الترجمة",
  "subsourceStatus": "Subsource",
  "subsourceReachable": "متاح",
  "subsourceUnreachable": "غير متاح",
  "openSubtitlesTitle": "OpenSubtitles.com",
  "openSubtitlesHelp": "لا يوفر SubForge مفتاح OpenSubtitles مشتركًا — أنشئ حسابك ومفتاح API الخاص بك مجانًا لتفعيل المطابقة عبر بصمة الملف، وهي أدق طريقة لإيجاد ترجمة مطابقة لنسختك بالضبط.",
  "openSubtitlesRegisterLink": "إنشاء حساب على OpenSubtitles.com",
  "openSubtitlesDocsLink": "توثيق API",
  "openSubtitlesApiKey": "مفتاح API",
  "openSubtitlesUsername": "اسم المستخدم",
  "openSubtitlesPassword": "كلمة المرور",
  "openSubtitlesSave": "حفظ",
  "openSubtitlesClear": "مسح",
  "openSubtitlesConfigured": "تم الإعداد"
```

- [ ] **Step 2: Add the section to `SettingsView.vue`**

Read the file's `<script setup>` block first to match its existing
`reactive`/`ref` patterns (it already imports `apiInvoke`, uses
`useAppStore`, `useTranslateStore`). Add near the top:

```typescript
import { useSubtitleSourcesStore } from "../stores/subtitleSourcesStore";
const subtitleSourcesStore = useSubtitleSourcesStore();
const osApiKey = ref("");
const osUsername = ref("");
const osPassword = ref("");

onMounted(() => {
  subtitleSourcesStore.loadStatus();
});
```

(add `onMounted` to the existing `vue` import if not already imported).

Add the template section after the existing "API & Providers" `<section>`
block (matching its `<h2>` + card wrapper structure — read the surrounding
markup around line 362 first to copy the exact wrapper classes used there):

```html
        <section>
          <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("settings.subtitleSources") }}</h2>
          <p class="text-xs text-text-muted mt-1 mb-4">{{ t("settings.subtitleSourcesDesc") }}</p>

          <label class="flex items-center gap-2 text-sm text-text-secondary mb-4">
            <input
              type="checkbox"
              class="accent-accent"
              :checked="subtitleSourcesStore.searchEnabled()"
              @change="subtitleSourcesStore.setSearchEnabled(($event.target as HTMLInputElement).checked)"
            />
            {{ t("settings.searchOnlineToggle") }}
          </label>

          <div class="flex items-center justify-between px-3 py-2 rounded-lg bg-bg-primary border border-border mb-3">
            <span class="text-sm text-text-primary">{{ t("settings.subsourceStatus") }}</span>
            <span
              :class="subtitleSourcesStore.subsourceReachable ? 'text-success' : 'text-error'"
              class="text-xs"
            >
              {{ subtitleSourcesStore.subsourceReachable ? t("settings.subsourceReachable") : t("settings.subsourceUnreachable") }}
            </span>
          </div>

          <div class="p-3 rounded-lg bg-bg-secondary border border-border space-y-2">
            <div class="flex items-center justify-between">
              <span class="text-sm text-text-primary">{{ t("settings.openSubtitlesTitle") }}</span>
              <span v-if="subtitleSourcesStore.opensubtitlesConfigured" class="text-xs text-success">
                {{ t("settings.openSubtitlesConfigured") }}
              </span>
            </div>
            <p class="text-xs text-text-muted">
              {{ t("settings.openSubtitlesHelp") }}
              <a href="https://www.opensubtitles.com" target="_blank" rel="noopener" class="text-accent hover:underline">{{ t("settings.openSubtitlesRegisterLink") }}</a>
              &middot;
              <a href="https://opensubtitles.stoplight.io" target="_blank" rel="noopener" class="text-accent hover:underline">{{ t("settings.openSubtitlesDocsLink") }}</a>
            </p>
            <input
              v-model="osApiKey"
              type="text"
              :placeholder="t('settings.openSubtitlesApiKey')"
              class="w-full px-3 py-2 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
            />
            <input
              v-model="osUsername"
              type="text"
              :placeholder="t('settings.openSubtitlesUsername')"
              class="w-full px-3 py-2 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
            />
            <input
              v-model="osPassword"
              type="password"
              :placeholder="t('settings.openSubtitlesPassword')"
              class="w-full px-3 py-2 rounded-lg bg-bg-primary border border-border text-sm text-text-primary focus:outline-none focus:border-accent"
            />
            <div class="flex items-center gap-2">
              <button
                class="px-3 py-1.5 rounded-md bg-accent text-white text-xs font-medium hover:bg-accent-hover transition-colors"
                @click="async () => { await subtitleSourcesStore.setOpenSubtitlesCredentials({ api_key: osApiKey, username: osUsername, password: osPassword }); await subtitleSourcesStore.testOpenSubtitlesConnection(); }"
              >
                {{ t("settings.openSubtitlesSave") }}
              </button>
              <button
                v-if="subtitleSourcesStore.opensubtitlesConfigured"
                class="px-3 py-1.5 rounded-md bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
                @click="subtitleSourcesStore.clearOpenSubtitlesCredentials"
              >
                {{ t("settings.openSubtitlesClear") }}
              </button>
              <span v-if="subtitleSourcesStore.testStatus === 'success'" class="text-xs text-success">{{ t("settings.testSuccess") }}</span>
              <span v-else-if="subtitleSourcesStore.testStatus === 'error'" class="text-xs text-error">{{ subtitleSourcesStore.testError }}</span>
            </div>
          </div>
        </section>
```

- [ ] **Step 2: Verify it type-checks and builds**

Run: `npx vue-tsc --noEmit` (from `subforge/`)
Run: `npm run lint` (from `subforge/`)
Expected: no new errors.

- [ ] **Step 3: Commit**

```bash
git add subforge/src/views/SettingsView.vue subforge/src/i18n/en.json subforge/src/i18n/ar.json
git commit -m "Add Subtitle Sources settings section"
```

---

### Task 13: Wire the search step into `AutoTranslateWizard.vue`

**Files:**
- Modify: `subforge/src/components/AutoTranslateWizard.vue`
- Modify: `subforge/src/i18n/en.json`
- Modify: `subforge/src/i18n/ar.json`

**Interfaces:**
- Consumes: `useSubtitleSourcesStore()` (Task 11), `batchStore.resolvedMetadata` (Task 10), `subforge_metadata::parser::parse_filename` via the existing `apiInvoke("parse_filename", ...)` call already used elsewhere in the codebase (`renameStore.ts`).
- Produces: nothing consumed by later tasks.

- [ ] **Step 1: Add i18n stage labels**

In `subforge/src/i18n/en.json`, inside `"autoTranslate"` → `"stage"`, add
`"searching": "Searching online subtitles..."` before `"extracting"`. Also
add two new top-level keys inside `"autoTranslate"`:

```json
  "foundOnline": "Found on {source}",
```

In `subforge/src/i18n/ar.json`, matching position:

```json
  "searching": "جارٍ البحث عن ترجمات عبر الإنترنت...",
```

and

```json
  "foundOnline": "تم العثور عليها في {source}",
```

- [ ] **Step 2: Extend `FileStage` and wire the search call**

In `subforge/src/components/AutoTranslateWizard.vue`:

Change the `FileStage` type:

```typescript
type FileStage =
  | "queued"
  | "searching"
  | "extracting"
  | "translating"
  | "merging"
  | "done"
  | "error"
  | "cancelled";
```

Add the import:

```typescript
import { useSubtitleSourcesStore } from "../stores/subtitleSourcesStore";
```

and instantiate it alongside the existing stores:

```typescript
const subtitleSourcesStore = useSubtitleSourcesStore();
```

In `startPipeline()`, the existing loop over `fileJobs.value` currently
starts with `const trackId = findSubtitleTrack(job.path);`. Wrap the whole
per-job body so the search happens first when enabled:

```typescript
  for (const job of fileJobs.value) {
    if (subtitleSourcesStore.searchEnabled()) {
      job.stage = "searching";
      const metadata = batchStore.resolvedMetadata[job.path];
      let title = metadata?.title;
      let year = metadata?.year;
      let season = metadata?.season;
      let episode = metadata?.episode;

      if (!title) {
        try {
          const fileName = job.path.split(/[/\\]/).pop() || job.path;
          const parsed = await apiInvoke<{ series_guess: string; season?: number; episode?: number }>(
            "parse_filename",
            { name: fileName }
          );
          title = parsed.series_guess;
          season = parsed.season;
          episode = parsed.episode;
        } catch {
          title = undefined;
        }
      }

      if (title) {
        try {
          const result = await subtitleSourcesStore.searchOnlineSubtitle({
            video_path: job.path,
            title,
            year,
            season,
            episode,
            language: job.lang,
          });
          if (result.status === "found") {
            job.stage = "done";
            job.progress = 100;
            job.message = t("autoTranslate.foundOnline", { source: result.source });
            batchStore.upsertExternalSub(job.path, job.lang, "ok");
            continue;
          }
        } catch {
          // Search failures fall through to translation — never block the
          // pipeline on a source being down.
        }
      }
    }

    const trackId = findSubtitleTrack(job.path);
    if (trackId === null) {
      job.stage = "error";
      job.message = t("autoTranslate.noSubtitleTrack");
      continue;
    }

    const request: Record<string, unknown> = {
      video_path: job.path,
      track_id: trackId,
      engine: provider.value,
      source_lang: sourceLang.value,
      target_lang: job.lang,
      auto_merge: autoMerge.value,
      set_default_subtitle: autoMerge.value && setDefaultSubtitle.value,
    };
    if (provider.value === "ollama") {
      request.host = translateStore.ollamaHost;
      request.model = translateStore.selectedOllamaModel || null;
    } else if (provider.value === "libretranslate") request.host = translateStore.libreHost;
    else if (provider.value === "deepl") request.api_key = translateStore.deeplKey;
    else if (provider.value === "openai") {
      request.api_key = translateStore.openaiKey;
      request.model = translateStore.openaiModel;
    } else if (provider.value === "google") request.api_key = translateStore.googleApiKey;

    try {
      const jobId = await apiInvoke<string>("run_translation_pipeline", { request });
      job.jobId = jobId;
      job.stage = "extracting";
      await batchStore.registerPipelineJob(jobId, job.path, job.lang);
    } catch (e) {
      job.stage = "error";
      job.message = mapJobErrorMessage(e instanceof Error ? e.message : String(e));
    }
  }
```

This replaces the existing `for (const job of fileJobs.value) { ... }` loop
body in `startPipeline()` entirely (the trailing portion from `const
trackId = findSubtitleTrack(job.path);` onward is unchanged from the
current code — only the new search block is prepended inside the loop).

- [ ] **Step 3: Add the `"searching"` stage to the stage icon/label logic**

The existing template already calls `stageLabel(job.stage)` generically via
`t(\`autoTranslate.stage.${stage}\`)`, so the new `"searching"` stage's
label resolves automatically from Step 1's i18n addition — no template
change needed for the label. Confirm the existing `Loader2` spinner icon
branch (`v-else` in the status icon `<span>`) already covers any
non-done/non-error stage, including `"searching"` — read the template
around the `CheckCircle2`/`XCircle`/`Loader2` icons (already shown in the
file's current content) to confirm; if the icon logic is truly generic
(`done` → check, `error`/`cancelled` → X, else → spinner), no template
change is needed here.

- [ ] **Step 4: Verify it type-checks**

Run: `npx vue-tsc --noEmit` (from `subforge/`)
Expected: no new errors.

- [ ] **Step 5: Commit**

```bash
git add subforge/src/components/AutoTranslateWizard.vue subforge/src/i18n/en.json subforge/src/i18n/ar.json
git commit -m "Search online subtitles before auto-translating, per file"
```

---

### Task 14: Final verification pass

**Files:**
- None created/modified — this task runs the full checklist from the spec.

**Interfaces:**
- Consumes: everything from Tasks 1–13.

- [ ] **Step 1: Run the full Rust test suite for all three crates**

Run: `SUBFORGE_SUBSOURCE_API_KEY=test-key cargo test --workspace` — if this
fails because the three crates aren't a real Cargo workspace (confirmed in
the design spec they're not — each has its own `Cargo.lock`), instead run
each individually:

```bash
SUBFORGE_SUBSOURCE_API_KEY=test-key cargo test --manifest-path subforge/crates/subforge-subtitle-sources/Cargo.toml
SUBFORGE_SUBSOURCE_API_KEY=test-key cargo test --manifest-path subforge/src-tauri/Cargo.toml
SUBFORGE_SUBSOURCE_API_KEY=test-key cargo test --manifest-path subforge/server/Cargo.toml
```

Expected: all PASS.

- [ ] **Step 2: Run the frontend test/lint/typecheck suite**

Run: `npm test && npm run lint && npx vue-tsc --noEmit` (from `subforge/`)
Expected: all PASS.

- [ ] **Step 3: Manual verification checklist (run these yourself; report back which pass)**

- [ ] Linux: `npm run tauri dev` launches, Settings shows the new "Subtitle
      Sources" section, the toggle persists across a restart.
- [ ] With no OpenSubtitles credentials configured: batch-scan a folder with
      a video whose embedded subtitle track exists, run Auto-Translate — the
      job should show "Searching online subtitles..." then either "Found on
      subsource" (skipping translation) or fall through to "Translating..."
      depending on whether Subsource actually has that title.
- [ ] With real OpenSubtitles credentials entered and saved (needs your own
      account — I cannot supply or fabricate one): "Test" button in Settings
      shows "Connected"; a batch run prefers `matched_via: "hash"` results
      when a hash match exists.
- [ ] Turn the toggle off: batch run skips straight to "Translating..." with
      no "Searching..." stage at all.
- [ ] Disconnect network (or block outbound to `api.subsource.net`/
      `api.opensubtitles.com`) and run a batch — the app must not hang or
      crash; every job should fall through to translation (or fail
      gracefully if translation also can't run) within a normal timeout.
- [ ] Windows: confirm CI's `windows-latest` build in `release.yml` succeeds
      after these changes (push a tag or check the next scheduled run).
- [ ] macOS: best-effort only, per the design spec — no hardware available
      here to verify directly.

- [ ] **Step 4: `git status` to confirm a clean tree, then stop (no final commit needed if Steps 1–3 required no fixes)**

Run: `git status` (from `subforge/`)
Expected: "nothing to commit, working tree clean" if every prior task's
commit already captured its changes. If Steps 1–2 required fixes, commit
those with a message like `"Fix test/lint failures found in final
verification pass"`.
