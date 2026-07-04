#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use dinotty_server::{
    agent, audit, auth, file_watcher, history, mcp, monitor, notification, openapi, plugin, proxy,
    qr_code, restore_state, session, settings, shell_profiles, tabs, token, webhook, workspace, ws,
};

use axum::{
    body::Body,
    extract::{ConnectInfo, Path, State},
    http::{header, HeaderValue, Response, StatusCode},
    middleware,
    response::{Html, IntoResponse},
    routing::{any, delete, get, post, put},
    Json, Router,
};
use rust_embed::Embed;
use std::fs;
use std::net::SocketAddr;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use std::collections::HashMap;
use std::sync::atomic;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

type StreamMap = Arc<
    Mutex<
        HashMap<
            String,
            tokio::sync::mpsc::UnboundedSender<tokio_tungstenite::tungstenite::Message>,
        >,
    >,
>;

use tower_http::cors::CorsLayer;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::file_watcher::FileWatcherState;
use crate::history::HistoryState;
use crate::monitor::MonitorState;
use crate::notification::NotificationBroadcast;
use crate::plugin::PluginManagerState;
use crate::session::SessionManager;
use crate::settings::SettingsState;

async fn index(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let content = StaticFiles::get("index.html").expect("index.html must exist in frontend/dist/");
    let html = String::from_utf8_lossy(&content.data);

    let stored_token = state.auth_token.read().await.clone();

    // Accept either ?code=xxx (one-time QR code) or ?token=xxx (direct token)
    let token_value = if let Some(code) = params.get("code") {
        state.qr_codes.consume(code).unwrap_or_default()
    } else {
        params
            .get("token")
            .filter(|t| urlencoding::decode(t).is_ok_and(|d| d == stored_token))
            .map(|_| stored_token)
            .unwrap_or_default()
    };

    let tag = format!("<meta name=\"auth-token\" content=\"{token_value}\">\n</head>");
    let html = html.replace("</head>", &tag);
    ([(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))], Html(html))
}

#[derive(Embed)]
#[folder = "frontend/dist/"]
pub struct StaticFiles;

#[derive(Clone, serde::Serialize)]
pub struct GitInfo {
    pub version: String,
    pub repo_url: String,
}

fn read_git_info() -> GitInfo {
    let lines: Vec<String> = fs::read_to_string("VERSION")
        .ok()
        .map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default();

    let version = lines
        .first()
        .cloned()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());

    let repo_url = lines
        .get(1)
        .cloned()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_REPOSITORY").to_string());

    GitInfo { version, repo_url }
}

#[derive(Clone)]
pub struct AppState {
    pub manager: Arc<SessionManager>,
    pub settings: SettingsState,
    pub file_watcher: Arc<FileWatcherState>,
    pub monitor: MonitorState,
    pub notifier: Arc<NotificationBroadcast>,
    pub history: HistoryState,
    pub auth_token: Arc<tokio::sync::RwLock<String>>,
    pub port: u16,
    pub plugins: PluginManagerState,
    pub git_info: GitInfo,
    pub tokens: token::TokenState,
    pub audit: audit::AuditState,
    pub agent: agent::AgentState,
    pub webhooks: webhook::WebhookState,
    pub mcp: mcp::transport::McpState,
    pub mcp_sse: Arc<mcp::transport::SseState>,
    pub qr_codes: Arc<qr_code::QrCodeState>,
}

// Allow extracting Arc<SessionManager> from AppState for ws handlers
impl axum::extract::FromRef<AppState> for Arc<SessionManager> {
    fn from_ref(state: &AppState) -> Self {
        state.manager.clone()
    }
}

// Allow extracting (Arc<SessionManager>, SettingsState) for settings handlers
impl axum::extract::FromRef<AppState> for (Arc<SessionManager>, SettingsState) {
    fn from_ref(state: &AppState) -> Self {
        (state.manager.clone(), state.settings.clone())
    }
}

// Allow extracting (Arc<SessionManager>, Arc<FileWatcherState>) for file watcher handlers
impl axum::extract::FromRef<AppState> for (Arc<SessionManager>, Arc<FileWatcherState>) {
    fn from_ref(state: &AppState) -> Self {
        (state.manager.clone(), state.file_watcher.clone())
    }
}

