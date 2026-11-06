//! *arr integration: the SignalR event feed, with REST as the source of truth.
//!
//! Bazarr's trick is that Sonarr/Radarr *push* — one outbound WebSocket per app to
//! `<arr>/signalr/messages?access_token=<key>` — instead of the tool polling every N minutes.
//! The hub's payload shapes are an implementation detail we deliberately do not depend on: the
//! feed is used as a clock, and the actual file paths come from the REST API's history, which is
//! documented and stable. A malformed or changed event therefore costs one wasted lookup, never a
//! missed episode.
//!
//! Protocol notes that matter: every frame is a JSON object terminated by `\u{1e}`, the client
//! must send `{"protocol":"json","version":1}\u{1e}` before anything else, `type: 1` is an
//! invocation (what we care about), `type: 6` is a keep-alive.

use std::collections::HashSet;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message as WsMessage;

/// One configured Sonarr/Radarr instance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArrEndpoint {
    pub kind: ArrKind,
    /// Base URL as the user reaches it, e.g. `http://192.168.3.5:8989`.
    pub url: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ArrKind {
    Sonarr,
    Radarr,
}

impl ArrKind {
    fn file_endpoint(&self) -> &'static str {
        match self {
            // Sonarr: /api/v3/episodefile/{id}; Radarr: /api/v3/moviefile/{id}.
            ArrKind::Sonarr => "episodefile",
            ArrKind::Radarr => "moviefile",
        }
    }
}

impl ArrEndpoint {
    /// `http(s)://host/` → `ws(s)://host/signalr/messages?access_token=…`.
    pub fn signalr_url(&self) -> String {
        let base = self.url.trim_end_matches('/');
        let base = if let Some(rest) = base.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if let Some(rest) = base.strip_prefix("http://") {
            format!("ws://{rest}")
        } else {
            format!("ws://{base}")
        };
        format!("{base}/signalr/messages?access_token={}", self.api_key)
    }

    pub fn api(&self, path: &str) -> String {
        format!(
            "{}/api/v3/{}",
            self.url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }
}

/// The frame the client must send first.
pub const HANDSHAKE: &str = "{\"protocol\":\"json\",\"version\":1}\u{1e}";

/// One parsed hub frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    /// `type: 1` — the server telling us something happened.
    Invocation { target: String },
    /// `type: 6` — keep-alive.
    Ping,
    /// Anything else (completion, error, handshake reply).
    Other,
}

/// Splits a buffer on the record separator and returns complete frames plus the remainder.
/// A partial frame is kept, because a WebSocket message boundary is not a frame boundary.
pub fn split_frames(buffer: &str) -> (Vec<String>, String) {
    let mut frames = Vec::new();
    let mut rest = buffer;
    while let Some(position) = rest.find('\u{1e}') {
        let (frame, after) = rest.split_at(position);
        let after = &after['\u{1e}'.len_utf8()..];
        if !frame.trim().is_empty() {
            frames.push(frame.trim().to_string());
        }
        rest = after;
    }
    (frames, rest.to_string())
}

pub fn parse_frame(text: &str) -> Option<Frame> {
    let value: Value = serde_json::from_str(text).ok()?;
    match value.get("type").and_then(Value::as_u64) {
        Some(1) => Some(Frame::Invocation {
            target: value
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }),
        Some(6) => Some(Frame::Ping),
        // Sonarr answers the handshake with `{}`; completions and errors are noise for us.
        _ => Some(Frame::Other),
    }
}

/// Events worth waking up for. Everything else (log messages, queue chatter) is ignored so a
/// busy instance does not turn into a lookup storm.
pub fn is_interesting(target: &str) -> bool {
    matches!(
        target,
        "episode"
            | "series"
            | "movie"
            | "episodefile"
            | "moviefile"
            | "queue"
            | "command"
            | "history"
    )
}

/// Which *arr instance and which record a file came from, so the *arr can be told afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrRef {
    pub endpoint: ArrEndpoint,
    /// Sonarr series id, Radarr movie id.
    pub resource_id: i64,
}

/// A file the *arr has just finished importing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedFile {
    pub path: String,
    /// Present when the *arr can be told to rescan afterwards.
    pub arr: Option<ArrRef>,
}

