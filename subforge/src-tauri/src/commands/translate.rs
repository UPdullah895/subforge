use crate::jobs::{Job, JobStatus, JobType, SharedJobQueue};
use crate::tools::translate;
use serde::{Deserialize, Serialize};
use std::fs;
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaInfo {
    pub available: bool,
    pub models: Vec<String>,
}

#[tauri::command]
pub async fn detect_ollama(host: Option<String>) -> Result<OllamaInfo, String> {
    let host = host.unwrap_or_else(|| "http://localhost:11434".to_string());
    match translate::detect_ollama(&host).await {
        Ok(resp) => Ok(OllamaInfo {
            available: true,
            models: resp.models.into_iter().map(|m| m.name).collect(),
        }),
        Err(_) => Ok(OllamaInfo {
            available: false,
            models: vec![],
        }),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibreInfo {
    pub available: bool,
    pub languages: Vec<translate::LibreLanguage>,
}

#[tauri::command]
pub async fn detect_libretranslate(host: Option<String>) -> Result<LibreInfo, String> {
    let host = host.unwrap_or_else(|| "http://localhost:5000".to_string());
    match translate::detect_libretranslate(&host).await {
        Ok(langs) => Ok(LibreInfo {
            available: true,
            languages: langs,
        }),
        Err(_) => Ok(LibreInfo {
            available: false,
            languages: vec![],
        }),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateRequest {
    pub engine: String,
    pub input: String,
    pub output: String,
    pub source_lang: String,
    pub target_lang: String,
    pub host: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
}

#[tauri::command]
pub async fn translate_subtitles(
    app: AppHandle,
    queue: State<'_, SharedJobQueue>,
    request: TranslateRequest,
) -> Result<String, String> {
    let job_id = format!("translate-{}", uuid::Uuid::new_v4());

    {
        let mut q = queue.lock().map_err(|e| e.to_string())?;
        q.add(Job {
            id: job_id.clone(),
            job_type: JobType::Translate,
            status: JobStatus::Pending,
            input_file: request.input.clone(),
            output_file: Some(request.output.clone()),
            progress: 0.0,
            message: Some("Queued".to_string()),
        });
    }

    let _ = app.emit("job-started", &job_id);

    let app_clone = app.clone();
    let queue_clone = queue.inner().clone();
    let job_id_clone = job_id.clone();
    let req = request;

    tokio::spawn(async move {
        // Extract lines
        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, JobStatus::Running);
            q.update_progress(&job_id_clone, 5.0, Some("Extracting lines...".to_string()));
        }
        let _ = app_clone.emit("job-progress", serde_json::json!({
            "job_id": job_id_clone,
            "progress": 5,
            "message": "Extracting lines...",
        }));

        let lines = match translate::extract_lines(&req.input) {
            Ok(l) => l,
            Err(e) => {
                let mut q = queue_clone.lock().unwrap();
                q.update_status(&job_id_clone, JobStatus::Failed);
                q.update_progress(&job_id_clone, 0.0, Some(e.to_string()));
                let _ = app_clone.emit("job-completed", serde_json::json!({
                    "job_id": job_id_clone,
                    "status": "failed",
                    "message": e.to_string(),
                }));
                return;
            }
        };

        let total = lines.len();
        let mut translated: Vec<String> = Vec::with_capacity(total);

        // Process in batches
        let batch_size = match req.engine.as_str() {
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
            let _ = app_clone.emit("job-progress", serde_json::json!({
                "job_id": job_id_clone,
                "progress": pct,
                "message": format!("Translating lines {}-{}/{}", batch_start + 1, batch_end, total),
            }));

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
                    let mut q = queue_clone.lock().unwrap();
                    q.update_status(&job_id_clone, JobStatus::Failed);
                    q.update_progress(&job_id_clone, 0.0, Some("Unknown engine".to_string()));
                    let _ = app_clone.emit("job-completed", serde_json::json!({
                        "job_id": job_id_clone,
                        "status": "failed",
                        "message": "Unknown translation engine",
                    }));
                    return;
                }
            };

            match result {
                Ok(batch_translated) => {
                    translated.extend(batch_translated);
                }
                Err(e) => {
                    let mut q = queue_clone.lock().unwrap();
                    q.update_status(&job_id_clone, JobStatus::Failed);
                    q.update_progress(&job_id_clone, 0.0, Some(e.to_string()));
                    let _ = app_clone.emit("job-completed", serde_json::json!({
                        "job_id": job_id_clone,
                        "status": "failed",
                        "message": e.to_string(),
                    }));
                    return;
                }
            }
        }

        // Ensure we have the right number of translations
        if translated.len() != total {
            // Pad with original text if mismatch
            while translated.len() < total {
                translated.push(lines[translated.len()].text.clone());
            }
            translated.truncate(total);
        }

        // Write translations to temp JSON
        let temp_json = std::env::temp_dir()
            .join(format!("subforge_translate_{}.json", &job_id_clone));
        let temp_json_str = temp_json.to_string_lossy().to_string();
        if let Err(e) = fs::write(&temp_json, serde_json::to_string(&translated).unwrap()) {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, JobStatus::Failed);
            q.update_progress(&job_id_clone, 0.0, Some(e.to_string()));
            let _ = app_clone.emit("job-completed", serde_json::json!({
                "job_id": job_id_clone,
                "status": "failed",
                "message": e.to_string(),
            }));
            return;
        }

        // Rebuild subtitle file
        {
            let mut q = queue_clone.lock().unwrap();
            q.update_progress(&job_id_clone, 95.0, Some("Rebuilding subtitle file...".to_string()));
        }
        let _ = app_clone.emit("job-progress", serde_json::json!({
            "job_id": job_id_clone,
            "progress": 95,
            "message": "Rebuilding subtitle file...",
        }));

        if let Err(e) = translate::rebuild_lines(&req.input, &temp_json_str, &req.output) {
            let _ = fs::remove_file(&temp_json);
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, JobStatus::Failed);
            q.update_progress(&job_id_clone, 0.0, Some(e.to_string()));
            let _ = app_clone.emit("job-completed", serde_json::json!({
                "job_id": job_id_clone,
                "status": "failed",
                "message": e.to_string(),
            }));
            return;
        }

        let _ = fs::remove_file(&temp_json);

        {
            let mut q = queue_clone.lock().unwrap();
            q.update_status(&job_id_clone, JobStatus::Completed);
            q.update_progress(&job_id_clone, 100.0, Some(format!("Translated {} lines", total)));
        }
        let _ = app_clone.emit("job-completed", serde_json::json!({
            "job_id": job_id_clone,
            "status": "completed",
            "message": format!("Translated {} lines", total),
        }));
    });

    Ok(job_id)
}
