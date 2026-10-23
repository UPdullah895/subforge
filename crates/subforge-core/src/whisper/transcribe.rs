//! Speech-to-text with whisper.cpp (via `whisper-rs`), writing SRT or WebVTT.
//!
//! Audio is decoded by ffmpeg to 16 kHz mono `f32le` in memory, then run through whisper.cpp
//! in-process. [`transcribe`] blocks; async callers run it in `spawn_blocking`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use subforge_deps::HideWindow;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

use super::device::{self, gpu_failure_line, Backend, Device};
use super::models::{list_downloaded, model_path, resolve_cache_dir, spec_for, WhisperError};
use super::VAD_MODEL;
use crate::progress::{CancellationToken, ProgressFn};
use crate::subtitle::{Cue, Format, SubtitleDoc};
use crate::tools::{resolve, ToolError, ToolId, ToolPaths};

/// Output subtitle format.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutFormat {
    #[default]
    Srt,
    Vtt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscribeOptions {
    pub input: PathBuf,
    pub output: PathBuf,
    /// Catalog name, e.g. `"base"` (see [`super::MODELS`]).
    pub model: String,
    #[serde(default)]
    pub device: Device,
    /// ISO 639-1 code; `None`, `""` or `"auto"` detects the language.
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub format: OutFormat,
    /// One cue per word instead of one per segment.
    #[serde(default)]
    pub word_timestamps: bool,
    /// Skip non-speech with the Silero VAD model.
    #[serde(default)]
    pub vad_filter: bool,
    /// Model cache; `None` or blank means [`super::default_cache_dir`].
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscribeOutcome {
    pub device_used: String,
    /// Number of cues written.
    pub segments: usize,
    pub detected_language: Option<String>,
}

// Progress budget: decode 0-5, model load 5-10, whisper 10-95, write 95-100.
const PCT_LOAD: f64 = 5.0;
const PCT_RUN: f64 = 10.0;
const PCT_WRITE: f64 = 95.0;

/// Transcribes `opts.input` into `opts.output`. Blocks until done, failed or cancelled; a
/// cancelled run writes no output. Cancelling takes effect when whisper.cpp next checks its abort
/// callback, which is at encoder/decoder window boundaries, so the latency is up to one encoder
/// pass (a fraction of a second for small models, longer for large ones or on a slow CPU).
pub fn transcribe(
    paths: &ToolPaths,
    opts: &TranscribeOptions,
    progress: ProgressFn<'_>,
    cancel: CancellationToken,
) -> Result<TranscribeOutcome, WhisperError> {
    let check = || {
        if cancel.is_cancelled() {
            Err(WhisperError::Cancelled)
        } else {
            Ok(())
        }
    };
    check()?;
    silence_engine_logs();

    let spec =
        spec_for(&opts.model).ok_or_else(|| WhisperError::UnknownModel(opts.model.clone()))?;
    let cache = resolve_cache_dir(opts.cache_dir.as_deref());
    if !list_downloaded(&cache).iter().any(|m| m == spec.name) {
        return Err(WhisperError::ModelNotDownloaded {
            model: spec.name.to_string(),
        });
    }
    let vad_path = if opts.vad_filter {
        let p = model_path(&cache, &VAD_MODEL);
        if !p.is_file() {
            return Err(WhisperError::VadModelMissing { path: p });
        }
        Some(path_str(&p)?.to_owned())
    } else {
        None
    };
    let language = normalise_language(opts.language.as_deref())?;
    let (device, mut device_used) =
        device::select_device(opts.device, gpu_for(opts.device).as_deref());

    progress(0.0, Some("Decoding audio"));
    let pcm = decode_audio(paths, &opts.input, &cancel)?;
    check()?;

    progress(
        PCT_LOAD,
        Some(if device == Device::Gpu {
            "Preparing GPU (first run can take a few seconds)"
        } else {
            "Loading model"
        }),
    );
    let model_file = model_path(&cache, spec);
    let (_ctx, mut state) = match load_engine(&model_file, device == Device::Gpu) {
        Ok(loaded) => {
            if device == Device::Gpu && loaded.backend != Backend::Gpu {
                // whisper.cpp skips a GPU it cannot initialise and carries on with the CPU.
                let why = gpu_failure_line(&loaded.log).unwrap_or_else(|| "unknown".into());
                device_used = "cpu".to_string();
                progress(
                    PCT_LOAD,
                    Some(&format!("GPU unavailable ({why}); using the CPU")),
                );
            }
            (loaded.ctx, loaded.state)
        }
        Err((_, log)) if device == Device::Gpu && gpu_failure_line(&log).is_some() => {
            // Loading failed with GPU trouble in the log (e.g. out of VRAM): retry on the CPU.
            // A model-file error with no GPU involvement is returned as is (below).
            let why = gpu_failure_line(&log).unwrap_or_default();
            device_used = "cpu".to_string();
            progress(
                PCT_LOAD,
                Some(&format!("GPU unavailable ({why}); using the CPU")),
            );
            let loaded = load_engine(&model_file, false).map_err(|(e, _)| e)?;
            (loaded.ctx, loaded.state)
        }
        Err((err, _)) => return Err(err),
    };
    check()?;

    let mut params = FullParams::new(SamplingStrategy::BeamSearch {
        beam_size: 5,
        patience: -1.0,
    });
    params.set_n_threads(thread_count());
    params.set_language(language.as_deref());
    params.set_translate(false);
    params.set_suppress_nst(true);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_special(false);
    params.set_print_timestamps(false);
    if opts.word_timestamps {
        // One segment per word, as `whisper-cli -ml 1 -sow` does.
        params.set_token_timestamps(true);
        params.set_max_len(1);
        params.set_split_on_word(true);
    }
    if let Some(vad) = vad_path.as_deref() {
        // `enable_vad` panics unless the model path is set first.
        params.set_vad_model_path(Some(vad));
        params.enable_vad(true);
    }

    // whisper-rs callbacks must be 'static, so they talk to this call through shared atomics.
    let whisper_pct = Arc::new(AtomicI32::new(-1));
    let pct_writer = whisper_pct.clone();
    params.set_progress_callback_safe(move |p: i32| pct_writer.store(p, Ordering::Relaxed));
    let abort_token = cancel.clone();
    // whisper-rs 0.16's abort trampoline reads the user data as `F`, but stores a
    // `Box<dyn FnMut>`; passing a `Box<dyn FnMut>` as `F` makes the two agree.
    let abort: Box<dyn FnMut() -> bool> = Box::new(move || abort_token.is_cancelled());
    params.set_abort_callback_safe(abort);

    progress(PCT_RUN, Some("Transcribing"));
    let run = run_with_progress(&mut *progress, &whisper_pct, || state.full(params, &pcm));
    check()?;
    run.map_err(|e| WhisperError::Engine(format!("transcription failed ({e})")))?;

    progress(PCT_WRITE, Some("Writing subtitles"));
    let mut cues = Vec::new();
    for seg in state.as_iter() {
        let text = seg
            .to_str_lossy()
            .map_err(|e| WhisperError::Engine(format!("could not read a segment ({e})")))?;
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        // whisper.cpp timestamps are in centiseconds.
        let start = seg.start_timestamp().max(0) * 10;
        let end = (seg.end_timestamp() * 10).max(start);
        cues.push(Cue::new(start, end, text));
    }
    let detected_language =
        whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(str::to_owned);
    check()?;

    let format = match opts.format {
        OutFormat::Srt => Format::Srt,
        OutFormat::Vtt => Format::Vtt,
    };
    let segments = cues.len();
    SubtitleDoc::new(format, cues).save(&opts.output)?;
    progress(100.0, Some("Done"));

    Ok(TranscribeOutcome {
        device_used,
        segments,
        detected_language,
    })
}

/// The GPU to offer `requested`; a CPU request never touches the GPU stack.
fn gpu_for(requested: Device) -> Option<String> {
    (requested != Device::Cpu)
        .then(device::detect_gpu)
        .flatten()
}

struct Loaded {
    ctx: WhisperContext,
    state: WhisperState,
    /// What whisper.cpp actually initialised (it silently falls back to the CPU).
    backend: Backend,
    log: Vec<String>,
}

/// Loads the model and creates the inference state, asking for the GPU when `use_gpu`. On error
/// the engine log of the attempt comes back too.
fn load_engine(model_file: &Path, use_gpu: bool) -> Result<Loaded, (WhisperError, Vec<String>)> {
    // Backend registry and Vulkan instance setup are not thread-safe (see `ggml_init_lock`);
    // holding the lock also keeps the captured log to this load.
    let _guard = device::ggml_init_lock();
    device::clear_engine_log();
    let mut ctx_params = WhisperContextParameters::default();
    ctx_params.use_gpu(use_gpu);
    let result = WhisperContext::new_with_params(model_file, ctx_params)
        .map_err(|e| {
            WhisperError::Engine(format!(
                "could not load the model {} ({e}). Delete it and download it again.",
                model_file.display()
            ))
        })
        .and_then(|ctx| {
            let state = ctx
                .create_state()
                .map_err(|e| WhisperError::Engine(format!("could not start the engine ({e})")))?;
            Ok((ctx, state))
        });
    let log = device::engine_log();
    match result {
        Ok((ctx, state)) => Ok(Loaded {
            ctx,
            state,
            backend: device::backend_from_log(&log),
            log,
        }),
        Err(e) => Err((e, log)),
    }
}

/// Runs `f` while forwarding whisper's 0-100 progress (written to `pct` by its callback) to
/// `progress`, mapped onto the whisper part of the budget.
fn run_with_progress<T>(progress: ProgressFn<'_>, pct: &AtomicI32, f: impl FnOnce() -> T) -> T {
    let (done_tx, done_rx) = mpsc::channel::<()>();
    std::thread::scope(|s| {
        s.spawn(move || {
            let mut last = -1;
            loop {
                let finished = !matches!(
                    done_rx.recv_timeout(Duration::from_millis(100)),
                    Err(RecvTimeoutError::Timeout)
                );
                let p = pct.load(Ordering::Relaxed).clamp(-1, 100);
                if p > last {
                    last = p;
                    let mapped = PCT_RUN + (PCT_WRITE - PCT_RUN) * f64::from(p) / 100.0;
                    progress(mapped, Some("Transcribing"));
                }
                if finished {
                    break;
                }
            }
        });
        let out = f();
        drop(done_tx);
        out
    })
}

/// Decodes the first audio stream of `input` to 16 kHz mono `f32` samples with ffmpeg.
fn decode_audio(
    paths: &ToolPaths,
    input: &Path,
    cancel: &CancellationToken,
) -> Result<Vec<f32>, WhisperError> {
    let ffmpeg = resolve(ToolId::Ffmpeg, paths)?;
    let mut child = Command::new(&ffmpeg.path)
        .hide_window()
        .args(["-hide_banner", "-nostdin", "-v", "error", "-i"])
        .arg(input)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-f", "f32le", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(ToolError::Io)?;

    let mut stderr = child.stderr.take().expect("stderr is piped");
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    });

    let mut stdout = child.stdout.take().expect("stdout is piped");
    let mut bytes = Vec::new();
    let mut buf = vec![0u8; 1 << 16];
    let read_result = loop {
        if cancel.is_cancelled() {
            break Err(WhisperError::Cancelled);
        }
        match stdout.read(&mut buf) {
            Ok(0) => break Ok(()),
            Ok(n) => bytes.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => break Err(WhisperError::Tool(ToolError::Io(e))),
        }
    };
    if let Err(e) = read_result {
        let _ = child.kill();
        let _ = child.wait();
        let _ = stderr_reader.join();
        return Err(e);
    }
    let status = child.wait().map_err(ToolError::Io)?;
    let details = stderr_reader.join().unwrap_or_default();
    if !status.success() {
        return Err(ToolError::failed(ToolId::Ffmpeg, &status, details).into());
    }

    let (frames, _) = bytes.as_chunks::<4>();
    let samples: Vec<f32> = frames.iter().map(|b| f32::from_le_bytes(*b)).collect();
    if samples.is_empty() {
        return Err(WhisperError::NoAudio {
            path: input.to_path_buf(),
        });
    }
    Ok(samples)
}

/// `None` for auto-detection; otherwise a language code whisper.cpp knows.
fn normalise_language(lang: Option<&str>) -> Result<Option<String>, WhisperError> {
    let lang = lang.map(str::trim).unwrap_or("").to_ascii_lowercase();
    if lang.is_empty() || lang == "auto" {
        return Ok(None);
    }
    if lang.contains('\0') || whisper_rs::get_lang_id(&lang).is_none() {
        return Err(WhisperError::Engine(format!(
            "unknown language code '{lang}'. Use a code such as en, ar or fr, or leave it empty to detect the language."
        )));
    }
    Ok(Some(lang))
}

fn path_str(p: &Path) -> Result<&str, WhisperError> {
    p.to_str().ok_or_else(|| {
        WhisperError::Engine(format!(
            "the path {} is not valid Unicode; move the model cache to another folder.",
            p.display()
        ))
    })
}

fn thread_count() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8) as i32
}

/// Route whisper.cpp / GGML logging into the capture buffer in `device` instead of stderr.
fn silence_engine_logs() {
    device::install_log_capture();
}