impl axum::extract::FromRef<AppState> for MonitorState {
    fn from_ref(state: &AppState) -> Self {
        state.monitor.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<NotificationBroadcast> {
    fn from_ref(state: &AppState) -> Self {
        state.notifier.clone()
    }
}

impl axum::extract::FromRef<AppState> for HistoryState {
    fn from_ref(state: &AppState) -> Self {
        state.history.clone()
    }
}

impl axum::extract::FromRef<AppState> for PluginManagerState {
    fn from_ref(state: &AppState) -> Self {
        state.plugins.clone()
    }
}

impl axum::extract::FromRef<AppState> for (PluginManagerState, Arc<SessionManager>) {
    fn from_ref(state: &AppState) -> Self {
        (state.plugins.clone(), state.manager.clone())
    }
}

impl axum::extract::FromRef<AppState> for token::TokenState {
    fn from_ref(state: &AppState) -> Self {
        state.tokens.clone()
    }
}

impl axum::extract::FromRef<AppState> for audit::AuditState {
    fn from_ref(state: &AppState) -> Self {
        state.audit.clone()
    }
}

impl axum::extract::FromRef<AppState> for agent::AgentState {
    fn from_ref(state: &AppState) -> Self {
        state.agent.clone()
    }
}

impl axum::extract::FromRef<AppState> for mcp::transport::McpState {
    fn from_ref(state: &AppState) -> Self {
        state.mcp.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<mcp::transport::SseState> {
    fn from_ref(state: &AppState) -> Self {
        state.mcp_sse.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<qr_code::QrCodeState> {
    fn from_ref(state: &AppState) -> Self {
        state.qr_codes.clone()
    }
}

async fn static_handler(Path(path): Path<String>) -> impl IntoResponse {
    let lookup = format!("assets/{path}");
    match StaticFiles::get(&lookup) {
        Some(content) => {
            let mime = mime_guess::from_path(&lookup).first_or_octet_stream();
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data.into_owned()))
                .unwrap()
        }
        None => {
            Response::builder().status(StatusCode::NOT_FOUND).body(Body::from("not found")).unwrap()
        }
    }
}

async fn manifest_handler() -> impl IntoResponse {
    match StaticFiles::get("manifest.json") {
        Some(content) => Response::builder()
            .header(header::CONTENT_TYPE, "application/manifest+json")
            .body(Body::from(content.data.into_owned()))
            .unwrap(),
        None => {
            Response::builder().status(StatusCode::NOT_FOUND).body(Body::from("not found")).unwrap()
        }
    }
}

async fn icon_handler(Path(path): Path<String>) -> impl IntoResponse {
    let lookup = format!("icons/{path}");
    match StaticFiles::get(&lookup) {
        Some(content) => {
            let mime = mime_guess::from_path(&lookup).first_or_octet_stream();
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .header(header::CACHE_CONTROL, "public, max-age=86400")
                .body(Body::from(content.data.into_owned()))
                .unwrap()
        }
        None => {
            Response::builder().status(StatusCode::NOT_FOUND).body(Body::from("not found")).unwrap()
        }
    }
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let raw: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < raw.len() {
        match raw[i].as_str() {
            "--port" | "-p" => {
                if let Some(v) = raw.get(i + 1) {
                    args.port = v.parse().expect("invalid port number");
                    i += 2;
                    continue;
                }
            }
            s if s.starts_with("--port=") => {
                args.port = s[7..].parse().expect("invalid port number");
                i += 1;
                continue;
            }
            "--relay-outbound" => {
                args.relay_url = raw.get(i + 1).cloned();
                args.relay_password = raw.get(i + 2).cloned();
                i += 3;
                continue;
            }
            "--relay-desktop-id" => {
                args.relay_desktop_id = raw.get(i + 1).cloned();
                i += 2;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    args
}

struct Args {
    port: u16,
    relay_url: Option<String>,
    relay_password: Option<String>,
    relay_desktop_id: Option<String>,
}

impl Default for Args {
    fn default() -> Self {
        Self { port: 8999, relay_url: None, relay_password: None, relay_desktop_id: None }
    }
}

/// Run the desktop's outbound relay client. When `--relay-outbound URL PASSWORD`
/// is passed, this Dinotty server opens an outbound WebSocket to the cloud
/// relay and pushes terminal screen state through it. v0 is read-only —
/// the desktop periodically snapshots the active pane and ships it; in v1
/// we'll bridge input frames the other way too.
async fn run_relay_outbound(
    relay_url: String,
    relay_password: String,
    desktop_id: String,
    manager: Arc<SessionManager>,
    shutdown: Arc<atomic::AtomicBool>,
    local_port: u16,
) {
    use futures_util::{SinkExt, StreamExt};
    use std::time::Duration;
    use tokio_tungstenite::tungstenite;

    info!(%relay_url, %desktop_id, "starting outbound relay client");

    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);

    // Track ws_bridge_proxy handles so we can abort them on reconnect.
    let bridge_handles: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>> =
        Arc::new(Mutex::new(Vec::new()));

    loop {
        // Abort any orphaned bridge proxy tasks from the previous reconnect
        // attempt before we create new ones.
        {
            let mut bh = bridge_handles.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            for h in bh.drain(..) {
                h.abort();
            }
        }

        if shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }

        let ws_url = format!("{}/relay/desktop/ws/{}", relay_url.trim_end_matches('/'), desktop_id);
        info!(%ws_url, "connecting to relay");

        let req = match tungstenite::handshake::client::Request::builder()
            .method("GET")
            .uri(&ws_url)
            .header("Authorization", format!("Bearer {}", relay_password))
            .header("Host", extract_host(&ws_url))
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
            .header("Sec-WebSocket-Version", "13")
            .body(())
        {
            Ok(r) => r,
            Err(e) => {
                warn!(?e, "failed to build ws request");
                sleep_or_shutdown(&shutdown, backoff).await;
                backoff = (backoff * 2).min(max_backoff);
                continue;
            }
        };

        let stream = match ws_connect_stream(ws_url.clone()).await {
            Ok(s) => s,
            Err(e) => {
                warn!(?e, "ws stream connect failed");
                sleep_or_shutdown(&shutdown, backoff).await;
                backoff = (backoff * 2).min(max_backoff);
                continue;
            }
        };
        let (ws, _response) = match tokio_tungstenite::client_async(req, stream).await {
            Ok(c) => c,
            Err(e) => {
                warn!(?e, "relay connect failed; will retry");
                sleep_or_shutdown(&shutdown, backoff).await;
                backoff = (backoff * 2).min(max_backoff);
                continue;
            }
        };

        info!("relay connected");
        backoff = Duration::from_secs(1);

        // First frame from desktop side: a hello JSON so the relay can
        // verify the desktop ID matches the registered slot.
        let mut ws = ws;
        let hello = serde_json::to_string(&serde_json::json!({"v": 1, "desktop_id": desktop_id}))
            .unwrap_or_default();
        if ws.send(tungstenite::Message::Text(hello)).await.is_err() {
            warn!("relay hello send failed; reconnecting");
            sleep_or_shutdown(&shutdown, backoff).await;
            continue;
        }

        // v0: read-only outbound. We split the WS and run two tasks:
        //   - reader: drains incoming frames (server → desktop). v0 has no
        //     such frames; we just keep the channel open so the connection
        //     stays alive.
        //   - screen ticker: every 200ms, snapshot the active pane's
        //     screen and push it as a Text frame tagged with the pane_id
        //     so the relay/mobile can route it.
        let (mut ws_tx, mut ws_rx) = ws.split();
        let manager_for_reader = manager.clone();
        let desktop_id_for_reader = desktop_id.clone();
        let manager_for_screen = manager.clone();
        let desktop_id_for_screen = desktop_id.clone();
        let shutdown_for_screen = shutdown.clone();
        let desktop_id_for_log = desktop_id.clone();
        let bridge_handles_for_reader = bridge_handles.clone();

        // Channel for reader → response forwarder (http_resp, ws_data,
        // ws_close frames).
        let (res_tx, mut res_rx) =
            tokio::sync::mpsc::unbounded_channel::<tokio_tungstenite::tungstenite::Message>();

        // Shared map: stream_id → sender for local WS bridge tasks.
        let streams: StreamMap = Arc::new(Mutex::new(HashMap::new()));

        // Reader task — parses framed JSON messages from the relay.
        //
        //   "input"     → write keystrokes to the active PTY          (existing)
        //   "http_req"  → make a local HTTP request, send back resp
        //   "ws_open"   → open local WS connection, bridge frames
        //   "ws_data"   → forward data to the local WS for stream_id
        //   "ws_close"  → close the local WS for stream_id
        let reader_res_tx = res_tx.clone();
        let reader_streams = streams.clone();
        let reader = tokio::spawn(async move {
            use std::io::Write;
            while let Some(msg) = ws_rx.next().await {
                match msg {
                    Ok(m) => {
                        let payload = match &m {
                            tokio_tungstenite::tungstenite::Message::Text(text) => {
                                Some(text.as_str())
                            }
                            tokio_tungstenite::tungstenite::Message::Binary(bytes) => {
                                std::str::from_utf8(bytes).ok()
                            }
                            _ => None,
                        };
                        if let Some(raw) = payload {
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) {
                                match val.get("type").and_then(|v| v.as_str()) {
                                    Some("input") => {
                                        if let Some(data) = val.get("data").and_then(|v| v.as_str())
                                        {
                                            let pane_id: Option<String> = val
                                                .get("pane_id")
                                                .and_then(|v| v.as_str())
                                                .map(ToString::to_string)
                                                .or_else(|| {
                                                    manager_for_reader
                                                        .active_pane_snapshot()
                                                        .ok()
                                                        .map(|snap| snap.pane_id)
                                                });
                                            if let Some(pane_id) = pane_id {
                                                if let Some(entry) =
                                                    manager_for_reader.sessions.get(&pane_id)
                                                {
                                                    if let Ok(mut w) = entry.writer.lock() {
                                                        let _ = w.write_all(data.as_bytes());
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Some("http_req") => {
                                        let tx = reader_res_tx.clone();
                                        let req_val = val.clone();
                                        tokio::spawn(async move {
                                            http_req_proxy(req_val, tx, local_port).await;
                                        });
                                    }
                                    Some("ws_open") => {
                                        let sid = val
                                            .get("stream_id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        let path = val
                                            .get("path")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("/ws")
                                            .to_string();
                                        let tx = reader_res_tx.clone();
                                        let strs = reader_streams.clone();
                                        let bridge_handle = tokio::spawn(async move {
                                            ws_bridge_proxy(sid, path, tx, strs, local_port).await;
                                        });
                                        bridge_handles_for_reader
                                            .lock()
                                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                                            .push(bridge_handle);
                                    }
                                    Some("ws_data") => {
                                        let sid = val
                                            .get("stream_id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        let data_b64 =
                                            val.get("data").and_then(|v| v.as_str()).unwrap_or("");
                                        let binary = val
                                            .get("binary")
                                            .and_then(serde_json::Value::as_bool)
                                            .unwrap_or(false);
                                        let bytes = BASE64.decode(data_b64).unwrap_or_default();
                                        let map = reader_streams
                                            .lock()
                                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                                        if let Some(tx) = map.get(sid) {
                                            let msg = if binary {
                                                tokio_tungstenite::tungstenite::Message::Binary(
                                                    bytes,
                                                )
                                            } else {
                                                tokio_tungstenite::tungstenite::Message::Text(
                                                    String::from_utf8_lossy(&bytes).to_string(),
                                                )
                                            };
                                            let _ = tx.send(msg);
                                        }
                                    }
                                    Some("ws_close") => {
                                        let sid = val
                                            .get("stream_id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        let mut map = reader_streams
                                            .lock()
                                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                                        if let Some(tx) = map.remove(&sid) {
                                            let _ = tx.send(
                                                tokio_tungstenite::tungstenite::Message::Close(
                                                    None,
                                                ),
                                            );
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        if matches!(m, tokio_tungstenite::tungstenite::Message::Close(_)) {
                            break;
                        }
                        let _ = &desktop_id_for_reader;
                    }
                    Err(_) => break,
                }
            }
            info!(desktop_id = %desktop_id_for_log, "relay reader ended");
        });

        // Screen-ticker task + response forwarder.
        //
        // Every 200 ms a screen snapshot is sent.  Also reads from
        // `res_rx` and forwards the frames through `ws_tx` — this lets
        // http_req / ws_open handlers send their responses without
        // needing direct access to `ws_tx`.
        let screen = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(200));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if shutdown_for_screen.load(atomic::Ordering::Relaxed) {
                            break;
                        }
                        let payload = snapshot_active_pane(&manager_for_screen);
                        if let Some(payload) = payload {
                            let frame = serde_json::to_string(&payload).unwrap_or_default();
                            if ws_tx
                                .send(
                                    tokio_tungstenite::tungstenite::Message::Text(frame),
                                )
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                    Some(msg) = res_rx.recv() => {
                        if ws_tx.send(msg).await.is_err() {
                            break;
                        }
                    }
                }
            }
            info!(desktop_id = %desktop_id_for_screen, "relay screen ticker ended");
        });

        // Pin in-place so we can still access the JoinHandles after select.
        tokio::pin!(reader);
        tokio::pin!(screen);

        // Wait for either task to finish. Reader finishing = relay closed;
        // screen finishing = something internal failed. Either way, we
        // reconnect after aborting the surviving task.
        tokio::select! {
            _ = reader.as_mut() => {
                info!("relay reader ended; reconnecting");
            }
            _ = screen.as_mut() => {
                warn!("relay screen ticker ended; reconnecting");
            }
        }

        // Abort whichever task didn't finish (abort on a completed handle
        // is a no-op). This prevents orphaned tasks from accumulating
        // across reconnect iterations.
        reader.abort();
        screen.abort();

        info!("relay connection lost; will retry");
        sleep_or_shutdown(&shutdown, backoff).await;
    }

    info!("outbound relay client exiting");
}

// ── Framed-message helpers ──────────────────────────────────────────

/// Handle an `http_req` frame: make a local HTTP request and send back
/// `http_resp` through the response channel.
async fn http_req_proxy(
    val: serde_json::Value,
    res_tx: tokio::sync::mpsc::UnboundedSender<tokio_tungstenite::tungstenite::Message>,
    local_port: u16,
) {
    let req_id = val.get("req_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let method = val.get("method").and_then(|v| v.as_str()).unwrap_or("GET");
    let path = val.get("path").and_then(|v| v.as_str()).unwrap_or("/");
    let body_b64 = val.get("body").and_then(|v| v.as_str()).unwrap_or("");
    let body_bytes = BASE64.decode(body_b64).unwrap_or_default();

    let url = format!("http://127.0.0.1:{local_port}{path}");

    let client = reqwest::Client::new();
    let mut req_builder = match method {
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        "PATCH" => client.patch(&url),
        "HEAD" => client.head(&url),
        _ => client.get(&url),
    };

    // Forward headers.
    if let Some(headers_val) = val.get("headers").and_then(|v| v.as_object()) {
        for (k, v) in headers_val {
            if let Some(vs) = v.as_str() {
                if let (Ok(hk), Ok(hv)) = (
                    reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                    reqwest::header::HeaderValue::from_str(vs),
                ) {
                    req_builder = req_builder.header(hk, hv);
                }
            }
        }
    }

    // Body for write-methods.
    if !body_bytes.is_empty() && method != "GET" && method != "HEAD" {
        req_builder = req_builder.body(body_bytes);
    }

    match req_builder.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let resp_headers = resp.headers().clone();
            let resp_body = resp.bytes().await.unwrap_or_default();

            let mut headers_json = serde_json::Map::new();
            for (name, value) in resp_headers.iter() {
                if let Ok(v) = value.to_str() {
                    headers_json.insert(
                        name.as_str().to_string(),
                        serde_json::Value::String(v.to_string()),
                    );
                }
            }

            let frame = serde_json::json!({
                "type": "http_resp",
                "req_id": req_id,
                "status": status,
                "headers": headers_json,
                "body": BASE64.encode(resp_body),
            });
            let json = serde_json::to_string(&frame).unwrap_or_default();
            let _ = res_tx.send(tokio_tungstenite::tungstenite::Message::Text(json));
        }
        Err(e) => {
            let err_frame = serde_json::json!({
                "type": "http_resp",
                "req_id": req_id,
                "status": 502,
                "headers": {},
                "body": BASE64.encode(format!("proxy error: {e}")),
            });
            let json = serde_json::to_string(&err_frame).unwrap_or_default();
            let _ = res_tx.send(tokio_tungstenite::tungstenite::Message::Text(json));
        }
    }
}

/// Handle a `ws_open` frame: connect to the local WS endpoint at
/// `ws://127.0.0.1:{local_port}{path}` and bridge frames bidirectionally
/// through the framed tunnel.
async fn ws_bridge_proxy(
    stream_id: String,
    path: String,
    res_tx: tokio::sync::mpsc::UnboundedSender<tokio_tungstenite::tungstenite::Message>,
    streams: StreamMap,
    local_port: u16,
) {
    use futures_util::{SinkExt, StreamExt};

    let url = format!("ws://127.0.0.1:{}{}", local_port, path);

    let (local_ws, _) = match tokio_tungstenite::connect_async(&url).await {
        Ok(ws) => ws,
        Err(e) => {
            tracing::warn!(%stream_id, %url, error = %e, "ws_open: local connect failed");
            return;
        }
    };

    let (mut local_tx, mut local_rx) = local_ws.split();

    // Channel for receiving data from outbound WS → local WS.
    let (stream_tx, mut stream_rx) =
        tokio::sync::mpsc::unbounded_channel::<tokio_tungstenite::tungstenite::Message>();

    // Register sender so the reader can forward ws_data / ws_close.
    streams
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(stream_id.clone(), stream_tx);

    // Task 1: local WS → outbound WS (via res_tx as ws_data / ws_close).
    let sid = stream_id.clone();
    let res_tx_clone = res_tx.clone();
    let to_outbound = tokio::spawn(async move {
        while let Some(item) = local_rx.next().await {
            let Ok(msg) = item else { break };
            let frame = match &msg {
                tokio_tungstenite::tungstenite::Message::Text(t) => serde_json::json!({
                    "type": "ws_data",
                    "stream_id": &sid,
                    "data": BASE64.encode(t.as_bytes()),
                    "binary": false,
                }),
                tokio_tungstenite::tungstenite::Message::Binary(b) => serde_json::json!({
                    "type": "ws_data",
                    "stream_id": &sid,
                    "data": BASE64.encode(b),
                    "binary": true,
                }),
                tokio_tungstenite::tungstenite::Message::Close(_) => serde_json::json!({
                    "type": "ws_close",
                    "stream_id": &sid,
                }),
                _ => continue,
            };
            let json = serde_json::to_string(&frame).unwrap_or_default();
            if res_tx_clone.send(tokio_tungstenite::tungstenite::Message::Text(json)).is_err() {
                break;
            }
            if matches!(msg, tokio_tungstenite::tungstenite::Message::Close(_)) {
                break;
            }
        }
        // On unexpected disconnect, send ws_close.
        let close_frame = serde_json::json!({
            "type": "ws_close",
            "stream_id": &sid,
        });
        if let Ok(json) = serde_json::to_string(&close_frame) {
            let _ = res_tx_clone.send(tokio_tungstenite::tungstenite::Message::Text(json));
        }
    });

    // Task 2: outbound WS → local WS (via stream_rx).
    let to_local = tokio::spawn(async move {
        while let Some(msg) = stream_rx.recv().await {
            if local_tx.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Wait for either direction to close.
    tokio::select! {
        _ = to_outbound => {},
        _ = to_local => {},
    }

    // Cleanup: unregister the stream.
    streams.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&stream_id);
}

fn extract_host(url: &str) -> String {
    // Strip scheme:// and trailing path. Just the host:port for the Host
    // header.
    let without_scheme =
        url.strip_prefix("ws://").or_else(|| url.strip_prefix("wss://")).unwrap_or(url);
    without_scheme.split('/').next().unwrap_or(without_scheme).to_string()
}

/// Build the underlying TCP / TLS stream the websocket rides on. For ws://
/// we use plain TCP; for wss:// we use tokio_rustls. Hand-rolled instead
/// of `tungstenite::connect` because the latter is blocking (it's a
/// pure-rust sync wrapper around `std::net::TcpStream`).
async fn ws_connect_stream(ws_url: String) -> Result<WsStream, WsErr> {
    use std::io;
    use tokio::net::TcpStream;

    let (scheme, rest) = if let Some(s) = ws_url.strip_prefix("wss://") {
        ("wss", s.to_string())
    } else if let Some(s) = ws_url.strip_prefix("ws://") {
        ("ws", s.to_string())
    } else {
        return Err(WsErr::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "url must be ws:// or wss://",
        )));
    };
    let host_port = rest.split('/').next().unwrap_or(&rest).to_string();
    let (host, port) = host_port
        .rsplit_once(':')
        .ok_or_else(|| WsErr::Io(io::Error::new(io::ErrorKind::InvalidInput, "missing port")))?;
    let port: u16 =
        port.parse().map_err(|e| WsErr::Io(io::Error::new(io::ErrorKind::InvalidInput, e)))?;
    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .map_err(|e| WsErr::Io(io::Error::new(io::ErrorKind::InvalidInput, e)))?;
    let host = host.to_string();
    let tcp = TcpStream::connect(addr).await?;
    if scheme == "wss" {
        use rustls::{ClientConfig, RootCertStore};
        use tokio_rustls::TlsConnector;

        // Lazy: import only when needed so the dev-binary build without
        // rustls still works.
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let cfg = ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(cfg));
        let server_name = rustls::pki_types::ServerName::try_from(host.clone())
            .map_err(|e| WsErr::Io(io::Error::new(io::ErrorKind::InvalidInput, e)))?;
        let tls = connector.connect(server_name, tcp).await?;
        Ok(WsStream::Tls(Box::new(tls)))
    } else {
        Ok(WsStream::Plain(tcp))
    }
}

enum WsStream {
    Plain(tokio::net::TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>),
}

impl tokio::io::AsyncRead for WsStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            WsStream::Plain(t) => std::pin::Pin::new(t).poll_read(cx, buf),
            WsStream::Tls(t) => std::pin::Pin::new(t).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for WsStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            WsStream::Plain(t) => std::pin::Pin::new(t).poll_write(cx, buf),
            WsStream::Tls(t) => std::pin::Pin::new(t).poll_write(cx, buf),
        }
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            WsStream::Plain(t) => std::pin::Pin::new(t).poll_flush(cx),
            WsStream::Tls(t) => std::pin::Pin::new(t).poll_flush(cx),
        }
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            WsStream::Plain(t) => std::pin::Pin::new(t).poll_shutdown(cx),
            WsStream::Tls(t) => std::pin::Pin::new(t).poll_shutdown(cx),
        }
    }
}

type WsErr = tokio_tungstenite::tungstenite::Error;

async fn sleep_or_shutdown(shutdown: &Arc<atomic::AtomicBool>, dur: Duration) {
    let start = std::time::Instant::now();
    while start.elapsed() < dur {
        if shutdown.load(atomic::Ordering::Relaxed) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Snapshot the currently-active pane's screen for relay forwarding.
/// Returns a JSON object `{ type, pane_id, screen, cols, rows }` or
/// `None` if no active pane / session exists.
fn snapshot_active_pane(manager: &SessionManager) -> Option<serde_json::Value> {
    let snap = manager.active_pane_snapshot().ok()?;
    let session = manager.sessions.get(&snap.pane_id)?;
    let (cols, rows) = *session.size.lock().ok()?;
    let screen = session.screen.lock().ok()?;
    let text = screen.snapshot();
    Some(serde_json::json!({
        "type": "screen",
        "pane_id": snap.pane_id,
        "cols": cols,
        "rows": rows,
        "screen": text,
    }))
}

async fn server_info(State(state): State<AppState>) -> Json<serde_json::Value> {
    let lan_ip =
        local_ip_address::local_ip().map_or_else(|_| "127.0.0.1".to_string(), |ip| ip.to_string());
    Json(serde_json::json!({
        "lan_ip": lan_ip,
        "port": state.port,
        "version": state.git_info.version,
        "repo_url": state.git_info.repo_url,
    }))
}

#[derive(serde::Deserialize)]
struct UpdateTokenRequest {
    token: String,
}

async fn check_auth(State(state): State<AppState>) -> impl IntoResponse {
    let _ = state;
    StatusCode::OK
}

async fn get_token(State(state): State<AppState>) -> impl IntoResponse {
    let token = state.auth_token.read().await;
    Json(serde_json::json!({ "token": *token }))
}

async fn token_configured(State(state): State<AppState>) -> impl IntoResponse {
    let token = state.auth_token.read().await;
    Json(serde_json::json!({ "configured": !token.is_empty() }))
}

async fn update_token(
    State(state): State<AppState>,
    Json(body): Json<UpdateTokenRequest>,
) -> impl IntoResponse {
    let new_token = body.token.trim().to_string();
    if new_token.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "token cannot be empty"})),
        )
            .into_response();
    }
    // Update in-memory token
    *state.auth_token.write().await = new_token.clone();
    // Persist to dedicated token file
    if let Err(e) = settings::save_token(&new_token) {
        tracing::error!("Failed to persist token: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to save"})),
        )
            .into_response();
    }
    StatusCode::OK.into_response()
}

