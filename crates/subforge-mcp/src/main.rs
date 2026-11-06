//! `subforge-mcp` — an MCP (Model Context Protocol) server for SubForge.
//!
//! It speaks the stdio transport: one JSON-RPC object per line on stdin, responses on stdout,
//! logs on stderr. An agent (Claude Desktop, an IDE, a script) lists the tools, calls them,
//! and gets JSON back. Tools map onto `subforge-core` directly — no shell strings, no parsing
//! of human output.
//!
//! There is deliberately no separate state: the same environment that gives the CLI its
//! tools works here (`SUBFORGE_BUNDLED_DIR`, or a `bin` directory next to the binary, or the
//! tools on `PATH`).

mod http;
mod jsonrpc;
mod tools;

use std::io::{BufRead, Write};

use serde_json::{json, Value};

use jsonrpc::{Incoming, Response, INVALID_PARAMS, METHOD_NOT_FOUND};

const SERVER_NAME: &str = "subforge-mcp";

enum Mode {
    Stdio,
    Http(std::net::SocketAddr),
}

/// `--http [addr]` (or `--http=addr`); everything else prints and exits.
fn parse_args(args: &[String]) -> Result<Mode, String> {
    let mut mode = Mode::Stdio;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--version" | "-V" => {
                println!("{} {}", SERVER_NAME, env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--list-tools" => {
                // Handy for debugging without an MCP client.
                println!("{}", serde_json::to_string_pretty(&tools::specs()).unwrap());
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!(
                    "{SERVER_NAME} {}\n\n\
                     MCP server. Default transport: stdio (one JSON-RPC message per line).\n\
                     \nOptions:\n  \
                     --http [ADDR]   serve streamable HTTP instead, POSTing to /mcp\n  \
                     \x20                (ADDR defaults to 127.0.0.1:8091, or use --http=ADDR)\n  \
                     --list-tools    print the tool table and exit\n  \
                     --version       print the version and exit\n\n\
                     Point SUBFORGE_BUNDLED_DIR at a directory holding mkvmerge/mkvextract/\n\
                     ffmpeg, or put them on PATH.",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            "--http" => {
                let addr = match args.get(index + 1).map(String::as_str) {
                    Some(next) if !next.starts_with('-') => {
                        index += 1;
                        next
                    }
                    _ => "127.0.0.1:8091",
                };
                mode = Mode::Http(
                    addr.parse()
                        .map_err(|e| format!("invalid address {addr:?}: {e}"))?,
                );
            }
            other if other.starts_with("--http=") => {
                let addr = &other["--http=".len()..];
                mode = Mode::Http(
                    addr.parse()
                        .map_err(|e| format!("invalid address {addr:?}: {e}"))?,
                );
            }
            other => return Err(format!("unknown argument: {other} (try --help)")),
        }
        index += 1;
    }
    Ok(mode)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&args) {
        Ok(Mode::Http(addr)) => {
            if let Err(error) = http::serve(addr) {
                eprintln!("[subforge-mcp] cannot serve on {addr}: {error}");
                std::process::exit(1);
            }
            return;
        }
        Ok(Mode::Stdio) => {}
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    }

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(response) = handle_line(line) {
            let encoded = serde_json::to_string(&response.to_json()).unwrap_or_else(|_| {
                r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"encoding failed"}}"#
                    .to_string()
            });
            if writeln!(stdout, "{encoded}").is_err() || stdout.flush().is_err() {
                break;
            }
        }
    }
}

/// Handles one JSON-RPC message; `None` means "notification, stay silent".
///
/// Shared by both transports: stdio calls it per line, HTTP per request body.
pub(crate) fn handle_line(line: &str) -> Option<Response> {
    let incoming = match jsonrpc::parse_line(line) {
        Ok(incoming) => incoming,
        Err(response) => return Some(response),
    };

    match incoming {
        Incoming::Notification { method, .. } => {
            // `notifications/initialized` and `notifications/cancelled` need no answer.
            eprintln!("[{SERVER_NAME}] notification: {method}");
            None
        }
        Incoming::Request { id, method, params } => Some(handle_request(id, &method, &params)),
    }
}

fn handle_request(id: Value, method: &str, params: &Value) -> Response {
    match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(Value::as_str);
            Response::ok(
                id,
                json!({
                    "protocolVersion": jsonrpc::negotiate_version(requested),
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
                    "instructions": "SubForge: inspect a media file with list_tracks, find \
                                     missing subtitle languages with find_missing_languages, \
                                     extract or mux subtitles, convert and retime .srt/.ass/.vtt."
                }),
            )
        }

        "ping" => Response::ok(id, json!({})),

        "tools/list" => Response::ok(id, tools::specs()),

        // Clients probe these even when the server advertises only tools.
        "resources/list" => Response::ok(id, json!({ "resources": [] })),
        "prompts/list" => Response::ok(id, json!({ "prompts": [] })),

        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Response::err(id, INVALID_PARAMS, "tools/call needs a `name`");
            };
            let empty = json!({});
            let arguments = params.get("arguments").unwrap_or(&empty);
            match tools::call(name, arguments) {
                Ok(data) => {
                    let text = serde_json::to_string_pretty(&data).unwrap_or_default();
                    Response::ok(
                        id,
                        json!({
                            "content": [{ "type": "text", "text": text }],
                            "isError": false
                        }),
                    )
                }
                Err(message) => Response::ok(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": message }],
                        "isError": true
                    }),
                ),
            }
        }

        other => Response::err(id, METHOD_NOT_FOUND, format!("unknown method: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: i64, method: &str, params: Value) -> Value {
        handle_request(json!(id), method, &params).to_json()
    }

    #[test]
    fn initialize_answers_with_a_version_and_tool_capability() {
        let response = request(
            1,
            "initialize",
            json!({ "protocolVersion": "2025-06-18", "capabilities": {} }),
        );
        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(
            response["result"]["capabilities"]["tools"]["listChanged"],
            false
        );
        assert_eq!(response["result"]["serverInfo"]["name"], SERVER_NAME);
    }

    #[test]
    fn an_unknown_protocol_version_still_answers_with_ours() {
        let response = request(2, "initialize", json!({ "protocolVersion": "1999-01-01" }));
        assert_eq!(
            response["result"]["protocolVersion"],
            jsonrpc::PROTOCOL_VERSION
        );
    }

    #[test]
    fn tools_list_is_not_empty_and_shaped_for_clients() {
        let response = request(3, "tools/list", json!({}));
        let tools = response["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|t| t["name"] == "list_tracks"));
    }

    #[test]
    fn a_tool_error_is_a_result_with_iserror_not_a_protocol_error() {
        let response = request(4, "tools/call", json!({ "name": "nope", "arguments": {} }));
        assert!(response.get("error").is_none());
        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("unknown tool"));
    }

    #[test]
    fn tools_call_without_a_name_is_invalid_params() {
        let response = request(5, "tools/call", json!({}));
        assert_eq!(response["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn unknown_methods_are_reported_with_the_spec_code() {
        let response = request(6, "does/not/exist", json!({}));
        assert_eq!(response["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn notifications_produce_no_output() {
        assert!(handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    }

    #[test]
    fn a_bad_line_produces_a_parse_error_response() {
        let response = handle_line("{oops").unwrap().to_json();
        assert_eq!(response["error"]["code"], jsonrpc::PARSE_ERROR);
    }
}
