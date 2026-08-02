use crate::tools::mkvtoolnix;
use crate::tools::translate;
use crate::{jobs, AppState};
use axum::{extract::Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize)]
pub struct DetectRequest {
    pub host: Option<String>,
}

#[derive(Serialize)]
pub struct OllamaInfoResp {
    available: bool,
    models: Vec<String>,
}

pub async fn detect_ollama(
    Json(req): Json<DetectRequest>,
) -> Result<Json<OllamaInfoResp>, StatusCode> {
    let host = req.host.unwrap_or_else(|| "http://localhost:11434".to_string());
    match translate::detect_ollama(&host).await {
        Ok(resp) => Ok(Json(OllamaInfoResp {
            available: true,
            models: resp.models.into_iter().map(|m| m.name).collect(),
        })),
        Err(_) => Ok(Json(OllamaInfoResp {
            available: false,
            models: vec![],
        })),
    }
}

#[derive(Serialize)]
pub struct LibreInfoResp {
    available: bool,
    languages: Vec<translate::LibreLanguage>,
}

pub async fn detect_libretranslate(
    Json(req): Json<DetectRequest>,
) -> Result<Json<LibreInfoResp>, StatusCode> {
    let host = req.host.unwrap_or_else(|| "http://localhost:5000".to_string());
    match translate::detect_libretranslate(&host).await {
        Ok(langs) => Ok(Json(LibreInfoResp {
            available: true,
            languages: langs,
        })),
        Err(_) => Ok(Json(LibreInfoResp {
            available: false,
            languages: vec![],
        })),
    }
}

#[derive(Deserialize)]
pub struct TranslateRequest {
    engine: String,
    input: String,
    output: String,
    source_lang: String,
    target_lang: String,
    host: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
}