async fn generate_qr_code(State(state): State<AppState>) -> impl IntoResponse {
    let token = state.auth_token.read().await;
    if token.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "no token configured"})),
        )
            .into_response();
    }
    let code = state.qr_codes.generate(&token);
    Json(serde_json::json!({ "code": code })).into_response()
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = parse_args();

    // Outbound relay mode: connects to a cloud relay (run by
    // server-install.sh) and forwards incoming WS/HTTP requests to
    // another local dinotty-server instance. The outbound mode does
    // NOT start its own HTTP server — it assumes a normal dinotty-server
    // is already running on the same machine (same --port).
    //
    // Usage:
    //   Terminal 1: dinotty-server --port 8999
    //   Terminal 2: dinotty-server --port 8999 --relay-outbound <relay-url> <password>
    if let (Some(relay_url), Some(relay_password)) = (args.relay_url, args.relay_password) {
        let desktop_id = args.relay_desktop_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let shutdown = Arc::new(atomic::AtomicBool::new(false));

        // Restore any prior tabs from disk so the screen snapshot has
        // something to read on a freshly-restarted desktop. The
        // SessionManager is otherwise unused in this mode — no axum
        // server runs, no PTY spawns.
        let manager = Arc::new(SessionManager::new());
        restore_state::restore(&manager);

        // Persist the desktop_id so subsequent runs reuse it.
        if let Some(home) = dirs::home_dir() {
            let path = home.join(".dinotty").join("relay-desktop-id");
            if let Ok(()) = std::fs::create_dir_all(path.parent().unwrap()) {
                let _ = std::fs::write(&path, &desktop_id);
            }
        }
        run_relay_outbound(relay_url, relay_password, desktop_id, manager, shutdown, args.port)
            .await;
        return;
    }

    let port = args.port;
    let manager = Arc::new(SessionManager::new());
    restore_state::restore(&manager);
    manager.start_cleanup_task();

    let monitor_state = MonitorState::new();
    monitor_state.clone().start_collector();

    let notifier = Arc::new(NotificationBroadcast::new());
    let settings_state = settings::create_settings_state();
    notifier.set_settings(settings_state.clone());
    let history_state = HistoryState::new();

    // Load token from dedicated file or env var; empty means first-time setup
    let initial_token =
        settings::load_token().or_else(|| std::env::var("DINOTTY_TOKEN").ok()).unwrap_or_default();
    if initial_token.is_empty() {
        tracing::info!("No auth token configured — first-time setup required");
    } else {
        tracing::info!("Auth token loaded (length={})", initial_token.len());
    }
    let auth_token = Arc::new(tokio::sync::RwLock::new(initial_token));

    let plugins = Arc::new(plugin::PluginManager::new());
    plugins.scan();
    tracing::info!("Loaded {} plugins", plugins.list().len());

    let git_info = read_git_info();
    tracing::info!("Git info: {}", git_info.version);

    // Initialize new modules
    let tokens = Arc::new(token::TokenManager::new(auth_token.clone()));
    tokens.start_cleanup_task();

    let audit_logger = Arc::new(audit::AuditLogger::new());

    let agent_state = agent::AgentState {
        manager: manager.clone(),
        settings: settings_state.clone(),
        tokens: tokens.clone(),
        audit: audit_logger.clone(),
        run_limiter: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
    };

    // Load webhook configs from settings
    let webhook_configs = {
        // Webhook configs will be added to settings later; for now empty
        Vec::<webhook::WebhookConfig>::new()
    };
    let webhooks = Arc::new(webhook::WebhookDispatcher::new(webhook_configs));
    webhooks.start(&manager.event_bus);

    let mcp_server = Arc::new(mcp::server::McpServer::new(manager.clone(), settings_state.clone()));
    let mcp_sse = Arc::new(mcp::transport::SseState::new());
    let qr_codes = Arc::new(qr_code::QrCodeState::new());
    qr_codes.clone().start_cleanup_task();

    let state = AppState {
        manager,
        settings: settings_state,
        file_watcher: Arc::new(FileWatcherState::new()),
        monitor: monitor_state,
        notifier,
        history: history_state,
        auth_token: auth_token.clone(),
        port,
        plugins,
        git_info,
        tokens,
        audit: audit_logger,
        agent: agent_state,
        webhooks,
        mcp: mcp_server,
        mcp_sse,
        qr_codes,
    };

    state.plugins.watch_changes(state.manager.clone());

    let app =
        Router::new()
            .route("/ws", get(ws::ws_handler))
            .route("/ws/sync", get(ws::sync_handler))
            .route("/ws/watch", get(file_watcher::watch_handler))
            .route("/ws/monitor", get(monitor::ws_monitor_handler))
            .route("/ws/notify", get(ws::notification_ws_handler))
            .route("/api/notify", post(notification::post_notify))
            .route("/api/input", post(ws::post_input))
            // Open API
            .route("/api/sessions", get(openapi::list_sessions))
            .route("/api/sessions/:pane_id/screen", get(openapi::get_screen))
            .route("/api/sessions/:pane_id/scrollback", get(openapi::get_scrollback))
            .route("/api/sessions/:pane_id/input", post(openapi::session_input))
            .route("/api/sessions/:pane_id/resize", post(openapi::session_resize))
            .route("/ws/api/sessions/:pane_id/stream", get(openapi::session_stream))
            // Tab/Pane management
            .route("/api/tabs", get(tabs::list_tabs).post(tabs::create_tab))
            .route("/api/tabs/:tab_id", delete(tabs::close_tab))
            .route("/api/tabs/:tab_id/pane", post(tabs::split_pane))
            .route("/api/tabs/:tab_id/pane/:pane_id", delete(tabs::close_pane))
            .route("/api/tabs/:tab_id/pane/:pane_id/activate", put(tabs::activate_pane))
            .route("/api/tabs/:tab_id/layout", put(tabs::update_layout))
            .route("/api/tabs/:tab_id/meta", put(tabs::update_tab_meta))
            .route("/api/shell/profiles", get(shell_profiles::list_profiles))
            .route(
                "/api/restore-state",
                get(restore_state::get_restore_state).delete(restore_state::delete_restore_state),
            )
            .route("/api/auth", post(check_auth))
            .route("/api/token-configured", get(token_configured))
            .route("/api/settings", get(settings::get_settings).put(settings::put_settings))
            .route(
                "/api/settings/background",
                post(settings::upload_background).get(settings::get_background),
            )
            .route("/api/workspace/resolve", get(workspace::workspace_resolve))
            .route("/api/workspace/list", get(workspace::workspace_list))
            .route("/api/workspace/meta", get(workspace::workspace_meta))
            .route("/api/workspace/raw", get(workspace::workspace_raw))
            .merge(
                Router::new()
                    .route("/api/workspace/upload", post(workspace::workspace_upload))
                    .layer(axum::extract::DefaultBodyLimit::max(512 * 1024 * 1024)),
            )
            .route("/api/workspace/create", post(workspace::workspace_create_entry))
            .route("/api/workspace/file", put(workspace::workspace_put_file))
            .route("/api/workspace/delete", delete(workspace::workspace_delete))
            .route("/api/workspace/rename", post(workspace::workspace_rename))
            .route("/api/workspace/move", post(workspace::workspace_move))
            .route("/api/workspace/git-status", get(workspace::workspace_git_status))
            .route("/api/workspace/git-diff", get(workspace::workspace_git_diff))
            .route("/api/workspace/git-stage-lines", post(workspace::workspace_git_stage_lines))
            .route("/api/workspace/git-revert-lines", post(workspace::workspace_git_revert_lines))
            .route("/api/workspace/syntax-check", post(workspace::workspace_syntax_check))
            .route("/ws/history", get(history::ws_history_handler))
            .route("/api/history", get(history::get_history).delete(history::delete_history))
            .route("/api/proxy", any(proxy::external_proxy_handler))
            .route("/api/info", get(server_info))
            .route("/api/token", get(get_token).put(update_token))
            .route("/api/qr-code", post(generate_qr_code))
            // Plugin management
            .route("/api/plugins", get(plugin::list_plugins))
            .route("/api/plugins/market", get(plugin::get_market_registry))
            .route("/api/plugins/market/:id/readme", get(plugin::get_market_readme))
            .route("/api/plugins/dev-link", post(plugin::dev_link_plugin))
            .route("/api/plugins/install-dir", post(plugin::install_from_dir))
            .merge(
                Router::new()
                    .route("/api/plugins/install", post(plugin::install_plugin))
                    .route("/api/plugins/install-git", post(plugin::install_from_git))
                    .route("/api/plugins/:id/update", post(plugin::update_plugin))
                    .layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024)),
            )
            .route("/api/plugins/:id", get(plugin::plugin_detail).delete(plugin::delete_plugin))
            .route("/api/plugins/:id/exec", post(plugin::plugin_exec))
            .route("/api/plugins/:id/spawn", get(plugin::plugin_spawn_ws))
            .route("/api/plugins/:id/process/start", post(plugin::plugin_process_start))
            .route(
                "/api/plugins/:id/process",
                get(plugin::plugin_process_list).delete(plugin::plugin_process_stop_all),
            )
            .route("/api/plugins/:id/process/:pid", delete(plugin::plugin_process_stop))
            .route("/api/plugins/:id/storage", get(plugin::plugin_storage_list))
            .route(
                "/api/plugins/:id/storage/:key",
                get(plugin::plugin_storage_get)
                    .put(plugin::plugin_storage_set)
                    .delete(plugin::plugin_storage_delete),
            )
            .route("/api/plugins/:id/*path", get(plugin::plugin_asset))
            // Agent API + Token management + MCP — protected by agent token middleware
            .merge(
                Router::new()
                    .route("/api/agent/run", post(agent::agent_run))
                    .route("/api/agent/send", post(agent::agent_send))
                    .route("/api/agent/read", get(agent::agent_read))
                    .route("/ws/agent", get(agent::agent_ws_handler))
                    .route("/api/tokens", post(token::create_token).get(token::list_tokens))
                    .route(
                        "/api/tokens/:id",
                        get(token::get_token_detail)
                            .put(token::update_token)
                            .delete(token::revoke_token),
                    )
                    .route("/mcp/sse", get(mcp::transport::mcp_sse_handler))
                    .route("/mcp/message", post(mcp::transport::mcp_message_handler))
                    .layer(middleware::from_fn_with_state(
                        token::AgentAuthState {
                            global_token: auth_token.clone(),
                            tokens: state.tokens.clone(),
                        },
                        token::agent_token_middleware,
                    )),
            )
            .route("/preview/:port", any(proxy::proxy_handler_root))
            .route("/preview/:port/", any(proxy::proxy_handler_root))
            .route("/preview/:port/*path", any(proxy::proxy_handler_wildcard))
            .route("/assets/*path", get(static_handler))
            .route("/icons/*path", get(icon_handler))
            .route("/manifest.json", get(manifest_handler))
            .route(
                "/logo.png",
                get(|| async {
                    match StaticFiles::get("logo.png") {
                        Some(content) => Response::builder()
                            .header(header::CONTENT_TYPE, "image/png")
                            .header(header::CACHE_CONTROL, "public, max-age=86400")
                            .body(Body::from(content.data.into_owned()))
                            .unwrap(),
                        None => Response::builder()
                            .status(StatusCode::NOT_FOUND)
                            .body(Body::from("not found"))
                            .unwrap(),
                    }
                }),
            )
            .route("/", get(index))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                |State(s): State<AppState>,
                 ConnectInfo(addr): ConnectInfo<SocketAddr>,
                 req,
                 next| async move {
                    let token = s.auth_token.read().await.clone();
                    auth::auth_middleware(req, next, &token, &s.settings, addr.ip()).await
                },
            ))
            .layer(CorsLayer::permissive())
            .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!(
        port,
        config_dir = %dirs::config_dir().unwrap_or_default().to_string_lossy(),
        version = env!("CARGO_PKG_VERSION"),
        "Starting dinotty-server",
    );

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            tracing::error!(
                port,
                "Port {} is already in use. Check if another dinotty-server instance is running, \
                 or specify --port to use a different port.",
                port,
            );
            std::process::exit(1);
        }
        Err(e) => {
            tracing::error!("Failed to bind to {}: {}", addr, e);
            std::process::exit(1);
        }
    };
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
}
