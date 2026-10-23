//! Route tests driven through the real router with `oneshot`.

use super::*;
use crate::config::{AppConfig, TestConfigDir};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

fn test_app() -> Router {
    let (event_sender, _rx) = create_event_channel();
    let state = AppState {
        job_queue: Arc::new(Mutex::new(JobQueue::new())),
        event_sender,
        job_controls: Arc::new(Mutex::new(HashMap::new())),
        whisper_downloads: Default::default(),
    };
    Router::new().nest("/api", api_routes()).with_state(state)
}

async fn body_json(res: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn tools_status_reports_sources() {
    let _cfg = TestConfigDir::new();
    let bundled = std::env::temp_dir().join(format!("subforge_bundled_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&bundled).unwrap();
    std::fs::write(bundled.join("mkvmerge"), b"").unwrap();
    std::env::set_var("SUBFORGE_BUNDLED_DIR", &bundled);

    let missing = bundled.join("nope").join("mkvpropedit");
    let cfg = AppConfig {
        mkvpropedit_path: Some(missing.to_string_lossy().to_string()),
        ..AppConfig::default()
    };
    cfg.save().unwrap();

    let res = test_app()
        .oneshot(
            Request::get("/api/tools/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    std::env::remove_var("SUBFORGE_BUNDLED_DIR");
    let _ = std::fs::remove_dir_all(&bundled);

    assert_eq!(res.status(), StatusCode::OK);
    let json = body_json(res).await;
    let tools = json.as_array().unwrap();
    assert_eq!(tools.len(), 5, "{json}");
    let find = |id: &str| {
        tools
            .iter()
            .find(|t| t["id"] == id)
            .unwrap_or_else(|| panic!("{id} missing: {json}"))
    };
    assert_eq!(find("mkvmerge")["source"], "bundled");
    let propedit = find("mkvpropedit");
    assert!(propedit["source"].is_null(), "{propedit}");
    assert!(
        propedit["error"]
            .as_str()
            .unwrap()
            .contains("does not exist"),
        "{propedit}"
    );
}

#[tokio::test]
async fn save_config_never_lowers_config_version() {
    let _cfg = TestConfigDir::new();
    AppConfig {
        config_version: 3,
        ..AppConfig::default()
    }
    .save()
    .unwrap();

    // The web client omits `config_version`, which deserialises as 0.
    let mut body = serde_json::to_value(AppConfig::default()).unwrap();
    body.as_object_mut().unwrap().remove("config_version");
    body["theme"] = "light".into();
    let res = test_app()
        .oneshot(
            Request::post("/api/config")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let saved = AppConfig::load();
    assert_eq!(saved.theme, "light");
    assert_eq!(saved.config_version, 3);
}

#[tokio::test]
async fn whisper_info_and_catalog_routes() {
    let res = test_app()
        .oneshot(Request::get("/api/transcribe/info").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let info = body_json(res).await;
    assert!(info["engine"].as_str().unwrap().starts_with("whisper.cpp "), "{info}");
    assert!(info["gpu"].is_null(), "{info}");
    assert_eq!(info["devices"], serde_json::json!(["auto", "cpu"]));

    let res = test_app()
        .oneshot(Request::post("/api/transcribe/catalog").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let catalog = body_json(res).await;
    assert!(
        catalog
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["name"] == "base" && m["size_mb"] == 141),
        "{catalog}"
    );
}

#[tokio::test]
async fn whisper_download_rejects_unknown_model() {
    let res = test_app()
        .oneshot(
            Request::post("/api/transcribe/download")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"model":"large-v9","cache_dir":null}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let body = body_json(res).await;
    assert!(body["error"].as_str().unwrap().contains("large-v9"), "{body}");
}
