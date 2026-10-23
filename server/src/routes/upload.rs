use crate::AppState;
use axum::{
    extract::{Multipart, Path, State},
    http::{header, StatusCode},
    Json,
};
use serde_json::json;
use std::io::Write;
use std::path::PathBuf;
use uuid::Uuid;

pub async fn upload_file(mut multipart: Multipart) -> Result<Json<serde_json::Value>, StatusCode> {
    let upload_dir = std::env::temp_dir().join("subforge_uploads");
    std::fs::create_dir_all(&upload_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut file_path = None;

    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        if field.name().unwrap_or("") == "file" {
            let filename = field.file_name().unwrap_or("upload").to_string();
            let dest = upload_dir.join(format!("{}_{}", Uuid::new_v4(), filename));
            let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
            let mut file =
                std::fs::File::create(&dest).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            file.write_all(&data)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            file_path = Some(dest.to_string_lossy().to_string());
        }
    }

    match file_path {
        Some(path) => Ok(Json(json!({ "file_path": path }))),
        None => Err(StatusCode::BAD_REQUEST),
    }
}

pub async fn download_file(
    Path(job_id): Path<String>,
    State(state): State<AppState>,
) -> Result<(StatusCode, [(header::HeaderName, String); 1], Vec<u8>), StatusCode> {
    let path = {
        let queue = state.job_queue.lock().unwrap();
        let job = queue.get(&job_id).ok_or(StatusCode::NOT_FOUND)?;
        let output_path = job.output_file.as_ref().ok_or(StatusCode::NOT_FOUND)?;
        PathBuf::from(output_path)
    };

    if !path.exists() {
        return Err(StatusCode::NOT_FOUND);
    }

    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let filename = path.file_name().unwrap_or_default().to_string_lossy();

    Ok((
        StatusCode::OK,
        [(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )],
        bytes,
    ))
}

pub async fn get_job_status(
    Path(job_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let queue = state.job_queue.lock().unwrap();
    let job = queue.get(&job_id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(json!(job)))
}