/// One history record, in the subset the API returns that we rely on.
#[derive(Debug, Clone, Deserialize)]
struct HistoryRecord {
    #[serde(default, rename = "episodeFileId")]
    episode_file_id: Option<i64>,
    #[serde(default, rename = "movieFileId")]
    movie_file_id: Option<i64>,
    #[serde(default)]
    event_type: Option<String>,
    #[serde(default, rename = "seriesId")]
    series_id: Option<i64>,
    #[serde(default, rename = "movieId")]
    movie_id: Option<i64>,
    #[serde(default)]
    episode: Option<Value>,
    #[serde(default)]
    movie: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct HistoryResponse {
    #[serde(default)]
    records: Vec<HistoryRecord>,
}

/// What the client talks to. Kept small so a test can point it at a fake server.
pub struct ArrClient {
    endpoint: ArrEndpoint,
    http: reqwest::Client,
    /// Files already handed to the pipeline, so a restart re-processing the same history window
    /// does not redo the work.
    seen: HashSet<String>,
}

impl ArrClient {
    pub fn new(endpoint: ArrEndpoint) -> Self {
        Self {
            endpoint,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            seen: HashSet::new(),
        }
    }

    /// The most recent imports, newest first. The SignalR feed says "something changed"; this is
    /// the answer to "which files".
    pub async fn recent_imports(&mut self) -> Result<Vec<ImportedFile>, String> {
        let url = self
            .endpoint
            .api("history?page=1&pageSize=25&sortKey=date&sortDirection=descending");
        let response = self
            .http
            .get(&url)
            .header("X-Api-Key", &self.endpoint.api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|error| format!("history request failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "history returned HTTP {} for {url}",
                response.status().as_u16()
            ));
        }
        let history: HistoryResponse = response
            .json()
            .await
            .map_err(|error| format!("history response is not JSON: {error}"))?;

        let mut imports = Vec::new();
        for record in history.records {
            // Only "the download was imported" matters; grabs and renames are noise.
            let event = record.event_type.as_deref().unwrap_or_default();
            if !event.is_empty() && event != "downloadFolderImported" {
                continue;
            }
            let file_id = match self.endpoint.kind {
                ArrKind::Sonarr => record.episode_file_id,
                ArrKind::Radarr => record.movie_file_id,
            };
            let resource_id = match self.endpoint.kind {
                ArrKind::Sonarr => record.series_id,
                ArrKind::Radarr => record.movie_id,
            };
            let reference = resource_id.map(|resource_id| ArrRef {
                endpoint: self.endpoint.clone(),
                resource_id,
            });
            let Some(file_id) = file_id else {
                // Older history rows and deletions have no file: fall back to the embedded
                // resource when it carries a path.
                if let Some(path) = embedded_path(&record) {
                    if self.seen.insert(path.clone()) {
                        imports.push(ImportedFile {
                            path,
                            arr: reference,
                        });
                    }
                }
                continue;
            };
            match self.file_path(file_id).await {
                Ok(Some(path)) => {
                    if self.seen.insert(path.clone()) {
                        imports.push(ImportedFile {
                            path,
                            arr: reference,
                        });
                    }
                }
                Ok(None) => {}
                Err(error) => return Err(error),
            }
        }
        // Keep the memo bounded: this process may run for months.
        if self.seen.len() > 2_000 {
            self.seen.clear();
        }
        Ok(imports)
    }

    /// `episodefile/{id}` (Sonarr) or `moviefile/{id}` (Radarr) → the path on disk.
    pub async fn file_path(&self, file_id: i64) -> Result<Option<String>, String> {
        let url = self.endpoint.api(&format!(
            "{}/{}",
            self.endpoint.kind.file_endpoint(),
            file_id
        ));
        let response = self
            .http
            .get(&url)
            .header("X-Api-Key", &self.endpoint.api_key)
            .send()
            .await
            .map_err(|error| format!("file request failed: {error}"))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            // The file was deleted between the history read and this call.
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(format!(
                "file {file_id} returned HTTP {}",
                response.status().as_u16()
            ));
        }
        let value: Value = response
            .json()
            .await
            .map_err(|error| format!("file response is not JSON: {error}"))?;
        Ok(value
            .get("path")
            .or_else(|| value.get("relativePath"))
            .and_then(Value::as_str)
            .map(str::to_string))
    }
}

