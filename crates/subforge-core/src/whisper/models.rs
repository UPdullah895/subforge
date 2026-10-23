//! Model catalog (names, sizes, checksums) and the resumable download job.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::progress::{CancellationToken, ProgressFn};
use crate::tools::ToolError;

/// Production base URL for ggml whisper models: `<base>/ggml-<name>.bin`.
pub const DEFAULT_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// Full URL of the Silero VAD GGML model (whisper.cpp VAD repository).
pub const VAD_URL: &str =
    "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin";

/// One downloadable model file. `size_bytes` / `sha256` come from Hugging Face LFS metadata.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ModelSpec {
    pub name: &'static str,
    pub file: &'static str,
    pub size_mb: u32,
    pub size_bytes: Option<u64>,
    pub sha256: Option<&'static str>,
}

/// Silero VAD model (not a whisper model; never listed by [`list_downloaded`]).
pub const VAD_MODEL: ModelSpec = ModelSpec {
    name: "silero-v5.1.2",
    file: "ggml-silero-v5.1.2.bin",
    size_mb: 1,
    size_bytes: Some(885098),
    sha256: Some("29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf"),
};

pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        name: "tiny",
        file: "ggml-tiny.bin",
        size_mb: 74,
        size_bytes: Some(77691713),
        sha256: Some("be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"),
    },
    ModelSpec {
        name: "tiny.en",
        file: "ggml-tiny.en.bin",
        size_mb: 74,
        size_bytes: Some(77704715),
        sha256: Some("921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f"),
    },
    ModelSpec {
        name: "base",
        file: "ggml-base.bin",
        size_mb: 141,
        size_bytes: Some(147951465),
        sha256: Some("60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"),
    },
    ModelSpec {
        name: "base.en",
        file: "ggml-base.en.bin",
        size_mb: 141,
        size_bytes: Some(147964211),
        sha256: Some("a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002"),
    },
    ModelSpec {
        name: "small",
        file: "ggml-small.bin",
        size_mb: 465,
        size_bytes: Some(487601967),
        sha256: Some("1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"),
    },
    ModelSpec {
        name: "small.en",
        file: "ggml-small.en.bin",
        size_mb: 465,
        size_bytes: Some(487614201),
        sha256: Some("c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d"),
    },
    ModelSpec {
        name: "medium",
        file: "ggml-medium.bin",
        size_mb: 1463,
        size_bytes: Some(1533763059),
        sha256: Some("6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208"),
    },
    ModelSpec {
        name: "medium.en",
        file: "ggml-medium.en.bin",
        size_mb: 1463,
        size_bytes: Some(1533774781),
        sha256: Some("cc37e93478338ec7700281a7ac30a10128929eb8f427dda2e865faa8f6da4356"),
    },
    ModelSpec {
        name: "large-v3",
        file: "ggml-large-v3.bin",
        size_mb: 2952,
        size_bytes: Some(3095033483),
        sha256: Some("64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2"),
    },
    ModelSpec {
        name: "large-v3-turbo",
        file: "ggml-large-v3-turbo.bin",
        size_mb: 1549,
        size_bytes: Some(1624555275),
        sha256: Some("1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69"),
    },
    ModelSpec {
        name: "large-v3-turbo-q5_0",
        file: "ggml-large-v3-turbo-q5_0.bin",
        size_mb: 547,
        size_bytes: Some(574041195),
        sha256: Some("394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2"),
    },
];

#[derive(Debug, thiserror::Error)]
pub enum WhisperError {
    #[error("Unknown whisper model '{0}'")]
    UnknownModel(String),
    #[error("{summary}")]
    Network { summary: String, details: String },
    #[error("Checksum mismatch for {file} \u{2014} the file was deleted; try again")]
    Checksum { file: String },
    #[error("Cancelled")]
    Cancelled,
    #[error("File error: {0}")]
    Io(#[from] io::Error),
    #[error("Whisper engine error: {0}")]
    Engine(String),
    #[error(transparent)]
    Tool(#[from] ToolError),
    #[error(
        "The whisper model '{model}' is not downloaded. Download it on the Transcribe page first."
    )]
    ModelNotDownloaded { model: String },
    #[error(
        "The voice activity (VAD) model {} is missing. Download a whisper model again (it fetches the VAD model too), or turn off the VAD filter.",
        path.display()
    )]
    VadModelMissing { path: PathBuf },
    #[error("No audio could be decoded from {}. Check that the file has an audio track.", path.display())]
    NoAudio { path: PathBuf },
    #[error("Could not write the subtitle file: {0}")]
    Subtitle(#[from] crate::subtitle::SubtitleError),
}

impl WhisperError {
    fn network(what: &str, err: impl std::fmt::Display) -> Self {
        WhisperError::Network {
            summary: format!("{what} failed: {err}. Check your internet connection and try again."),
            details: format!("{err}"),
        }
    }
}

