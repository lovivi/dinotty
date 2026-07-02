//! Minimal relay outbound client for the Tauri desktop app.
//! Spawned when the user clicks "Connect" in the Remote tab.
//! Keeps an outbound WS to the relay, forwarding input frames
//! to the embedded dinotty-server's PTY.

use crate::session::SessionManager;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite;
use tracing::{info, warn};

type WsResult<T> = Result<T, tungstenite::Error>;

pub struct RelayClient {
    pub handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl RelayClient {
    pub const fn new() -> Self {
        Self { handle: Mutex::new(None) }
    }
}

/// Start the outbound relay client in a background tokio task.
/// Returns immediately; the task runs until cancelled or disconnected
/// (with auto-reconnect).
pub async fn spawn_relay(
    manager: Arc<SessionManager>,
    relay_url: String,
    password: String,
    desktop_id: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        let max_backoff = Duration::from_secs(30);

        loop {
            let ws_url = format!(
                "{}/relay/desktop/ws/{}",
                relay_url.trim_end_matches('/'),
                desktop_id
            );

            let (mut ws, err) = match connect_outbound(&ws_url, &password).await {
                Ok(ws) => (ws, false),
                Err(e) => {
                    warn!(?e, %ws_url, "relay connect failed; retry in {:?}", backoff);
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(max_backoff);
                    continue;
                }
            };

            // Send hello
            let hello = serde_json::json!({"v": 1, "desktop_id": desktop_id}).to_string();
            if ws.send(tungstenite::Message::Text(hello)).await.is_err() {
                warn!("relay hello failed; reconnecting");
                tokio::time::sleep(backoff).await;
                continue;
            }

            info!("relay outbound connected (desktop_id = {})", desktop_id);
            backoff = Duration::from_secs(1);

            // Read frames from relay; forward "input" frames to PTY
            while let Some(msg) = ws.next().await {
                match msg {
                    Ok(tungstenite::Message::Close(_)) => break,
                    Ok(m) => {
                        let raw = match &m {
                            tungstenite::Message::Text(t) => Some(t.as_str()),
                            tungstenite::Message::Binary(b) => std::str::from_utf8(b).ok(),
                            _ => None,
                        };
                        if let Some(json_str) = raw {
                            if let Ok(val) = serde_json::from_str::<Value>(json_str) {
                                if val.get("type").and_then(|v| v.as_str()) == Some("input") {
                                    if let Some(data) = val.get("data").and_then(|v| v.as_str()) {
                                        if let Ok(snap) = manager.active_pane_snapshot() {
                                            let pane_id = snap.pane_id;
                                            if let Some(entry) = manager.sessions.get(&pane_id) {
                                                if let Ok(mut w) = entry.writer.lock() {
                                                    use std::io::Write;
                                                    let _ = w.write_all(data.as_bytes());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }

            info!("relay disconnected; reconnecting in {:?}", backoff);
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(max_backoff);
        }
    })
}

async fn connect_outbound(
    ws_url: &str,
    password: &str,
) -> WsResult<tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>> {
    use tokio_tungstenite::MaybeTlsStream;
    use tokio_tungstenite::WebSocketStream;

    let (scheme, rest) = ws_url
        .strip_prefix("wss://")
        .map(|r| ("wss", r))
        .or_else(|| ws_url.strip_prefix("ws://").map(|r| ("ws", r)))
        .ok_or_else(|| tungstenite::Error::ConnectionClosed)?;

    let host_port = rest.split('/').next().unwrap_or(rest);
    let (host, port_str) = host_port.rsplit_once(':').unwrap_or((host_port, "80"));
    let port: u16 = port_str.parse().unwrap_or(80);

    let addr: std::net::SocketAddr = format!("{}:{}", host, port)
        .parse()
        .map_err(|_| tungstenite::Error::ConnectionClosed)?;

    let tcp = tokio::net::TcpStream::connect(addr)
        .await
        .map_err(|e| tungstenite::Error::Io(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, e)))?;

    let stream: MaybeTlsStream<tokio::net::TcpStream> = if scheme == "wss" {
        // Skip TLS for now (relay uses self-signed; trust on first use).
        // Use plain TCP to talk to relay's self-signed endpoint. The relay
        // serves both HTTP and WSS; we connect via plain WS.
        // For TLS we'd need tokio-rustls + cert pinning. Future work.
        MaybeTlsStream::Plain(tcp)
    } else {
        MaybeTlsStream::Plain(tcp)
    };

    let req = tungstenite::handshake::client::Request::builder()
        .method("GET")
        .uri(ws_url)
        .header("Authorization", format!("Bearer {}", password))
        .header("Host", format!("{}:{}", host, port))
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
        .header("Sec-WebSocket-Version", "13")
        .body(())
        .map_err(|e| tungstenite::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))?;

    let (ws, _) = tokio_tungstenite::client_async(req, stream)
        .await
        .map_err(|e| {
            tungstenite::Error::Io(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, e))
        })?;

    Ok(ws)
}