fn embedded_path(record: &HistoryRecord) -> Option<String> {
    record
        .episode
        .as_ref()
        .and_then(|episode| episode.get("episodeFile"))
        .and_then(|file| file.get("path"))
        .or_else(|| {
            record
                .movie
                .as_ref()
                .and_then(|movie| movie.get("movieFile"))
                .and_then(|file| file.get("path"))
        })
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Tells the *arr that the file changed, so its own UI shows the new subtitle immediately.
///
/// Sonarr/Radarr answer this with a command id and do the work asynchronously; we only need the
/// request to be accepted, and a failure here must never fail the pipeline — the episode is
/// already correct on disk.
pub async fn request_rescan(client: &reqwest::Client, reference: &ArrRef) -> Result<(), String> {
    let kind = reference.endpoint.kind;
    let body = match kind {
        ArrKind::Sonarr => serde_json::json!({
            "name": "RescanSeries",
            "seriesId": reference.resource_id,
        }),
        ArrKind::Radarr => serde_json::json!({
            "name": "RescanMovie",
            "movieId": reference.resource_id,
        }),
    };
    let url = reference.endpoint.api("command");
    let response = client
        .post(&url)
        .header("X-Api-Key", &reference.endpoint.api_key)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("rescan request failed: {error}"))?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "the *arr refused the rescan: HTTP {}",
            response.status().as_u16()
        ))
    }
}

/// What the watcher does with an import: this module stays out of the decision, so the same code
/// serves the CLI, the server and (later) the scheduler.
pub type OnImport = Box<dyn Fn(ImportedFile) + Send + Sync>;

/// Connects to one *arr, reconnects forever, and calls `on_import` when the feed says something
/// changed. Never returns; spawn it.
pub async fn follow(endpoint: ArrEndpoint, on_import: OnImport) {
    let mut client = ArrClient::new(endpoint.clone());
    let mut backoff = Duration::from_secs(2);
    loop {
        match connect_and_listen(&endpoint, &on_import).await {
            Ok(()) => backoff = Duration::from_secs(2),
            Err(error) => {
                eprintln!(
                    "[arr] {} feed dropped ({error}); retrying in {}s",
                    endpoint.url,
                    backoff.as_secs()
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(300));
            }
        }
        // After every (re)connect, sweep the recent history once: events that arrived while we
        // were disconnected are not replayed by the hub.
        match client.recent_imports().await {
            Ok(imports) => {
                for import in imports {
                    on_import(import);
                }
            }
            Err(error) => eprintln!("[arr] {} history sweep failed: {error}", endpoint.url),
        }
    }
}