/// `<cache dir>/subforge/whisper-ggml` (new directory; never collides with the legacy one).
pub fn default_cache_dir() -> PathBuf {
    base_dir().join("whisper-ggml")
}

/// `<cache dir>/subforge/whisper-models`, where the old faster-whisper models lived.
pub fn legacy_cache_dir() -> PathBuf {
    base_dir().join("whisper-models")
}

fn base_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("subforge")
}

pub fn spec_for(name: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|m| m.name == name)
}

pub fn model_path(cache: &Path, spec: &ModelSpec) -> PathBuf {
    cache.join(spec.file)
}

fn part_path(cache: &Path, spec: &ModelSpec) -> PathBuf {
    cache.join(format!("{}.part", spec.file))
}

/// Names of fully downloaded models: the `.bin` exists, no `.part` sits beside it and the size
/// matches when known.
pub fn list_downloaded(cache: &Path) -> Vec<String> {
    list_downloaded_specs(cache, MODELS)
}

/// [`list_downloaded`] over an arbitrary catalog.
pub fn list_downloaded_specs(cache: &Path, specs: &[ModelSpec]) -> Vec<String> {
    specs
        .iter()
        .filter(|s| is_complete(cache, s))
        .map(|s| s.name.to_string())
        .collect()
}

fn is_complete(cache: &Path, spec: &ModelSpec) -> bool {
    if part_path(cache, spec).exists() {
        return false;
    }
    match std::fs::metadata(model_path(cache, spec)) {
        Ok(md) if md.is_file() => spec.size_bytes.is_none_or(|n| n == md.len()),
        _ => false,
    }
}

/// Downloads `name` (and the VAD model if missing) into `cache`.
///
/// `base_url` is [`DEFAULT_BASE_URL`] in production. For any other base (tests), the VAD model is
/// fetched from `<base_url>/<VAD_MODEL.file>` instead of [`VAD_URL`].
pub async fn download(
    name: &str,
    cache: &Path,
    base_url: &str,
    progress: ProgressFn<'_>,
    cancel: CancellationToken,
) -> Result<PathBuf, WhisperError> {
    let spec = spec_for(name).ok_or_else(|| WhisperError::UnknownModel(name.to_string()))?;
    download_with(spec, Some(&VAD_MODEL), cache, base_url, progress, cancel).await
}

/// [`download`] for an explicit spec (and optional companion VAD spec).
pub async fn download_with(
    spec: &ModelSpec,
    vad: Option<&ModelSpec>,
    cache: &Path,
    base_url: &str,
    progress: ProgressFn<'_>,
    cancel: CancellationToken,
) -> Result<PathBuf, WhisperError> {
    tokio::fs::create_dir_all(cache).await?;
    let base = base_url.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| WhisperError::network("Download setup", e))?;
    let url = format!("{base}/{}", spec.file);
    let path = fetch(&client, spec, &url, cache, Some(progress), &cancel).await?;
    if let Some(vad) = vad {
        if !is_complete(cache, vad) {
            let vad_url = if base == DEFAULT_BASE_URL.trim_end_matches('/') {
                VAD_URL.to_string()
            } else {
                format!("{base}/{}", vad.file)
            };
            fetch(&client, vad, &vad_url, cache, None, &cancel).await?;
        }
    }
    Ok(path)
}

struct Reporter<'a> {
    cb: Option<ProgressFn<'a>>,
    started: Instant,
    last: Option<Instant>,
    base: u64,
    total: Option<u64>,
}

impl Reporter<'_> {
    fn emit(&mut self, done: u64, force: bool) {
        let Some(cb) = self.cb.as_mut() else { return };
        let now = Instant::now();
        if !force {
            if let Some(last) = self.last {
                if now.duration_since(last) < Duration::from_millis(100) {
                    return;
                }
            }
        }
        self.last = Some(now);
        let mb = |b: u64| b as f64 / 1_048_576.0;
        let secs = self.started.elapsed().as_secs_f64().max(0.001);
        let speed = mb(done.saturating_sub(self.base)) / secs;
        let pct = match self.total {
            Some(t) if t > 0 => (done as f64 / t as f64 * 100.0).min(99.9),
            _ => 0.0,
        };
        let pct = if force { 100.0 } else { pct };
        let msg = match self.total {
            Some(t) => format!("{:.0}/{:.0} MB \u{b7} {speed:.1} MB/s", mb(done), mb(t)),
            None => format!("{:.0} MB \u{b7} {speed:.1} MB/s", mb(done)),
        };
        cb(pct, Some(&msg));
    }
}

async fn hash_file(
    path: &Path,
    hasher: &mut Sha256,
    cancel: &CancellationToken,
) -> Result<(), WhisperError> {
    let mut f = tokio::fs::File::open(path).await?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.is_cancelled() {
            return Err(WhisperError::Cancelled);
        }
        let n = f.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        hasher.update(&buf[..n]);
    }
}