pub async fn translate_subtitles(
    State(state): State<AppState>,
    Json(req): Json<TranslateRequest>,
) -> Result<Json<String>, StatusCode> {
    let job_id = format!("translate-{}", uuid::Uuid::new_v4());

    {
        let mut q = state.job_queue.lock().unwrap();
        q.add(jobs::Job {
            id: job_id.clone(),
            job_type: jobs::JobType::Translate,
            status: jobs::JobStatus::Pending,
            input_file: req.input.clone(),
            output_file: Some(req.output.clone()),
            progress: 0.0,
            message: Some("Queued".to_string()),
        });
    }

    let sender = state.event_sender.clone();
    jobs::emit_event(
        &sender,
        jobs::JobEvent {
            event_type: "job-started".to_string(),
            job_id: job_id.clone(),
            status: None,
            progress: None,
            message: None,
            stage: None,
            output_path: None,
        },
    );

    let queue_clone = state.job_queue.clone();
    let controls_clone = state.job_controls.clone();
    let sender_clone = state.event_sender.clone();
    let job_id_clone = job_id.clone();
    let req_engine = req.engine.clone();
    let req_input = req.input.clone();
    let req_output = req.output.clone();
    let req_source = req.source_lang.clone();
    let req_target = req.target_lang.clone();
    let req_host = req.host.clone();
    let req_model = req.model.clone();
    let req_key = req.api_key.clone();

    tokio::spawn(async move {
        use std::sync::Arc;
        let control = Arc::new(jobs::JobControl::new());
        {
            let mut registry = controls_clone.lock().unwrap();
            registry.insert(job_id_clone.clone(), control.clone());
        }
        // Extract lines
        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, jobs::JobStatus::Running);
            q.update_progress(&job_id_clone, 5.0, Some("Extracting lines...".to_string()));
        }
        jobs::emit_event(
            &sender,
            jobs::JobEvent {
                event_type: "job-progress".to_string(),
                job_id: job_id_clone.clone(),
                status: None,
                progress: Some(5.0),
                message: Some("Extracting lines...".to_string()),
                stage: None,
                output_path: None,
            },
        );

        let lines = match translate::extract_lines(&req_input) {
            Ok(l) => l,
            Err(e) => {
                let mut q = queue_clone.lock().unwrap();
                q.update_status(&job_id_clone, jobs::JobStatus::Failed);
                jobs::emit_event(
                    &sender,
                    jobs::JobEvent {
                        event_type: "job-completed".to_string(),
                        job_id: job_id_clone,
                        status: Some(jobs::JobStatus::Failed),
                        progress: Some(0.0),
                        message: Some(e.to_string()),
                        stage: None,
                        output_path: None,
                    },
                );
                return;
            }
        };

        let total = lines.len();
        let mut translated: Vec<String> = Vec::with_capacity(total);
        let batch_size = match req_engine.as_str() {
            "ollama" | "openai" => 25,
            _ => 10,
        };

        for (batch_idx, chunk) in lines.chunks(batch_size).enumerate() {
            control.wait_if_paused().await;
            if control.is_cancelled() {
                {
                    let mut q = queue_clone.lock().unwrap();
                    q.update_status(&job_id_clone, jobs::JobStatus::Cancelled);
                    q.update_progress(&job_id_clone, 0.0, Some("Translation cancelled".to_string()));
                }
                jobs::emit_event(
                    &sender_clone,
                    jobs::JobEvent {
                        event_type: "job-completed".to_string(),
                        job_id: job_id_clone.clone(),
                        status: Some(jobs::JobStatus::Cancelled),
                        progress: Some(0.0),
                        message: Some("Translation cancelled".to_string()),
                        stage: None,
                        output_path: None,
                    },
                );
                let mut registry = controls_clone.lock().unwrap();
                registry.remove(&job_id_clone);
                return;
            }

            let batch_start = batch_idx * batch_size;
            let batch_end = (batch_start + chunk.len()).min(total);
            let pct = 10 + ((batch_end as f64 / total as f64) * 80.0) as i32;

            {
                let mut q = queue_clone.lock().unwrap();
                q.update_progress(
                    &job_id_clone,
                    pct as f64,
                    Some(format!(
                        "Translating lines {}-{}/{}",
                        batch_start + 1,
                        batch_end,
                        total
                    )),
                );
            }
            jobs::emit_event(
                &sender,
                jobs::JobEvent {
                    event_type: "job-progress".to_string(),
                    job_id: job_id_clone.clone(),
                    status: None,
                    progress: Some(pct as f64),
                    message: Some(format!(
                        "Translating lines {}-{}/{}",
                        batch_start + 1,
                        batch_end,
                        total
                    )),
                    stage: None,
                    output_path: None,
                },
            );

            let result = match req_engine.as_str() {
                "ollama" => {
                    let host = req_host
                        .clone()
                        .unwrap_or_else(|| "http://localhost:11434".to_string());
                    let model = req_model.clone().unwrap_or_else(|| "llama3".to_string());
                    translate::translate_ollama(&host, &model, chunk, &req_source, &req_target)
                        .await
                }
                "libretranslate" => {
                    let host = req_host
                        .clone()
                        .unwrap_or_else(|| "http://localhost:5000".to_string());
                    translate::translate_libretranslate(&host, chunk, &req_source, &req_target)
                        .await
                }
                "deepl" => {
                    let key = req_key.clone().unwrap_or_default();
                    translate::translate_deepl(&key, chunk, &req_source, &req_target).await
                }
                "openai" => {
                    let key = req_key.clone().unwrap_or_default();
                    let model = req_model
                        .clone()
                        .unwrap_or_else(|| "gpt-4o-mini".to_string());
                    translate::translate_openai(&key, &model, chunk, &req_source, &req_target)
                        .await
                }
                "google" => {
                    let key = req_key.clone().unwrap_or_default();
                    translate::translate_google(&key, chunk, &req_source, &req_target).await
                }
                _ => {
                    let mut q = queue_clone.lock().unwrap();
                    q.update_status(&job_id_clone, jobs::JobStatus::Failed);
                    jobs::emit_event(
                        &sender_clone,
                        jobs::JobEvent {
                            event_type: "job-completed".to_string(),
                            job_id: job_id_clone.clone(),
                            status: Some(jobs::JobStatus::Failed),
                            progress: Some(0.0),
                            message: Some("Unknown engine".to_string()),
                            stage: None,
                            output_path: None,
                        },
                    );
                    let mut registry = controls_clone.lock().unwrap();
                    registry.remove(&job_id_clone);
                    return;
                }
            };

            match result {
                Ok(batch_translated) => translated.extend(batch_translated),
                Err(e) => {
                    let mut q = queue_clone.lock().unwrap();
                    q.update_status(&job_id_clone, jobs::JobStatus::Failed);
                    jobs::emit_event(
                        &sender_clone,
                        jobs::JobEvent {
                            event_type: "job-completed".to_string(),
                            job_id: job_id_clone.clone(),
                            status: Some(jobs::JobStatus::Failed),
                            progress: Some(0.0),
                            message: Some(e.to_string()),
                            stage: None,
                            output_path: None,
                        },
                    );
                    let mut registry = controls_clone.lock().unwrap();
                    registry.remove(&job_id_clone);
                    return;
                }
            }
        }

        // Pad to original count if any batch returned fewer lines
        if translated.len() != total {
            while translated.len() < total {
                translated.push(lines[translated.len()].text.clone());
            }
            translated.truncate(total);
        }

        // Write translations to a temp file for subtitle rebuild
        let temp_json = std::env::temp_dir()
            .join(format!("subforge_translate_{}.json", &job_id_clone));
        let temp_json_str = temp_json.to_string_lossy().to_string();

        if let Err(e) = fs::write(&temp_json, serde_json::to_string(&translated).unwrap()) {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, jobs::JobStatus::Failed);
            jobs::emit_event(
                &sender_clone,
                jobs::JobEvent {
                    event_type: "job-completed".to_string(),
                    job_id: job_id_clone.clone(),
                    status: Some(jobs::JobStatus::Failed),
                    progress: Some(0.0),
                    message: Some(e.to_string()),
                    stage: None,
                    output_path: None,
                },
            );
            return;
        }

        {
            let mut q = queue_clone.lock().unwrap();
            q.update_progress(
                &job_id_clone,
                95.0,
                Some("Rebuilding subtitle file...".to_string()),
            );
        }

        if let Err(e) = translate::rebuild_lines(&req_input, &temp_json_str, &req_output) {
            let _ = fs::remove_file(&temp_json);
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, jobs::JobStatus::Failed);
            jobs::emit_event(
                &sender,
                jobs::JobEvent {
                    event_type: "job-completed".to_string(),
                    job_id: job_id_clone.clone(),
                    status: Some(jobs::JobStatus::Failed),
                    progress: Some(0.0),
                    message: Some(e.to_string()),
                    stage: None,
                    output_path: None,
                },
            );
            let mut registry = controls_clone.lock().unwrap();
            registry.remove(&job_id_clone);
            return;
        }

        let _ = fs::remove_file(&temp_json);

        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, jobs::JobStatus::Completed);
            q.update_progress(
                &job_id_clone,
                100.0,
                Some(format!("Translated {} lines", total)),
            );
        }
        jobs::emit_event(
            &sender_clone,
            jobs::JobEvent {
                event_type: "job-completed".to_string(),
                job_id: job_id_clone.clone(),
                status: Some(jobs::JobStatus::Completed),
                progress: Some(100.0),
                message: Some(format!("Translated {} lines", total)),
                stage: None,
                output_path: None,
            },
        );

        let mut registry = controls_clone.lock().unwrap();
        registry.remove(&job_id_clone);
    });

    Ok(Json(job_id))
}

