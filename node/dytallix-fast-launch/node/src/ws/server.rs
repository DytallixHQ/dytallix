use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    Extension,
};
use futures::{SinkExt, StreamExt};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct WsHub {
    inner: Arc<Mutex<Vec<tokio::sync::mpsc::UnboundedSender<Message>>>>,
}
impl WsHub {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn broadcast_json(&self, v: &serde_json::Value) {
        let txt = Message::Text(v.to_string());
        let mut guard = self.inner.lock().unwrap();
        guard.retain(|tx| tx.send(txt.clone()).is_ok());
    }
}

pub async fn ws_handler(
    Extension(hub): Extension<WsHub>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(hub, socket))
}

async fn handle_socket(hub: WsHub, socket: WebSocket) {
    let (mut tx_ws, mut rx_ws) = socket.split();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    {
        hub.inner.lock().unwrap().push(tx);
    }
    let mut rx = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
    // Forward outbound messages manually
    tokio::spawn(async move {
        while let Some(msg) = rx.next().await {
            if let Err(e) = tx_ws.send(msg).await {
                eprintln!("ws send error {e}");
                break;
            }
        }
    });
    while let Some(_msg) = rx_ws.next().await { /* ignore client messages */ }
}
