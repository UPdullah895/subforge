use crate::AppState;
use axum::{
    extract::{ws::WebSocket, State, WebSocketUpgrade},
    response::IntoResponse,
};
use futures::StreamExt;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let mut rx = state.event_sender.subscribe();

    loop {
        tokio::select! {
            Ok(event) = rx.recv() => {
                let mut msg = serde_json::json!({
                    "type": event.event_type,
                    "payload": {
                        "job_id": event.job_id,
                        "status": event.status,
                        "progress": event.progress,
                        "message": event.message,
                        "stage": event.stage,
                        "output_path": event.output_path,
                    }
                });
                // Only failed jobs with raw tool/network output carry `details`.
                if let Some(details) = event.details {
                    msg["payload"]["details"] = details.into();
                }
                if socket.send(axum::extract::ws::Message::Text(msg.to_string().into())).await.is_err() {
                    break;
                }
            }
            Some(Ok(msg)) = socket.next() => {
                if let axum::extract::ws::Message::Close(_) = msg {
                    break;
                }
            }
            else => break,
        }
    }
}