/// Start offset from a `Content-Range: bytes <start>-<end>/<total>` header.
fn content_range_start(resp: &reqwest::Response) -> Option<u64> {
    let v = resp
        .headers()
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?;
    v.strip_prefix("bytes ")?
        .split('-')
        .next()?
        .trim()
        .parse()
        .ok()
}

async fn fetch(
    client: &reqwest::Client,
    spec: &ModelSpec,
    url: &str,
    cache: &Path,
    progress: Option<ProgressFn<'_>>,
    cancel: &CancellationToken,
) -> Result<PathBuf, WhisperError> {
    let final_path = model_path(cache, spec);
    if is_complete(cache, spec) {
        if let Some(cb) = progress {
            cb(100.0, None);
        }
        return Ok(final_path);
    }
    let part = part_path(cache, spec);
    let what = format!("Download of model '{}'", spec.name);

    let mut offset = tokio::fs::metadata(&part)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    let mut req = client.get(url);
    if offset > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }
    let mut resp = tokio::select! {
        r = req.send() => r.map_err(|e| WhisperError::network(&what, e))?,
        _ = cancel.cancelled() => return Err(WhisperError::Cancelled),
    };

    let mut append = false;
    if resp.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        if offset > 0 && spec.size_bytes == Some(offset) {
            // The partial file is already complete; just verify it.
            let mut hasher = Sha256::new();
            hash_file(&part, &mut hasher, cancel).await?;
            verify_and_commit(spec, hasher, &part, &final_path).await?;
            if let Some(cb) = progress {
                cb(100.0, None);
            }
            return Ok(final_path);
        }
        // Stale partial: restart from scratch.
        let _ = tokio::fs::remove_file(&part).await;
        offset = 0;
        resp = tokio::select! {
            r = client.get(url).send() => r.map_err(|e| WhisperError::network(&what, e))?,
            _ = cancel.cancelled() => return Err(WhisperError::Cancelled),
        };
    } else if resp.status() == reqwest::StatusCode::PARTIAL_CONTENT && offset > 0 {
        if content_range_start(&resp) == Some(offset) {
            append = true;
        } else {
            // The server answered a different range than requested: restart from scratch.
            let _ = tokio::fs::remove_file(&part).await;
            offset = 0;
            resp = tokio::select! {
                r = client.get(url).send() => r.map_err(|e| WhisperError::network(&what, e))?,
                _ = cancel.cancelled() => return Err(WhisperError::Cancelled),
            };
        }
    } else {
        offset = 0;
    }
    if !resp.status().is_success() {
        return Err(WhisperError::Network {
            summary: format!(
                "{what} failed: the server answered HTTP {}. Try again later.",
                resp.status()
            ),
            details: format!("GET {url} -> {}", resp.status()),
        });
    }

    let total = resp
        .content_length()
        .map(|l| l + offset)
        .or(spec.size_bytes);

    let mut hasher = Sha256::new();
    let mut file = if append {
        hash_file(&part, &mut hasher, cancel).await?;
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(&part)
            .await?
    } else {
        tokio::fs::File::create(&part).await?
    };

    let mut rep = Reporter {
        cb: progress,
        started: Instant::now(),
        last: None,
        base: offset,
        total,
    };
    let mut done = offset;
    rep.emit(done, false);
    loop {
        if cancel.is_cancelled() {
            let _ = file.flush().await;
            return Err(WhisperError::Cancelled);
        }
        let chunk = tokio::select! {
            c = resp.chunk() => c.map_err(|e| WhisperError::network(&what, e))?,
            _ = cancel.cancelled() => {
                let _ = file.flush().await;
                return Err(WhisperError::Cancelled);
            }
        };
        let Some(chunk) = chunk else { break };
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        done += chunk.len() as u64;
        rep.emit(done, false);
    }
    file.flush().await?;
    file.sync_all().await?;
    drop(file);

    if let Some(expected) = total {
        if done != expected {
            return Err(WhisperError::Network {
                summary: format!(
                    "{what} failed: the connection closed early. Try again to resume."
                ),
                details: format!("received {done} of {expected} bytes"),
            });
        }
    }
    verify_and_commit(spec, hasher, &part, &final_path).await?;
    rep.emit(done, true);
    Ok(final_path)
}

async fn verify_and_commit(
    spec: &ModelSpec,
    hasher: Sha256,
    part: &Path,
    final_path: &Path,
) -> Result<(), WhisperError> {
    if let Some(expected) = spec.sha256 {
        let actual: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = tokio::fs::remove_file(part).await;
            return Err(WhisperError::Checksum {
                file: spec.file.to_string(),
            });
        }
    }
    tokio::fs::rename(part, final_path).await?;
    Ok(())
}