#[derive(Deserialize)]
pub struct PipelineRequest {
    video_path: String,
    track_id: u64,
    engine: String,
    source_lang: String,
    target_lang: String,
    host: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    auto_merge: bool,
    #[serde(default)]
    track_name: Option<String>,
    #[serde(default)]
    hearing_impaired: bool,
    #[serde(default)]
    set_default_subtitle: bool,
}

fn emit_stage(sender: &jobs::EventSender, job_id: &str, stage: &str, progress: i32, message: &str) {
    jobs::emit_event(
        sender,
        jobs::JobEvent {
            event_type: "job-progress".to_string(),
            job_id: job_id.to_string(),
            status: None,
            progress: Some(progress as f64),
            message: Some(message.to_string()),
            stage: Some(stage.to_string()),
            output_path: None,
        },
    );
}

/// Runs the full extract -> translate -> (optional) merge pipeline for a single
/// video's subtitle track, reusing the same per-engine translate:: functions and
/// mkvtoolnix extract/merge calls used elsewhere, rather than reimplementing them.
pub async fn run_translation_pipeline(
    State(state): State<AppState>,
    Json(req): Json<PipelineRequest>,
) -> Result<Json<String>, StatusCode> {
    let job_id = format!("pipeline-{}", uuid::Uuid::new_v4());

    {
        let mut q = state.job_queue.lock().unwrap();
        q.add(jobs::Job {
            id: job_id.clone(),
            job_type: jobs::JobType::Translate,
            status: jobs::JobStatus::Pending,
            input_file: req.video_path.clone(),
            output_file: None,
            progress: 0.0,
            message: Some("Queued".to_string()),
        });
    }

    let sender = state.event_sender.clone();
    jobs::emit_event(
        &sender,
        jobs::JobEvent {
            event_type: "job-started".to_string(),
            job_id: job_id.clone(),
            status: None,
            progress: None,
            message: None,
            stage: None,
            output_path: None,
        },
    );

    let queue_clone = state.job_queue.clone();
    let controls_clone = state.job_controls.clone();
    let job_id_clone = job_id.clone();
    let sender_clone = sender.clone();

    tokio::spawn(async move {
        use std::sync::Arc;
        let control = Arc::new(jobs::JobControl::new());
        {
            let mut registry = controls_clone.lock().unwrap();
            registry.insert(job_id_clone.clone(), control.clone());
        }

        macro_rules! fail {
            ($msg:expr) => {{
                let __msg: String = $msg;
                {
                    let mut q = queue_clone.lock().unwrap();
                    q.update_status(&job_id_clone, jobs::JobStatus::Failed);
                    q.update_progress(&job_id_clone, 0.0, Some(__msg.clone()));
                }
                jobs::emit_event(
                    &sender_clone,
                    jobs::JobEvent {
                        event_type: "job-completed".to_string(),
                        job_id: job_id_clone.clone(),
                        status: Some(jobs::JobStatus::Failed),
                        progress: Some(0.0),
                        message: Some(__msg),
                        stage: Some("error".to_string()),
                        output_path: None,
                    },
                );
                let mut registry = controls_clone.lock().unwrap();
                registry.remove(&job_id_clone);
                return;
            }};
        }

        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, jobs::JobStatus::Running);
        }

        let video_stem = PathBuf::from(&req.video_path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "subtitle".to_string());
        let video_dir = PathBuf::from(&req.video_path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        // Stage 1: extract
        emit_stage(&sender_clone, &job_id_clone, "extracting", 5, "Extracting subtitle track...");
        let extracted_path = std::env::temp_dir()
            .join(format!("subforge_pipeline_{}_extracted.srt", &job_id_clone))
            .to_string_lossy()
            .to_string();

        if let Err(e) = mkvtoolnix::extract_subtitle_track(&req.video_path, req.track_id, &extracted_path) {
            fail!(e.to_string());
        }

        // Stage 2: translate
        emit_stage(&sender_clone, &job_id_clone, "translating", 15, "Extracting lines...");
        let lines = match translate::extract_lines(&extracted_path) {
            Ok(l) => l,
            Err(e) => {
                let _ = fs::remove_file(&extracted_path);
                fail!(e.to_string());
            }
        };

        let total = lines.len();
        let mut translated: Vec<String> = Vec::with_capacity(total);
        let batch_size = match req.engine.as_str() {
            "ollama" | "openai" => 25,
            _ => 10,
        };

        for (batch_idx, chunk) in lines.chunks(batch_size).enumerate() {
            control.wait_if_paused().await;
            if control.is_cancelled() {
                {
                    let mut q = queue_clone.lock().unwrap();
                    q.update_status(&job_id_clone, jobs::JobStatus::Cancelled);
                    q.update_progress(&job_id_clone, 0.0, Some("Translation cancelled".to_string()));
                }
                jobs::emit_event(
                    &sender_clone,
                    jobs::JobEvent {
                        event_type: "job-completed".to_string(),
                        job_id: job_id_clone.clone(),
                        status: Some(jobs::JobStatus::Cancelled),
                        progress: Some(0.0),
                        message: Some("Translation cancelled".to_string()),
                        stage: Some("error".to_string()),
                        output_path: None,
                    },
                );
                let mut registry = controls_clone.lock().unwrap();
                registry.remove(&job_id_clone);
                return;
            }

            let batch_start = batch_idx * batch_size;
            let batch_end = (batch_start + chunk.len()).min(total);
            let pct = 15 + ((batch_end as f64 / total as f64) * 65.0) as i32;
            emit_stage(
                &sender_clone,
                &job_id_clone,
                "translating",
                pct,
                &format!("Translating lines {}-{}/{}", batch_start + 1, batch_end, total),
            );

            let result = match req.engine.as_str() {
                "ollama" => {
                    let host = req.host.clone().unwrap_or_else(|| "http://localhost:11434".to_string());
                    let model = req.model.clone().unwrap_or_else(|| "llama3".to_string());
                    translate::translate_ollama(&host, &model, chunk, &req.source_lang, &req.target_lang).await
                }
                "libretranslate" => {
                    let host = req.host.clone().unwrap_or_else(|| "http://localhost:5000".to_string());
                    translate::translate_libretranslate(&host, chunk, &req.source_lang, &req.target_lang).await
                }
                "deepl" => {
                    let key = req.api_key.clone().unwrap_or_default();
                    translate::translate_deepl(&key, chunk, &req.source_lang, &req.target_lang).await
                }
                "openai" => {
                    let key = req.api_key.clone().unwrap_or_default();
                    let model = req.model.clone().unwrap_or_else(|| "gpt-4o-mini".to_string());
                    translate::translate_openai(&key, &model, chunk, &req.source_lang, &req.target_lang).await
                }
                "google" => {
                    let key = req.api_key.clone().unwrap_or_default();
                    translate::translate_google(&key, chunk, &req.source_lang, &req.target_lang).await
                }
                _ => {
                    let _ = fs::remove_file(&extracted_path);
                    fail!("Unknown translation engine".to_string());
                }
            };

            match result {
                Ok(batch_translated) => translated.extend(batch_translated),
                Err(e) => {
                    let _ = fs::remove_file(&extracted_path);
                    fail!(e.to_string());
                }
            }
        }

        if translated.len() != total {
            while translated.len() < total {
                translated.push(lines[translated.len()].text.clone());
            }
            translated.truncate(total);
        }

        let temp_json = std::env::temp_dir().join(format!("subforge_pipeline_{}.json", &job_id_clone));
        let temp_json_str = temp_json.to_string_lossy().to_string();
        if let Err(e) = fs::write(&temp_json, serde_json::to_string(&translated).unwrap()) {
            let _ = fs::remove_file(&extracted_path);
            fail!(e.to_string());
        }

        let translated_srt = video_dir
            .join(format!("{}.{}.srt", video_stem, req.target_lang))
            .to_string_lossy()
            .to_string();

        emit_stage(&sender_clone, &job_id_clone, "translating", 82, "Rebuilding subtitle file...");
        if let Err(e) = translate::rebuild_lines(&extracted_path, &temp_json_str, &translated_srt) {
            let _ = fs::remove_file(&extracted_path);
            let _ = fs::remove_file(&temp_json);
            fail!(e.to_string());
        }
        let _ = fs::remove_file(&extracted_path);
        let _ = fs::remove_file(&temp_json);

        // Stage 3: merge (optional)
        control.wait_if_paused().await;
        if control.is_cancelled() {
            {
                let mut q = queue_clone.lock().unwrap();
                q.update_status(&job_id_clone, jobs::JobStatus::Cancelled);
                q.update_progress(&job_id_clone, 0.0, Some("Translation cancelled".to_string()));
            }
            jobs::emit_event(
                &sender_clone,
                jobs::JobEvent {
                    event_type: "job-completed".to_string(),
                    job_id: job_id_clone.clone(),
                    status: Some(jobs::JobStatus::Cancelled),
                    progress: Some(0.0),
                    message: Some("Translation cancelled".to_string()),
                    stage: Some("error".to_string()),
                    output_path: None,
                },
            );
            let mut registry = controls_clone.lock().unwrap();
            registry.remove(&job_id_clone);
            return;
        }

        let output_path = if req.auto_merge {
            emit_stage(&sender_clone, &job_id_clone, "merging", 90, "Merging translated subtitle into video...");
            let merged_path = video_dir
                .join(format!("{}.{}.mkv", video_stem, req.target_lang))
                .to_string_lossy()
                .to_string();
            let sub_spec = mkvtoolnix::SubtitleMergeSpec {
                path: translated_srt.clone(),
                language: Some(req.target_lang.clone()),
                track_name: req.track_name.clone(),
                hearing_impaired: req.hearing_impaired,
                make_default: req.set_default_subtitle,
            };
            if let Err(e) = mkvtoolnix::merge_subtitles_async(&req.video_path, &[sub_spec], &merged_path, Some(&control), |pct| {
                let merge_pct = 90.0 + (pct * 0.1);
                {
                    let mut q = queue_clone.lock().unwrap();
                    q.update_progress(&job_id_clone, merge_pct, None);
                }
                emit_stage(&sender_clone, &job_id_clone, "merging", merge_pct as i32, &format!("Merging... {}%", pct as i32));
            }).await {
                fail!(e.to_string());
            }
            merged_path
        } else {
            translated_srt.clone()
        };

        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, jobs::JobStatus::Completed);
            q.update_progress(&job_id_clone, 100.0, Some(format!("Saved to {}", output_path)));
        }
        jobs::emit_event(
            &sender_clone,
            jobs::JobEvent {
                event_type: "job-completed".to_string(),
                job_id: job_id_clone.clone(),
                status: Some(jobs::JobStatus::Completed),
                progress: Some(100.0),
                message: Some(format!("Saved to {}", output_path)),
                stage: Some("done".to_string()),
                output_path: Some(output_path),
            },
        );

        let mut registry = controls_clone.lock().unwrap();
        registry.remove(&job_id_clone);
    });

    Ok(Json(job_id))
}
