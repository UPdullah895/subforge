//! Streamable HTTP transport for the MCP server.
//!
//! The spec allows a server to answer a POSTed JSON-RPC message with a single
//! `application/json` body instead of an SSE stream; every tool here is synchronous, so that
//! is what this does. Notifications get `202 Accepted` and no body. A request that cannot be
//! parsed still answers HTTP 200 with a JSON-RPC error object — transport-level failures are
//! the only thing that turns into a non-200.

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{body::Bytes, Router};
use std::net::SocketAddr;

use crate::jsonrpc;

/// One endpoint hosts the whole protocol, so a client only needs the URL.
pub const DEFAULT_PATH: &str = "/mcp";

pub fn router() -> Router {
    Router::new()
        .route(DEFAULT_PATH, post(handle))
        .route("/health", get(health))
}

async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        br#"{"status":"ok","server":"subforge-mcp"}"#.to_vec(),
    )
}

/// Origins a browser may send. The spec requires validating `Origin` so a web page cannot
/// drive the server through DNS rebinding: `http://localhost:8091` is fine, `http://evil.example`
/// resolving to 127.0.0.1 is not. Non-browser clients (CLI, agents) send no Origin at all, which
/// is why absence is allowed and a foreign value is refused.
///
/// `SUBFORGE_MCP_ALLOWED_ORIGINS` adds hosts (comma separated) for the case where the server is
/// fronted by a real web UI on another origin.
fn origin_allowed(origin: &str) -> bool {
    let extra: Vec<String> = std::env::var("SUBFORGE_MCP_ALLOWED_ORIGINS")
        .map(|list| {
            list.split(',')
                .map(|item| item.trim().to_ascii_lowercase())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default();
    origin_allowed_with(origin, &extra)
}

fn origin_allowed_with(origin: &str, extra: &[String]) -> bool {
    let origin = origin.trim().to_ascii_lowercase();
    if extra.iter().any(|allowed| allowed == &origin) {
        return true;
    }
    // Compare the host only: the port a page was served from says nothing about safety.
    let after_scheme = origin
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(&origin);
    let host_and_port = after_scheme.split('/').next().unwrap_or(after_scheme);
    let host = if let Some(stripped) = host_and_port.strip_prefix('[') {
        stripped.split(']').next().unwrap_or(stripped)
    } else {
        host_and_port.split(':').next().unwrap_or(host_and_port)
    };
    matches!(
        host,
        "localhost" | "127.0.0.1" | "::1" | "0.0.0.0" | "[::1]"
    )
}

async fn handle(_state: State<()>, headers: HeaderMap, body: Bytes) -> Response {
    if let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        if !origin_allowed(origin) {
            return (
                StatusCode::FORBIDDEN,
                "this origin may not talk to the MCP endpoint (set SUBFORGE_MCP_ALLOWED_ORIGINS)",
            )
                .into_response();
        }
    }

    // MCP clients must accept JSON; be lenient if the header is missing entirely, but refuse
    // an explicit "text/event-stream only" request rather than answering in the wrong shape.
    let accepts_json = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.contains("application/json") || value.contains("*/*"))
        .unwrap_or(true);
    if !accepts_json {
        return (
            StatusCode::NOT_ACCEPTABLE,
            "this server answers with application/json",
        )
            .into_response();
    }

    let Ok(text) = std::str::from_utf8(&body) else {
        return json_response(
            StatusCode::OK,
            jsonrpc::Response::err(
                serde_json::Value::Null,
                jsonrpc::PARSE_ERROR,
                "body is not valid UTF-8",
            )
            .to_json(),
        );
    };

    let text = text.trim();
    if text.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "empty body: send one JSON-RPC message",
        )
            .into_response();
    }

    match respond(text) {
        Some(json) => json_response(StatusCode::OK, json),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

/// The transport-independent half: one line in, at most one response object out.
fn respond(body: &str) -> Option<serde_json::Value> {
    crate::handle_line(body).map(|response| response.to_json())
}

fn json_response(status: StatusCode, value: serde_json::Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        value.to_string(),
    )
        .into_response()
}

/// Serves until Ctrl-C. Blocking wrapper so `main` stays synchronous for the stdio mode.
pub fn serve(addr: SocketAddr) -> std::io::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        eprintln!("[subforge-mcp] listening on http://{addr}{DEFAULT_PATH}");
        axum::serve(listener, router())
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_request_is_answered_inline() {
        let value = respond(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
        assert_eq!(value["id"], 1);
        assert!(value["result"]["tools"].as_array().unwrap().len() >= 7);
    }

    #[test]
    fn a_notification_has_no_body() {
        assert!(respond(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    }

    #[test]
    fn broken_json_answers_with_a_jsonrpc_error_not_a_transport_failure() {
        let value = respond("{oops").unwrap();
        assert_eq!(value["error"]["code"], json!(-32700));
        assert_eq!(value["id"], serde_json::Value::Null);
    }

    #[test]
    fn a_foreign_origin_is_refused_but_loopback_is_not() {
        // The spec's DNS-rebinding rule: a page on any origin must not reach the endpoint.
        assert!(origin_allowed_with("http://localhost:3000", &[]));
        assert!(origin_allowed_with("http://127.0.0.1:8091", &[]));
        assert!(origin_allowed_with("https://[::1]:8443", &[]));
        assert!(origin_allowed_with("HTTP://LOCALHOST", &[]));

        assert!(!origin_allowed_with("http://evil.example", &[]));
        assert!(!origin_allowed_with("http://localhost.evil.example", &[]));
        assert!(!origin_allowed_with("null", &[]));

        // An operator can allow the origin their own UI is served from.
        let extra = vec!["https://media.example".to_string()];
        assert!(origin_allowed_with("https://media.example", &extra));
        assert!(!origin_allowed_with("https://other.example", &extra));
    }

    #[test]
    fn initialize_works_over_http_too() {
        let value =
            respond(r#"{"jsonrpc":"2.0","id":"a","method":"initialize","params":{}}"#).unwrap();
        assert_eq!(value["result"]["serverInfo"]["name"], "subforge-mcp");
    }
}
