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
mod error;
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
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024)) // 2 GB
        .with_state(state.clone());

    let host = std::env::var("SUBFORGE_HOST").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let addr: SocketAddr = host.parse().expect("Invalid SUBFORGE_HOST");

    println!("SubForge server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn api_routes() -> Router<AppState> {
    Router::new()
        // ── Health ──────────────────────────────────────────────────────────
        .route("/health", get(routes::health::health_check))
        // ── File upload / download / job status ─────────────────────────────
        .route("/upload", post(routes::upload::upload_file))
        .route("/download/{job_id}", get(routes::upload::download_file))
        .route("/jobs/{job_id}", get(routes::upload::get_job_status))
        // ── Tool registry ────────────────────────────────────────────────────
        .route("/tools/registry", get(routes::tools::get_registry))
        // ── Config ───────────────────────────────────────────────────────────
        .route(
            "/config",
            get(routes::config::load_config).post(routes::config::save_config),
        )
        // ── Subtitle operations ──────────────────────────────────────────────
        .route("/subtitle/enumerate", post(routes::subtitle::enumerate_tracks))
        .route("/subtitle/extract", post(routes::subtitle::extract_subtitle))
        .route("/subtitle/extract-all", post(routes::subtitle::extract_all_subtitles))
        .route("/subtitle/merge", post(routes::subtitle::merge_subtitle))
        .route("/subtitle/remove", post(routes::subtitle::remove_subtitles))
        .route("/subtitle/convert", post(routes::subtitle::convert_format))
        .route("/subtitle/shift", post(routes::subtitle::shift_timing))
        .route("/subtitle/meta", post(routes::subtitle::set_track_metadata))
        // ── Audio operations ─────────────────────────────────────────────────
        .route("/audio/tracks", post(routes::audio::list_audio_tracks))
        .route("/audio/extract", post(routes::audio::extract_audio_track))
        .route("/audio/merge", post(routes::audio::merge_audio_track))
        .route("/audio/remove", post(routes::audio::remove_audio_tracks))
        // ── Batch operations ─────────────────────────────────────────────────
        .route("/batch/scan", post(routes::batch::scan_directory))
        .route("/batch/extract", post(routes::batch::batch_extract_subtitles))
        // ── Transcription (Whisper) ──────────────────────────────────────────
        // GET for browser direct access; POST for apiInvoke() from frontend
        .route(
            "/transcribe/info",
            get(routes::transcribe::get_whisper_info)
                .post(routes::transcribe::get_whisper_info),
        )
        .route("/transcribe/models", post(routes::transcribe::list_whisper_models))
        .route("/transcribe/download", post(routes::transcribe::download_whisper_model))
        .route("/transcribe", post(routes::transcribe::transcribe_audio))
        // ── Translation ──────────────────────────────────────────────────────
        .route(
            "/translate/detect/ollama",
            post(routes::translate::detect_ollama),
        )
        .route(
            "/translate/detect/libretranslate",
            post(routes::translate::detect_libretranslate),
        )
        .route("/translate", post(routes::translate::translate_subtitles))
        // ── Rename (TMDB/TVmaze) ─────────────────────────────────────────────
        .route("/rename/parse", post(routes::rename::parse_filename))
        .route("/rename/search", post(routes::rename::search_series))
        .route("/rename/episodes", post(routes::rename::get_episodes))
        .route("/rename/preview", post(routes::rename::preview_rename))
        .route("/rename/apply", post(routes::rename::apply_rename))
        // ── Providers ─────────────────────────────────────────────────────────
        .route("/providers/test", post(routes::providers::test_provider_config))
}
