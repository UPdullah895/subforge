//! Download job tests against a local axum stub (supports Range, can stall).

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use sha2::{Digest, Sha256};
use subforge_core::whisper::{
    default_cache_dir, download, download_with, legacy_cache_dir, list_downloaded,
    list_downloaded_specs, ModelSpec, WhisperError, MODELS, VAD_MODEL,
};
use subforge_core::CancellationToken;

const SIZE: usize = 5 * 1024 * 1024;
const CHUNK: usize = 64 * 1024;

struct Stub {
    body: Vec<u8>,
    /// Bytes sent so far across all responses.
    sent: AtomicUsize,
    /// A response stalls (never finishes) once it has sent this many bytes.
    stall_at: AtomicUsize,
    /// 0 normal, 1 ignore Range (always 200), 2 answer 206 with the wrong Content-Range start.
    mode: AtomicUsize,
    ranges: Mutex<Vec<String>>,
    paths: Mutex<Vec<String>>,
}

fn body_bytes() -> Vec<u8> {
    (0..SIZE).map(|i| (i % 251) as u8).collect()
}

async fn serve(State(st): State<Arc<Stub>>, uri: axum::http::Uri, headers: HeaderMap) -> Response {
    st.paths.lock().unwrap().push(uri.path().to_string());
    let mut start = 0usize;
    let mut partial = false;
    if let Some(r) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
        st.ranges.lock().unwrap().push(r.to_string());
        start = r
            .trim_start_matches("bytes=")
            .trim_end_matches('-')
            .parse()
            .unwrap();
        if start >= SIZE {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .body(Body::empty())
                .unwrap();
        }
        partial = true;
        match st.mode.load(Ordering::SeqCst) {
            1 => {
                start = 0;
                partial = false;
            }
            2 => start = 0,
            _ => {}
        }
    }
    let stream =
        futures_util::stream::unfold((st.clone(), start, 0usize), |(st, pos, sent)| async move {
            if pos >= SIZE {
                return None;
            }
            if sent >= st.stall_at.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            let end = (pos + CHUNK).min(SIZE);
            tokio::time::sleep(Duration::from_millis(1)).await;
            st.sent.fetch_add(end - pos, Ordering::SeqCst);
            let data = st.body[pos..end].to_vec();
            Some((Ok::<_, std::io::Error>(data), (st, end, sent + (end - pos))))
        });
    let mut b = Response::builder()
        .status(if partial {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header(header::CONTENT_LENGTH, SIZE - start);
    if partial {
        b = b.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{}/{SIZE}", SIZE - 1),
        );
    }
    b.body(Body::from_stream(stream)).unwrap()
}

async fn start_stub() -> (String, Arc<Stub>) {
    let st = Arc::new(Stub {
        body: body_bytes(),
        sent: AtomicUsize::new(0),
        stall_at: AtomicUsize::new(usize::MAX),
        mode: AtomicUsize::new(0),
        ranges: Mutex::new(vec![]),
        paths: Mutex::new(vec![]),
    });
    let app = Router::new().fallback(get(serve)).with_state(st.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), st)
}

fn sha(data: &[u8]) -> &'static str {
    Box::leak(format!("{:x}", Sha256::digest(data)).into_boxed_str())
}

fn spec(sha256: Option<&'static str>) -> ModelSpec {
    ModelSpec {
        name: "test",
        file: "ggml-test.bin",
        size_mb: 5,
        size_bytes: Some(SIZE as u64),
        sha256,
    }
}

fn vad_spec() -> ModelSpec {
    ModelSpec {
        sha256: Some(sha(&body_bytes())),
        size_bytes: Some(SIZE as u64),
        ..VAD_MODEL
    }
}

/// Cancels `cancel` once the stub has sent half the body.
fn cancel_at_half(st: &Arc<Stub>, cancel: &CancellationToken) {
    let (st, cancel) = (st.clone(), cancel.clone());
    tokio::spawn(async move {
        while st.sent.load(Ordering::SeqCst) < SIZE / 2 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        cancel.cancel();
    });
}

