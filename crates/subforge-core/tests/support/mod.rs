//! Shared test support: a local HTTP stub for model downloads (supports Range, can stall),
//! plus model specs whose checksums match its body.
#![allow(dead_code)]

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
use subforge_core::whisper::{ModelSpec, VAD_MODEL};
use subforge_core::CancellationToken;

pub const SIZE: usize = 5 * 1024 * 1024;
pub const CHUNK: usize = 64 * 1024;

pub struct Stub {
    pub body: Vec<u8>,
    /// Bytes sent so far across all responses.
    pub sent: AtomicUsize,
    /// A response stalls (never finishes) once it has sent this many bytes.
    pub stall_at: AtomicUsize,
    /// 0 normal, 1 ignore Range (always 200), 2 answer 206 with the wrong Content-Range start.
    pub mode: AtomicUsize,
    pub ranges: Mutex<Vec<String>>,
    pub paths: Mutex<Vec<String>>,
}

pub fn body_bytes() -> Vec<u8> {
    (0..SIZE).map(|i| (i % 251) as u8).collect()
}

pub async fn serve(
    State(st): State<Arc<Stub>>,
    uri: axum::http::Uri,
    headers: HeaderMap,
) -> Response {
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

pub async fn start_stub() -> (String, Arc<Stub>) {
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

pub fn sha(data: &[u8]) -> &'static str {
    Box::leak(format!("{:x}", Sha256::digest(data)).into_boxed_str())
}

pub fn spec(sha256: Option<&'static str>) -> ModelSpec {
    ModelSpec {
        name: "test",
        file: "ggml-test.bin",
        size_mb: 5,
        size_bytes: Some(SIZE as u64),
        sha256,
    }
}

pub fn vad_spec() -> ModelSpec {
    ModelSpec {
        sha256: Some(sha(&body_bytes())),
        size_bytes: Some(SIZE as u64),
        ..VAD_MODEL
    }
}

/// Cancels `cancel` once the stub has sent half the body.
pub fn cancel_at_half(st: &Arc<Stub>, cancel: &CancellationToken) {
    let (st, cancel) = (st.clone(), cancel.clone());
    tokio::spawn(async move {
        while st.sent.load(Ordering::SeqCst) < SIZE / 2 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        cancel.cancel();
    });
}
