use crate::tools::translate;
use crate::{jobs, AppState};
use axum::{extract::Json, extract::Query, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

#[derive(Serialize)]
pub struct OllamaInfoResp {
    available: bool,
    models: Vec<String>,
}

pub async fn detect_ollama(
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<OllamaInfoResp>, StatusCode> {
    let host = params.get("host").cloned().unwrap_or_else(|| "http://localhost:11434".to_string());
    match translate::detect_ollama(&host).await {
        Ok(resp) => Ok(Json(OllamaInfoResp {
            available: true,
            models: resp.models.into_iter().map(|m| m.name).collect(),
        })),
        Err(_) => Ok(Json(OllamaInfoResp { available: false, models: vec![] })),
    }
}

#[derive(Serialize)]
pub struct LibreInfoResp {
    available: bool,
    languages: Vec<translate::LibreLanguage>,
}

pub async fn detect_libretranslate(
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<LibreInfoResp>, StatusCode> {
    let host = params.get("host").cloned().unwrap_or_else(|| "http://localhost:5000".to_string());
    match translate::detect_libretranslate(&host).await {
        Ok(langs) => Ok(Json(LibreInfoResp {
            available: true,
            languages: langs,
        })),
        Err(_) => Ok(Json(LibreInfoResp { available: false, languages: vec![] })),
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
) -> Result<Json<serde_json::Value>, StatusCode> {
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
    jobs::emit_event(&sender, jobs::JobEvent {
        event_type: "job-started".to_string(),
        job_id: job_id.clone(),
        status: None,
        progress: None,
        message: None,
    });

    let queue_clone = state.job_queue.clone();
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
        // Extract lines
        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, jobs::JobStatus::Running);
            q.update_progress(&job_id_clone, 5.0, Some("Extracting lines...".to_string()));
        }
        jobs::emit_event(&sender, jobs::JobEvent {
            event_type: "job-progress".to_string(),
            job_id: job_id_clone.clone(),
            status: None,
            progress: Some(5.0),
            message: Some("Extracting lines...".to_string()),
        });

        let lines = match translate::extract_lines(&req_input) {
            Ok(l) => l,
            Err(e) => {
                let mut q = queue_clone.lock().unwrap();
                q.update_status(&job_id_clone, jobs::JobStatus::Failed);
                jobs::emit_event(&sender, jobs::JobEvent {
                    event_type: "job-completed".to_string(),
                    job_id: job_id_clone,
                    status: Some(jobs::JobStatus::Failed),
                    progress: Some(0.0),
                    message: Some(e.to_string()),
                });
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
            let batch_start = batch_idx * batch_size;
            let batch_end = (batch_start + chunk.len()).min(total);
            let pct = 10 + ((batch_end as f64 / total as f64) * 80.0) as i32;

            {
                let mut q = queue_clone.lock().unwrap();
                q.update_progress(&job_id_clone, pct as f64, Some(format!("Translating lines {}-{}/{}", batch_start + 1, batch_end, total)));
            }
            jobs::emit_event(&sender, jobs::JobEvent {
                event_type: "job-progress".to_string(),
                job_id: job_id_clone.clone(),
                status: None,
                progress: Some(pct as f64),
                message: Some(format!("Translating lines {}-{}/{}", batch_start + 1, batch_end, total)),
            });

            let result = match req_engine.as_str() {
                "ollama" => {
                    let host = req_host.clone().unwrap_or_else(|| "http://localhost:11434".to_string());
                    let model = req_model.clone().unwrap_or_else(|| "llama3".to_string());
                    translate::translate_ollama(&host, &model, chunk, &req_source, &req_target).await
                }
                "libretranslate" => {
                    let host = req_host.clone().unwrap_or_else(|| "http://localhost:5000".to_string());
                    translate::translate_libretranslate(&host, chunk, &req_source, &req_target).await
                }
                "deepl" => {
                    let key = req_key.clone().unwrap_or_default();
                    translate::translate_deepl(&key, chunk, &req_source, &req_target).await
                }
                "openai" => {
                    let key = req_key.clone().unwrap_or_default();
                    let model = req_model.clone().unwrap_or_else(|| "gpt-4o-mini".to_string());
                    translate::translate_openai(&key, &model, chunk, &req_source, &req_target).await
                }
                _ => {
                    let mut q = queue_clone.lock().unwrap();
                    q.update_status(&job_id_clone, jobs::JobStatus::Failed);
                    jobs::emit_event(&sender, jobs::JobEvent {
                        event_type: "job-completed".to_string(),
                        job_id: job_id_clone,
                        status: Some(jobs::JobStatus::Failed),
                        progress: Some(0.0),
                        message: Some("Unknown engine".to_string()),
                    });
                    return;
                }
            };

            match result {
                Ok(batch_translated) => translated.extend(batch_translated),
                Err(e) => {
                    let mut q = queue_clone.lock().unwrap();
                    q.update_status(&job_id_clone, jobs::JobStatus::Failed);
                    jobs::emit_event(&sender, jobs::JobEvent {
                        event_type: "job-completed".to_string(),
                        job_id: job_id_clone,
                        status: Some(jobs::JobStatus::Failed),
                        progress: Some(0.0),
                        message: Some(e.to_string()),
                    });
                    return;
                }
            }
        }

        if translated.len() != total {
            while translated.len() < total {
                translated.push(lines[translated.len()].text.clone());
            }
            translated.truncate(total);
        }

        let temp_json = format!("/tmp/subforge_translate_{}.json", job_id_clone);
        if let Err(e) = fs::write(&temp_json, serde_json::to_string(&translated).unwrap()) {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, jobs::JobStatus::Failed);
            jobs::emit_event(&sender, jobs::JobEvent {
                event_type: "job-completed".to_string(),
                job_id: job_id_clone,
                status: Some(jobs::JobStatus::Failed),
                progress: Some(0.0),
                message: Some(e.to_string()),
            });
            return;
        }

        {
            let mut q = queue_clone.lock().unwrap();
            q.update_progress(&job_id_clone, 95.0, Some("Rebuilding subtitle file...".to_string()));
        }

        if let Err(e) = translate::rebuild_lines(&req_input, &temp_json, &req_output) {
            let _ = fs::remove_file(&temp_json);
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, jobs::JobStatus::Failed);
            jobs::emit_event(&sender, jobs::JobEvent {
                event_type: "job-completed".to_string(),
                job_id: job_id_clone,
                status: Some(jobs::JobStatus::Failed),
                progress: Some(0.0),
                message: Some(e.to_string()),
            });
            return;
        }

        let _ = fs::remove_file(&temp_json);

        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, jobs::JobStatus::Completed);
            q.update_progress(&job_id_clone, 100.0, Some(format!("Translated {} lines", total)));
        }
        jobs::emit_event(&sender, jobs::JobEvent {
            event_type: "job-completed".to_string(),
            job_id: job_id_clone,
            status: Some(jobs::JobStatus::Completed),
            progress: Some(100.0),
            message: Some(format!("Translated {} lines", total)),
        });
    });

    Ok(Json(serde_json::json!({ "job_id": job_id })))
}
