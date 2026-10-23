use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use axum::http::{header::HeaderValue, request::Parts};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;

mod config;
mod error;
mod jobs;
mod routes;
#[cfg(test)]
mod tests;
mod tools;
mod ws;

use jobs::{create_event_channel, JobQueue, SharedJobQueue, JobControlRegistry};

#[derive(Clone)]
pub struct AppState {
    pub job_queue: SharedJobQueue,
    pub event_sender: jobs::EventSender,
    pub job_controls: JobControlRegistry,
}

#[tokio::main]
async fn main() {
    let job_queue: SharedJobQueue = Arc::new(Mutex::new(JobQueue::new()));
    let (event_sender, _event_receiver) = create_event_channel();
    let job_controls: JobControlRegistry = Arc::new(Mutex::new(HashMap::new()));

    let mut config = crate::config::AppConfig::load();
    if let Ok(key) = std::env::var("OPENSUBTITLES_API_KEY") {
        config.opensubtitles_api_key = Some(key);
    }
    if let Ok(username) = std::env::var("OPENSUBTITLES_USERNAME") {
        config.opensubtitles_username = Some(username);
    }
    if let Ok(password) = std::env::var("OPENSUBTITLES_PASSWORD") {
        config.opensubtitles_password = Some(password);
    }
    let _ = config.save();

    let state = AppState {
        job_queue,
        event_sender,
        job_controls,
    };

    let app = Router::new()
        .nest("/api", api_routes())
        .route("/ws", get(ws::ws_handler))
        .fallback_service(ServeDir::new("dist").append_index_html_on_directories(true))
        .layer(cors_layer())
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024)) // 2 GB
        .with_state(state.clone());

    let host = std::env::var("SUBFORGE_HOST").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
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
        // ── Job control (cancel / pause / resume) ────────────────────────────
        .route("/jobs/cancel", post(routes::jobs::cancel_job))
        .route("/jobs/pause", post(routes::jobs::pause_job))
        .route("/jobs/resume", post(routes::jobs::resume_job))
        // ── Filesystem browsing (web file picker) ────────────────────────────
        .route("/fs/list", get(routes::fs::list_dir))
        // ── Tool registry ────────────────────────────────────────────────────
        .route("/tools/registry", get(routes::tools::get_registry))
        .route("/tools/status", get(routes::tools::get_status))
        // ── Dependencies (startup check + one-click install) ─────────────────
        .route("/deps", get(routes::deps::get_deps))
        .route("/deps/install", post(routes::deps::install_dep))
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
        .route("/subtitle/stretch", post(routes::subtitle::stretch_timing))
        .route("/subtitle/meta", post(routes::subtitle::set_track_metadata))
        // ── Audio operations ─────────────────────────────────────────────────
        .route("/audio/tracks", post(routes::audio::list_audio_tracks))
        .route("/audio/extract", post(routes::audio::extract_audio_track))
        .route("/audio/merge", post(routes::audio::merge_audio_track))
        .route("/audio/remove", post(routes::audio::remove_audio_tracks))
        // ── Batch operations ─────────────────────────────────────────────────
        .route("/batch/scan", post(routes::batch::scan_directory))
        .route("/batch/extract", post(routes::batch::batch_extract_subtitles))
        .route("/batch/merge-subs", post(routes::batch::merge_external_subs))
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
        .route(
            "/translate/pipeline",
            post(routes::translate::run_translation_pipeline),
        )
        // ── Rename (TMDB/TVmaze) ─────────────────────────────────────────────
        .route("/rename/parse", post(routes::rename::parse_filename))
        .route("/rename/search", post(routes::rename::search_series))
        .route("/rename/episodes", post(routes::rename::get_episodes))
        .route("/rename/preview", post(routes::rename::preview_rename))
        .route("/rename/apply", post(routes::rename::apply_rename))
        // ── Providers ─────────────────────────────────────────────────────────
        .route("/providers/test", post(routes::providers::test_provider_config))
        // ── Online subtitle sources ─────────────────────────────────────────
        .route(
            "/subtitle-sources/search",
            post(routes::subtitle_sources::search_online_subtitle),
        )
        .route(
            "/subtitle-sources/opensubtitles/credentials",
            post(routes::subtitle_sources::set_opensubtitles_credentials),
        )
        .route(
            "/subtitle-sources/opensubtitles/clear",
            post(routes::subtitle_sources::clear_opensubtitles_credentials),
        )
        .route(
            "/subtitle-sources/opensubtitles/status",
            post(routes::subtitle_sources::get_opensubtitles_status),
        )
        .route(
            "/subtitle-sources/subsource/status",
            post(routes::subtitle_sources::check_subsource_status),
        )
}

/// The bundled frontend is served from this same origin, so cross-origin
/// access is only needed for local dev servers. Anything that isn't a
/// loopback origin is refused, which stops arbitrary websites from driving
/// the API from a visitor's browser.
fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin: &HeaderValue, _: &Parts| {
            let Ok(origin) = origin.to_str() else { return false };
            let Some(rest) = origin
                .strip_prefix("http://")
                .or_else(|| origin.strip_prefix("https://"))
            else {
                return false;
            };
            let host = if rest.starts_with("[::1]") {
                "[::1]"
            } else {
                rest.split(':').next().unwrap_or("")
            };
            matches!(host, "localhost" | "127.0.0.1" | "[::1]")
        }))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any)
}
