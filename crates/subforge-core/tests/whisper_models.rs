//! Download job tests against the local stub in `support` (supports Range, can stall).

mod support;

use std::sync::atomic::Ordering;

use subforge_core::whisper::{
    default_cache_dir, download, download_with, legacy_cache_dir, list_downloaded,
    list_downloaded_specs, resolve_cache_dir, ModelSpec, WhisperError, MODELS, VAD_MODEL,
};
use subforge_core::CancellationToken;
use support::*;

#[tokio::test]
async fn concurrent_downloads_share_one_vad_fetch() {
    let (url, st) = start_stub().await;
    let dir = tempfile::tempdir().unwrap();
    let a = spec(Some(sha(&body_bytes())));
    let b = ModelSpec {
        name: "test2",
        file: "ggml-test2.bin",
        ..a
    };
    let vad = vad_spec();
    let mut cb1 = |_: f64, _: Option<&str>| {};
    let mut cb2 = |_: f64, _: Option<&str>| {};
    let (r1, r2) = tokio::join!(
        download_with(
            &a,
            Some(&vad),
            dir.path(),
            &url,
            &mut cb1,
            CancellationToken::new()
        ),
        download_with(
            &b,
            Some(&vad),
            dir.path(),
            &url,
            &mut cb2,
            CancellationToken::new()
        ),
    );
    r1.unwrap();
    r2.unwrap();
    let vad_gets = st
        .paths
        .lock()
        .unwrap()
        .iter()
        .filter(|p| p.ends_with(VAD_MODEL.file))
        .count();
    assert_eq!(
        vad_gets, 1,
        "the VAD model must be fetched by one download only"
    );
    assert!(dir.path().join(VAD_MODEL.file).is_file());
    assert!(!dir.path().join(format!("{}.part", VAD_MODEL.file)).exists());
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

#[test]
fn blank_cache_dir_means_default() {
    use std::path::Path;
    assert_eq!(resolve_cache_dir(None), default_cache_dir());
    assert_eq!(resolve_cache_dir(Some(Path::new(""))), default_cache_dir());
    assert_eq!(
        resolve_cache_dir(Some(Path::new("  "))),
        default_cache_dir()
    );
    assert_eq!(
        resolve_cache_dir(Some(Path::new("/x/my cache"))),
        Path::new("/x/my cache")
    );
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