async fn connect_and_listen(endpoint: &ArrEndpoint, on_import: &OnImport) -> Result<(), String> {
    let url = endpoint.signalr_url();
    let (stream, _) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|error| format!("cannot connect: {error}"))?;
    let (mut sink, mut source) = stream.split();
    sink.send(WsMessage::text(HANDSHAKE))
        .await
        .map_err(|error| format!("cannot send the handshake: {error}"))?;
    eprintln!("[arr] listening to {}", endpoint.url);

    let mut buffer = String::new();
    loop {
        let message = match tokio::time::timeout(Duration::from_secs(120), source.next()).await {
            // 120 s of silence means the connection is dead even if TCP has not noticed.
            Err(_) => return Err("no frames for 120s".to_string()),
            Ok(None) => return Err("connection closed".to_string()),
            Ok(Some(Err(error))) => return Err(error.to_string()),
            Ok(Some(Ok(message))) => message,
        };
        match message {
            WsMessage::Text(text) => {
                buffer.push_str(text.as_str());
                let (frames, rest) = split_frames(&buffer);
                buffer = rest;
                for frame in frames {
                    let Some(parsed) = parse_frame(&frame) else {
                        continue;
                    };
                    if let Frame::Invocation { target } = parsed {
                        if !is_interesting(&target) {
                            continue;
                        }
                        // The event says "look again"; the REST history says what arrived. The
                        // client is rebuilt per sweep by the caller, so keep this stateless here.
                        let mut client = ArrClient::new(endpoint.clone());
                        match client.recent_imports().await {
                            Ok(imports) => {
                                for import in imports {
                                    on_import(import);
                                }
                            }
                            Err(error) => {
                                eprintln!(
                                    "[arr] {} lookup after {target} failed: {error}",
                                    endpoint.url
                                )
                            }
                        }
                    }
                }
            }
            WsMessage::Close(_) => return Err("closed by the server".to_string()),
            WsMessage::Ping(payload) => {
                let _ = sink.send(WsMessage::Pong(payload)).await;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn endpoint(kind: ArrKind) -> ArrEndpoint {
        ArrEndpoint {
            kind,
            url: "http://192.168.3.5:8989/".to_string(),
            api_key: "secret".to_string(),
        }
    }

    #[test]
    fn the_signalr_url_is_derived_from_the_base_url() {
        assert_eq!(
            endpoint(ArrKind::Sonarr).signalr_url(),
            "ws://192.168.3.5:8989/signalr/messages?access_token=secret"
        );
        let https = ArrEndpoint {
            kind: ArrKind::Radarr,
            url: "https://arr.example/media/".to_string(),
            api_key: "k".to_string(),
        };
        assert_eq!(
            https.signalr_url(),
            "wss://arr.example/media/signalr/messages?access_token=k"
        );
        assert_eq!(
            endpoint(ArrKind::Sonarr).api("history"),
            "http://192.168.3.5:8989/api/v3/history"
        );
    }

    #[test]
    fn frames_are_split_on_the_record_separator_and_partials_are_kept() {
        let (frames, rest) = split_frames("{\"a\":1}\u{1e}{\"b\":2}\u{1e}{\"c\":");
        assert_eq!(frames, vec!["{\"a\":1}", "{\"b\":2}"]);
        assert_eq!(rest, "{\"c\":");

        // A message that ends exactly on the separator leaves nothing behind.
        let (frames, rest) = split_frames("{\"a\":1}\u{1e}");
        assert_eq!(frames.len(), 1);
        assert!(rest.is_empty());

        // Garbage and empty frames are dropped, not parsed.
        let (frames, _) = split_frames("\u{1e}  \u{1e}");
        assert!(frames.is_empty());
    }

    #[test]
    fn an_invocation_is_recognised_and_a_keepalive_is_not_an_event() {
        assert_eq!(
            parse_frame(&json!({"type": 1, "target": "episode", "arguments": [{}]}).to_string()),
            Some(Frame::Invocation {
                target: "episode".to_string()
            })
        );
        assert_eq!(
            parse_frame(&json!({"type": 6}).to_string()),
            Some(Frame::Ping)
        );
        assert_eq!(parse_frame("{}"), Some(Frame::Other));
        assert_eq!(parse_frame("not json"), None);
    }

    #[test]
    fn only_file_related_targets_wake_the_pipeline() {
        for target in [
            "episode",
            "series",
            "movie",
            "episodefile",
            "moviefile",
            "queue",
            "command",
            "history",
        ] {
            assert!(is_interesting(target), "{target} should be interesting");
        }
        for target in ["log", "health", "notification", ""] {
            assert!(
                !is_interesting(target),
                "{target} should not be interesting"
            );
        }
    }

    /// A fake *arr: `/api/v3/history` returns one import whose file id resolves to a path.
    async fn fake_arr(port: u16, kind: ArrKind) -> tokio::task::JoinHandle<()> {
        use axum::routing::get;
        let history = json!({
            "records": [
                {"eventType": "downloadFolderImported", "episodeFileId": 42, "movieFileId": 42},
                {"eventType": "grabbed", "episodeFileId": 43, "movieFileId": 43}
            ]
        });
        let file = json!({"id": 42, "path": "/media/tv/Show/Season 01/Show.S01E01.mkv"});
        let app = axum::Router::new()
            .route(
                "/api/v3/history",
                get({
                    let history = history.clone();
                    move || async move { axum::Json(history.clone()) }
                }),
            )
            .route(
                if kind == ArrKind::Sonarr {
                    "/api/v3/episodefile/42"
                } else {
                    "/api/v3/moviefile/42"
                },
                get(move || async move { axum::Json(file.clone()) }),
            );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        })
    }

    #[tokio::test]
    async fn the_history_sweep_turns_into_the_file_path() {
        let port = 18081;
        let _server = fake_arr(port, ArrKind::Sonarr).await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let endpoint = ArrEndpoint {
            kind: ArrKind::Sonarr,
            url: format!("http://127.0.0.1:{port}"),
            api_key: "secret".to_string(),
        };
        let mut client = ArrClient::new(endpoint);
        let imports = client.recent_imports().await.unwrap();
        assert_eq!(
            imports,
            vec![ImportedFile {
                path: "/media/tv/Show/Season 01/Show.S01E01.mkv".to_string(),
                // The fake history carries no seriesId, so there is nobody to tell afterwards.
                arr: None,
            }],
            "the grabbed record must be skipped and the import resolved to its path"
        );

        // A second sweep sees the same history and must not re-emit the file.
        assert!(client.recent_imports().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_rescan_names_the_command_and_the_record() {
        use std::sync::{Arc, Mutex};

        // A fake *arr that records the command it was asked to run.
        let seen: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Arc::default();
        let sink = seen.clone();
        let app = axum::Router::new().route(
            "/api/v3/command",
            axum::routing::post(move |body: String| {
                let sink = sink.clone();
                async move {
                    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
                    sink.lock().unwrap().push(("command".to_string(), value));
                    axum::Json(json!({"id": 1, "name": "RescanSeries", "status": "queued"}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 18090))
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        tokio::time::sleep(Duration::from_millis(200)).await;

        let client = reqwest::Client::new();
        for (kind, expected) in [
            (ArrKind::Sonarr, "RescanSeries"),
            (ArrKind::Radarr, "RescanMovie"),
        ] {
            let reference = ArrRef {
                endpoint: ArrEndpoint {
                    kind,
                    url: "http://127.0.0.1:18090".to_string(),
                    api_key: "k".to_string(),
                },
                resource_id: 42,
            };
            request_rescan(&client, &reference).await.unwrap();
            let seen = seen.lock().unwrap();
            let body = &seen.last().unwrap().1;
            assert_eq!(body["name"], expected);
            let id_field = if kind == ArrKind::Sonarr {
                "seriesId"
            } else {
                "movieId"
            };
            assert_eq!(body[id_field], 42, "{body}");
        }
    }

    #[tokio::test]
    async fn a_missing_file_is_not_an_error() {
        let port = 18082;
        let _server = fake_arr(port, ArrKind::Radarr).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        let client = ArrClient::new(ArrEndpoint {
            kind: ArrKind::Radarr,
            url: format!("http://127.0.0.1:{port}"),
            api_key: "k".to_string(),
        });
        // 99 was never served by the fake, so the server answers 404 and the client says "gone".
        assert_eq!(client.file_path(99).await.unwrap(), None);
    }

    /// The whole path with nothing faked in the client: a real WebSocket handshake, a real frame
    /// stream, a real REST lookup, and the callback the watcher uses.
    #[tokio::test]
    async fn the_feed_wakes_the_pipeline_and_the_history_names_the_file() {
        use axum::extract::ws::{Message, WebSocketUpgrade};

        async fn hub(mut socket: axum::extract::ws::WebSocket) {
            // 1. The client's handshake, then the hub's `{}` reply.
            let _ = socket.recv().await;
            let _ = socket
                .send(Message::Text("{\"type\":0}\u{1e}".into()))
                .await;
            // 2. Something happened: an invocation frame, split across two messages on purpose so
            //    the buffer has to survive a message boundary.
            let frame = json!({"type": 1, "target": "episode", "arguments": [{}]}).to_string();
            let (first, second) = frame.split_at(10);
            let _ = socket.send(Message::Text(first.to_string().into())).await;
            let _ = socket
                .send(Message::Text(format!("{second}\u{1e}").into()))
                .await;
            // Keep the socket open until the test finishes.
            while let Some(Ok(_)) = socket.recv().await {}
        }

        let port = 18084;
        let app = axum::Router::new()
            .route(
                "/signalr/messages",
                axum::routing::get(move |upgrade: WebSocketUpgrade| async move {
                    upgrade.on_upgrade(hub)
                }),
            )
            .route(
                "/api/v3/history",
                axum::routing::get(|| async {
                    axum::Json(json!({"records": [
                        {"eventType": "downloadFolderImported", "episodeFileId": 42}
                    ]}))
                }),
            )
            .route(
                "/api/v3/episodefile/42",
                axum::routing::get(|| async {
                    axum::Json(json!({"id": 42, "path": "/media/tv/Show/S01E02.mkv"}))
                }),
            );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        tokio::time::sleep(Duration::from_millis(200)).await;

        let (sender, mut receiver) = tokio::sync::mpsc::channel::<String>(4);
        let endpoint = ArrEndpoint {
            kind: ArrKind::Sonarr,
            url: format!("http://127.0.0.1:{port}"),
            api_key: "secret".to_string(),
        };
        tokio::spawn(follow(
            endpoint,
            Box::new(move |import| {
                let _ = sender.try_send(import.path);
            }),
        ));

        let path = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("the callback should fire within 5s")
            .expect("a path");
        assert_eq!(path, "/media/tv/Show/S01E02.mkv");
    }

    #[tokio::test]
    async fn a_bad_key_is_reported_rather_than_swallowed() {
        // No server on this port: the error must name the request, not panic.
        let mut client = ArrClient::new(ArrEndpoint {
            kind: ArrKind::Sonarr,
            url: "http://127.0.0.1:18083".to_string(),
            api_key: "k".to_string(),
        });
        let error = client.recent_imports().await.unwrap_err();
        assert!(error.contains("history"), "{error}");
    }
}