#[tokio::test]
async fn download_reports_monotonic_progress_to_100() {
    let (url, _st) = start_stub().await;
    let dir = tempfile::tempdir().unwrap();
    let s = spec(Some(sha(&body_bytes())));
    let mut seen: Vec<(f64, Option<String>)> = vec![];
    let mut cb = |p: f64, m: Option<&str>| seen.push((p, m.map(str::to_string)));
    let path = download_with(
        &s,
        None,
        dir.path(),
        &url,
        &mut cb,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(path.exists());
    assert!(!dir.path().join("ggml-test.bin.part").exists());
    assert_eq!(seen.last().unwrap().0, 100.0);
    assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "{seen:?}");
    assert!(seen.len() >= 2, "{seen:?}");
    let msg = seen.iter().find_map(|(_, m)| m.clone()).unwrap();
    assert!(msg.contains("MB/s") && msg.contains("/5 MB"), "{msg}");
    assert_eq!(list_downloaded_specs(dir.path(), &[s]), vec!["test"]);
}

#[tokio::test]
async fn cancelled_download_leaves_no_model() {
    let (url, st) = start_stub().await;
    st.stall_at.store(SIZE / 2, Ordering::SeqCst);
    let dir = tempfile::tempdir().unwrap();
    let s = spec(Some(sha(&body_bytes())));
    let cancel = CancellationToken::new();
    cancel_at_half(&st, &cancel);
    let mut cb = |_: f64, _: Option<&str>| {};
    let r = download_with(&s, None, dir.path(), &url, &mut cb, cancel).await;
    assert!(matches!(r, Err(WhisperError::Cancelled)), "{r:?}");
    assert!(list_downloaded_specs(dir.path(), &[s]).is_empty());
    assert!(list_downloaded(dir.path()).is_empty());
    let part = dir.path().join("ggml-test.bin.part");
    assert!(part.exists());
    assert!(std::fs::metadata(part).unwrap().len() < SIZE as u64);
}

#[tokio::test]
async fn resume_appends_from_partial() {
    let (url, st) = start_stub().await;
    st.stall_at.store(SIZE / 2, Ordering::SeqCst);
    let dir = tempfile::tempdir().unwrap();
    let s = spec(Some(sha(&body_bytes())));
    let cancel = CancellationToken::new();
    cancel_at_half(&st, &cancel);
    let mut cb = |_: f64, _: Option<&str>| {};
    let _ = download_with(&s, None, dir.path(), &url, &mut cb, cancel).await;
    let part_len = std::fs::metadata(dir.path().join("ggml-test.bin.part"))
        .unwrap()
        .len();
    assert!(part_len > 0 && part_len < SIZE as u64);

    st.stall_at.store(usize::MAX, Ordering::SeqCst);
    let path = download_with(
        &s,
        None,
        dir.path(),
        &url,
        &mut cb,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        st.ranges.lock().unwrap().as_slice(),
        [format!("bytes={part_len}-")]
    );
    assert_eq!(std::fs::read(path).unwrap(), body_bytes());
}

