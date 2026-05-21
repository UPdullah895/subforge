use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

mod config;
mod jobs;
mod routes;
mod tools;
mod ws;

use jobs::{create_event_channel, JobQueue, SharedJobQueue};

#[derive(Clone)]
pub struct AppState {
    pub job_queue: SharedJobQueue,
    pub event_sender: jobs::EventSender,
}

#[tokio::main]
async fn main() {
    let job_queue: SharedJobQueue = Arc::new(Mutex::new(JobQueue::new()));
    let (event_sender, _event_receiver) = create_event_channel();

    let state = AppState {
        job_queue,
        event_sender,
    };

    let app = Router::new()
        .nest("/api", api_routes())
        .route("/ws", get(ws::ws_handler))
        .fallback_service(ServeDir::new("dist").append_index_html_on_directories(true))
        .layer(CorsLayer::permissive())
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024)) // 2GB
        .with_state(state.clone());

    let host = std::env::var("SUBFORGE_HOST").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let addr: SocketAddr = host.parse().expect("Invalid SUBFORGE_HOST");

    println!("SubForge server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn api_routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(routes::health::health_check))
        .route("/upload", post(routes::upload::upload_file))
        .route("/download/{job_id}", get(routes::upload::download_file))
        .route("/jobs/{job_id}", get(routes::upload::get_job_status))
        .route("/subtitle/enumerate", post(routes::subtitle::enumerate_tracks))
        .route("/subtitle/extract", post(routes::subtitle::extract_subtitle))
        .route("/subtitle/merge", post(routes::subtitle::merge_subtitle))
        .route("/subtitle/remove", post(routes::subtitle::remove_subtitles))
        .route("/subtitle/convert", post(routes::subtitle::convert_format))
        .route("/subtitle/shift", post(routes::subtitle::shift_timing))
        .route("/audio/tracks", post(routes::audio::list_audio_tracks))
        .route("/audio/extract", post(routes::audio::extract_audio_track))
        .route("/audio/merge", post(routes::audio::merge_audio_track))
        .route("/audio/remove", post(routes::audio::remove_audio_tracks))
        .route("/batch/scan", post(routes::batch::scan_directory))
        .route("/transcribe/info", get(routes::transcribe::get_whisper_info))
        .route("/transcribe/models", get(routes::transcribe::list_whisper_models))
        .route("/transcribe/download", post(routes::transcribe::download_whisper_model))
        .route("/transcribe", post(routes::transcribe::transcribe_audio))
        .route("/translate/detect/ollama", get(routes::translate::detect_ollama))
        .route("/translate/detect/libretranslate", get(routes::translate::detect_libretranslate))
        .route("/translate", post(routes::translate::translate_subtitles))
}
