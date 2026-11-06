//! JSON-RPC 2.0 plumbing for the MCP stdio transport.
//!
//! MCP over stdio is one JSON object per line on stdin and one per line on stdout; anything
//! that is not a response (logs, warnings) belongs on stderr. This module only deals with
//! framing and the error shapes the spec defines — the tool table lives in `tools`.

use serde_json::{json, Value};

/// Newest protocol revision this server implements. The client's requested revision is
/// echoed back when it is one we understand, which is what MCP requires of a server.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

/// A response ready to be written as one line.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub id: Value,
    pub result: Option<Value>,
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl Response {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }

    pub fn to_json(&self) -> Value {
        let mut object = json!({ "jsonrpc": "2.0", "id": self.id });
        if let Some(result) = &self.result {
            object["result"] = result.clone();
        }
        if let Some(error) = &self.error {
            object["error"] = json!({ "code": error.code, "message": error.message });
        }
        object
    }
}

/// What an incoming line turned out to be.
#[derive(Debug)]
pub enum Incoming {
    /// A request that expects a response.
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    /// A notification: handle it, say nothing.
    Notification { method: String },
}

/// Parses one line of stdin. `Err` is a JSON-RPC error to answer with (id null when the
/// request could not even be read).
pub fn parse_line(line: &str) -> Result<Incoming, Response> {
    let value: Value = serde_json::from_str(line)
        .map_err(|e| Response::err(Value::Null, PARSE_ERROR, format!("invalid JSON: {e}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| Response::err(Value::Null, INVALID_REQUEST, "expected a JSON object"))?;

    let method = object
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| Response::err(Value::Null, INVALID_REQUEST, "missing method"))?
        .to_string();
    match object.get("id") {
        Some(id) => Ok(Incoming::Request {
            id: id.clone(),
            method,
            params: object.get("params").cloned().unwrap_or(Value::Null),
        }),
        None => Ok(Incoming::Notification { method }),
    }
}

/// Picks the protocol revision to answer with: the client's when we implement it, ours
/// otherwise (MCP requires the server to answer with a revision it will actually speak).
pub fn negotiate_version(requested: Option<&str>) -> &'static str {
    match requested {
        Some("2025-03-26") => "2025-03-26",
        Some("2024-11-05") => "2024-11-05",
        _ => PROTOCOL_VERSION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_carries_its_id() {
        let incoming = parse_line(r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#).unwrap();
        match incoming {
            Incoming::Request { id, method, .. } => {
                assert_eq!(id, json!(7));
                assert_eq!(method, "ping");
            }
            _ => panic!("expected a request"),
        }
    }

    #[test]
    fn a_message_without_an_id_is_a_notification() {
        let incoming =
            parse_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).unwrap();
        assert!(matches!(incoming, Incoming::Notification { .. }));
    }

    #[test]
    fn broken_json_is_a_parse_error_with_a_null_id() {
        let error = parse_line("{not json").unwrap_err();
        assert_eq!(error.id, Value::Null);
        assert_eq!(error.error.unwrap().code, PARSE_ERROR);
    }

    #[test]
    fn a_request_without_a_method_is_invalid() {
        let error = parse_line(r#"{"jsonrpc":"2.0","id":1}"#).unwrap_err();
        assert_eq!(error.error.unwrap().code, INVALID_REQUEST);
    }

    #[test]
    fn responses_serialise_with_jsonrpc_and_no_nulls() {
        let json = Response::ok(json!("a"), json!({"ok": true})).to_json();
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["id"], "a");
        assert!(json.get("error").is_none());
    }
}