#[tokio::test]
async fn checksum_mismatch_deletes_file() {
    let (url, _st) = start_stub().await;
    let dir = tempfile::tempdir().unwrap();
    let s = spec(Some(Box::leak("0".repeat(64).into_boxed_str())));
    let mut cb = |_: f64, _: Option<&str>| {};
    let r = download_with(
        &s,
        None,
        dir.path(),
        &url,
        &mut cb,
        CancellationToken::new(),
    )
    .await;
    let err = r.unwrap_err();
    assert!(matches!(err, WhisperError::Checksum { .. }));
    assert!(err.to_string().contains("ggml-test.bin"));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn vad_model_fetched_with_first_model() {
    let (url, st) = start_stub().await;
    let dir = tempfile::tempdir().unwrap();
    let s = spec(Some(sha(&body_bytes())));
    let vad = vad_spec();
    let mut cb = |_: f64, _: Option<&str>| {};
    download_with(
        &s,
        Some(&vad),
        dir.path(),
        &url,
        &mut cb,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(dir.path().join(vad.file).exists());
    assert_eq!(
        st.paths.lock().unwrap().as_slice(),
        ["/ggml-test.bin".to_string(), format!("/{}", vad.file)]
    );
    // The VAD model is not a whisper model.
    assert!(list_downloaded(dir.path()).is_empty());
}

#[test]
fn catalog_and_dirs() {
    assert_eq!(MODELS.len(), 11);
    assert!(MODELS
        .iter()
        .all(|m| m.sha256.is_some() && m.file == format!("ggml-{}.bin", m.name)));
    assert_ne!(default_cache_dir(), legacy_cache_dir());
    assert!(default_cache_dir().ends_with("subforge/whisper-ggml"));
    assert!(legacy_cache_dir().ends_with("subforge/whisper-models"));
}

#[tokio::test]
async fn unknown_model_is_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut cb = |_: f64, _: Option<&str>| {};
    let r = download(
        "nope",
        dir.path(),
        "http://x",
        &mut cb,
        CancellationToken::new(),
    )
    .await;
    assert!(matches!(r, Err(WhisperError::UnknownModel(_))));
}

fn write_part(dir: &std::path::Path, data: &[u8]) {
    std::fs::write(dir.join("ggml-test.bin.part"), data).unwrap();
}

async fn run_expect_ok(url: &str, dir: &std::path::Path) -> Vec<u8> {
    let s = spec(Some(sha(&body_bytes())));
    let mut cb = |_: f64, _: Option<&str>| {};
    let path = download_with(&s, None, dir, url, &mut cb, CancellationToken::new())
        .await
        .unwrap();
    assert!(!dir.join("ggml-test.bin.part").exists());
    std::fs::read(path).unwrap()
}

#[tokio::test]
async fn server_ignoring_range_restarts_not_doubles() {
    let (url, st) = start_stub().await;
    st.mode.store(1, Ordering::SeqCst);
    let dir = tempfile::tempdir().unwrap();
    write_part(dir.path(), &body_bytes()[..1024 * 1024]);
    let out = run_expect_ok(&url, dir.path()).await;
    assert_eq!(out.len(), SIZE);
    assert_eq!(out, body_bytes());
    assert_eq!(st.ranges.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn range_416_with_complete_part_completes() {
    let (url, st) = start_stub().await;
    let dir = tempfile::tempdir().unwrap();
    write_part(dir.path(), &body_bytes());
    let out = run_expect_ok(&url, dir.path()).await;
    assert_eq!(out, body_bytes());
    assert_eq!(
        st.sent.load(Ordering::SeqCst),
        0,
        "no body should be fetched"
    );
}

#[tokio::test]
async fn range_416_with_stale_part_restarts() {
    let (url, st) = start_stub().await;
    let dir = tempfile::tempdir().unwrap();
    write_part(dir.path(), &vec![7u8; SIZE + 10]);
    let out = run_expect_ok(&url, dir.path()).await;
    assert_eq!(out, body_bytes());
    assert_eq!(st.ranges.lock().unwrap().len(), 1);
    assert_eq!(st.paths.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn mismatched_content_range_restarts() {
    let (url, st) = start_stub().await;
    st.mode.store(2, Ordering::SeqCst);
    let dir = tempfile::tempdir().unwrap();
    write_part(dir.path(), &body_bytes()[..1024 * 1024]);
    let out = run_expect_ok(&url, dir.path()).await;
    assert_eq!(out, body_bytes());
    // Ranged request, then a plain restart (which mode 2 serves as a normal 200).
    assert_eq!(st.paths.lock().unwrap().len(), 2);
}
