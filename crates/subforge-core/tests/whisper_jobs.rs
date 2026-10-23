//! Background job orchestration (download / transcription) through a recording `JobSink`.

mod support;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use subforge_core::tools::ToolPaths;
use subforge_core::whisper::{
    list_downloaded_specs, start_download_with, start_model_download, start_transcription,
    ActiveDownloads, JobKind, JobOutcome, JobSink, TranscribeOptions, WhisperError,
};
use subforge_core::CancellationToken;
use support::*;

#[derive(Debug, Clone, PartialEq)]
enum Ev {
    Started(JobKind, String, Option<String>),
    Running,
    Progress(f64),
    Finished(JobOutcome),
}

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<(String, Ev)>>,
    tokens: Mutex<HashMap<String, CancellationToken>>,
}

impl Recorder {
    fn of(&self, id: &str) -> Vec<Ev> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(j, _)| j == id)
            .map(|(_, e)| e.clone())
            .collect()
    }

    fn progress(&self, id: &str) -> Vec<f64> {
        self.of(id)
            .into_iter()
            .filter_map(|e| match e {
                Ev::Progress(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    async fn finished(&self, id: &str) -> JobOutcome {
        for _ in 0..500 {
            if let Some(Ev::Finished(o)) = self
                .of(id)
                .into_iter()
                .find(|e| matches!(e, Ev::Finished(_)))
            {
                return o;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("job {id} never finished: {:?}", self.of(id));
    }
}

impl JobSink for Recorder {
    fn started(
        &self,
        id: &str,
        kind: JobKind,
        input: &str,
        output: Option<&str>,
    ) -> CancellationToken {
        let token = CancellationToken::new();
        self.tokens.lock().unwrap().insert(id.into(), token.clone());
        self.events.lock().unwrap().push((
            id.into(),
            Ev::Started(kind, input.into(), output.map(str::to_string)),
        ));
        token
    }
    fn running(&self, id: &str) {
        self.events.lock().unwrap().push((id.into(), Ev::Running));
    }
    fn progress(&self, id: &str, pct: f64, _message: Option<&str>) {
        self.events
            .lock()
            .unwrap()
            .push((id.into(), Ev::Progress(pct)));
    }
    fn finished(&self, id: &str, outcome: &JobOutcome) {
        self.tokens.lock().unwrap().remove(id);
        self.events
            .lock()
            .unwrap()
            .push((id.into(), Ev::Finished(outcome.clone())));
    }
}

fn test_spec() -> subforge_core::whisper::ModelSpec {
    spec(Some(sha(&body_bytes())))
}

#[tokio::test]
async fn download_job_reports_progress_and_completes() {
    let (url, _st) = start_stub().await;
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let downloads = ActiveDownloads::default();
    let id = start_download_with(
        rec.clone(),
        &downloads,
        test_spec(),
        None,
        dir.path().into(),
        url,
    );
    assert!(id.starts_with("download-"), "{id}");
    // `started` ran before the id was returned, so a cancel can never miss the job.
    assert!(rec.tokens.lock().unwrap().contains_key(&id));

    let outcome = rec.finished(&id).await;
    let model = dir.path().join("ggml-test.bin");
    assert_eq!(
        outcome,
        JobOutcome::Completed {
            message: "Model 'test' downloaded".into(),
            output_path: Some(model.to_string_lossy().into_owned()),
        }
    );
    let events = rec.of(&id);
    assert_eq!(
        events[0],
        Ev::Started(
            JobKind::DownloadModel,
            "test".into(),
            Some(model.to_string_lossy().into_owned())
        )
    );
    assert_eq!(events[1], Ev::Running);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Ev::Finished(_)))
            .count(),
        1
    );
    let progress = rec.progress(&id);
    assert!(progress.len() >= 2, "{progress:?}");
    assert!(progress.windows(2).all(|w| w[0] <= w[1]), "{progress:?}");
    assert_eq!(*progress.last().unwrap(), 100.0);
    assert!(model.is_file());
    assert!(downloads.is_empty());
}

#[tokio::test]
async fn download_job_cancel_leaves_no_model() {
    let (url, st) = start_stub().await;
    st.stall_at.store(SIZE / 2, Ordering::SeqCst);
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let downloads = ActiveDownloads::default();
    let id = start_download_with(
        rec.clone(),
        &downloads,
        test_spec(),
        None,
        dir.path().into(),
        url,
    );
    while st.sent.load(Ordering::SeqCst) < SIZE / 2 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    rec.tokens.lock().unwrap()[&id].cancel();

    let outcome = rec.finished(&id).await;
    assert_eq!(outcome.status(), "cancelled", "{outcome:?}");
    assert!(list_downloaded_specs(dir.path(), &[test_spec()]).is_empty());
    assert!(!dir.path().join("ggml-test.bin").exists());
    assert!(downloads.is_empty());
}

#[tokio::test]
async fn second_download_of_same_model_joins_the_running_job() {
    let (url, st) = start_stub().await;
    st.stall_at.store(SIZE / 2, Ordering::SeqCst);
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let downloads = ActiveDownloads::default();
    let start = || {
        start_download_with(
            rec.clone(),
            &downloads,
            test_spec(),
            None,
            dir.path().into(),
            url.clone(),
        )
    };
    let first = start();
    let second = start();
    assert_eq!(first, second);
    let started = rec
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, e)| matches!(e, Ev::Started(..)))
        .count();
    assert_eq!(started, 1);
    // A different cache directory is a different download.
    let other = tempfile::tempdir().unwrap();
    let third = start_download_with(
        rec.clone(),
        &downloads,
        test_spec(),
        None,
        other.path().into(),
        url.clone(),
    );
    assert_ne!(first, third);
    for id in [&first, &third] {
        rec.tokens.lock().unwrap()[id.as_str()].cancel();
        assert_eq!(rec.finished(id).await.status(), "cancelled");
    }
    assert!(downloads.is_empty());
}

#[tokio::test]
async fn unknown_model_is_rejected_before_any_job() {
    let rec = Arc::new(Recorder::default());
    let err = start_model_download(rec.clone(), &ActiveDownloads::default(), "large-v9", None)
        .unwrap_err();
    assert!(matches!(err, WhisperError::UnknownModel(_)), "{err:?}");
    assert!(rec.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn transcription_job_without_model_fails_with_clear_message() {
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Recorder::default());
    let options: TranscribeOptions = serde_json::from_value(serde_json::json!({
        "input": dir.path().join("in.wav"),
        "output": dir.path().join("in.srt"),
        "model": "tiny",
        "device": "cuda",
        "language": null,
        "format": "srt",
        "word_timestamps": false,
        "vad_filter": true,
        "cache_dir": dir.path().join("cache"),
    }))
    .unwrap();
    let id = start_transcription(rec.clone(), ToolPaths::default, options);
    assert!(id.starts_with("transcribe-"), "{id}");
    let outcome = rec.finished(&id).await;
    assert_eq!(outcome.status(), "failed");
    assert!(outcome.message().contains("not downloaded"), "{outcome:?}");
    let in_path: PathBuf = dir.path().join("in.wav");
    assert_eq!(
        rec.of(&id)[0],
        Ev::Started(
            JobKind::Transcribe,
            in_path.to_string_lossy().into_owned(),
            Some(dir.path().join("in.srt").to_string_lossy().into_owned())
        )
    );
}
