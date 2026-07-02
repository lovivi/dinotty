//! Relay catch-all proxy: forwards HTTP requests and WebSocket upgrades
//! through the desktop's outbound WS tunnel using a framed JSON protocol.
//!
//! Wire format (relay ↔ desktop outbound WS):
//!
//!   {type:"http_req",  req_id, method, path, headers, body}    → desktop
//!   {type:"http_resp", req_id, status, headers, body}          ← desktop
//!   {type:"ws_open",   stream_id, path}                        → desktop
//!   {type:"ws_data",   stream_id, data (base64), binary}       ↔ desktop
//!   {type:"ws_close",  stream_id}                              ↔ desktop

#![allow(clippy::module_name_repetitions)]

use rust_embed::Embed;
use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::{header, HeaderMap, HeaderValue, Request, Response, StatusCode, Uri},
    response::IntoResponse,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::time::Duration;
use tracing::{info, warn};
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────

/// Resolve the target desktop_id from query string or single-desktop
/// fallback.
fn resolve_desktop_id(
    desktops: &DashMap<String, super::DesktopSlot>,
    query: &str,
) -> Option<String> {
    // 1. Try `?desktop_id=` from query
    for pair in query.split('&') {
        if let Some(val) = pair.strip_prefix("desktop_id=") {
            return Some(val.to_string());
        }
    }
    // 2. If exactly one desktop is registered, use it
    let mut ids: Vec<String> = desktops.iter().map(|e| e.key().clone()).collect();
    if ids.len() == 1 {
        return Some(ids.remove(0));
    }
    None
}

/// Check whether the request contains a WebSocket upgrade header.
#[allow(dead_code)]
fn is_ws_upgrade(headers: &HeaderMap) -> bool {
    headers
        .get(header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
}

/// Try to serve a file from the embedded frontend dist. Returns `None`
/// when no static asset matches — the caller should then proxy the request.
fn try_serve_static(path: &str) -> Option<axum::response::Response<Body>> {
    let clean = path.trim_start_matches('/');

    // Root → index.html
    if clean.is_empty() {
        return Some(super::serve_index());
    }

    // Serve exact file matches only (no SPA fallback for unknown paths —
    // those go through the proxy).
    match super::Frontend::get(clean) {
        Some(file) => {
            let mime = mime_guess::from_path(clean).first_or_octet_stream();
            Some(
                Response::builder()
                    .header(header::CONTENT_TYPE, mime.as_ref())
                    .body(Body::from(file.data.into_owned()))
                    .unwrap()
                    .into_response(),
            )
        }
        None => None,
    }
}

// ── Main catch-all fallback (HTTP + static) ─────────────────────────

/// Fallback handler: serves embedded static assets for recognised file
/// paths, and proxies everything else through the desktop's outbound
/// tunnel.
pub async fn proxy_fallback(
    State(state): State<super::AppState>,
    req: Request<Body>,
) -> axum::response::Response<Body> {
    let path = req.uri().path();

    // 1. Serve static files directly
    if let Some(resp) = try_serve_static(path) {
        return resp;
    }

    // 2. Resolve desktop
    let query = req.uri().query().unwrap_or("");
    let desktop_id = match resolve_desktop_id(&state.desktops, query) {
        Some(id) => id,
        None => {
            return (StatusCode::BAD_GATEWAY, "no desktop registered").into_response();
        }
    };

    // 3. Forward as HTTP proxy
    http_proxy_handler(state, desktop_id, req).await
}

// ── WebSocket proxy handler ─────────────────────────────────────────

/// Handler for `GET /ws` and `GET /ws/*` — upgrades the connection and
/// bridges frames through the outbound tunnel.
pub async fn ws_proxy_handler(
    ws: WebSocketUpgrade,
    State(state): State<super::AppState>,
    uri: Uri,
) -> axum::response::Response<Body> {
    let query = uri.query().unwrap_or("");
    let path = uri.path().to_string();

    let desktop_id = match resolve_desktop_id(&state.desktops, query) {
        Some(id) => id,
        None => {
            return (StatusCode::BAD_REQUEST, "desktop_id required or exactly one desktop must be registered").into_response();
        }
    };

    let (dtm_rx, mtd_tx) = {
        let entry = state.desktops.get(&desktop_id);
        match entry.as_ref() {
            Some(slot) => (
                slot.desktop_to_mobile.subscribe(),
                slot.mobile_to_desktop.clone(),
            ),
            None => {
                return (StatusCode::BAD_GATEWAY, "desktop not registered").into_response();
            }
        }
    };

    let did = desktop_id.clone();

    ws.on_upgrade(move |socket| async move {
        info!(desktop_id = %did, path = %path, "ws proxy upgraded");
        ws_proxy_task(socket, dtm_rx, mtd_tx, did, path).await;
    })
    .into_response()
}

/// Bridge a single WebSocket session through the framed tunnel.
async fn ws_proxy_task(
    socket: WebSocket,
    mut dtm_rx: tokio::sync::broadcast::Receiver<Vec<u8>>,
    mtd_tx: tokio::sync::broadcast::Sender<Vec<u8>>,
    _desktop_id: String,
    path: String,
) {
    let stream_id = format!("ws-{}", Uuid::new_v4());
    let (mut ws_tx, mut ws_rx) = socket.split();

    // --- Send `ws_open` to the desktop ---
    let open = serde_json::json!({
        "type": "ws_open",
        "stream_id": &stream_id,
        "path": &path,
    });
    if mtd_tx
        .send(serde_json::to_string(&open).unwrap_or_default().into_bytes())
        .is_err()
    {
        warn!("ws proxy: desktop disconnected before ws_open");
        return;
    }

    // --- dtm_rx → ws_tx (desktop → mobile, filtered by stream_id) ---
    let dtm_task = {
        let sid = stream_id.clone();
        tokio::spawn(async move {
            loop {
                match dtm_rx.recv().await {
                    Ok(frame) => {
                        let Ok(val) = serde_json::from_slice::<Value>(&frame) else {
                            continue;
                        };
                        if val.get("stream_id").and_then(|v| v.as_str()) != Some(&sid) {
                            continue;
                        }
                        match val.get("type").and_then(|v| v.as_str()) {
                            Some("ws_data") => {
                                let raw = BASE64
                                    .decode(val["data"].as_str().unwrap_or(""))
                                    .unwrap_or_default();
                                let bin = val["binary"].as_bool().unwrap_or(false);
                                let msg = if bin {
                                    Message::Binary(raw)
                                } else {
                                    Message::Text(String::from_utf8_lossy(&raw).to_string())
                                };
                                if ws_tx.send(msg).await.is_err() {
                                    break;
                                }
                            }
                            Some("ws_close") => {
                                let _ = ws_tx.send(Message::Close(None)).await;
                                break;
                            }
                            _ => {}
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(lagged = n, %sid, "ws proxy dtm_rx lagged");
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    };

    // --- ws_rx → mtd_tx (mobile → desktop, as ws_data / ws_close) ---
    while let Some(msg) = ws_rx.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(_) => break,
        };
        let (frame, is_close) = match msg {
            Message::Text(t) => (
                serde_json::json!({
                    "type": "ws_data",
                    "stream_id": &stream_id,
                    "data": BASE64.encode(t.as_bytes()),
                    "binary": false,
                }),
                false,
            ),
            Message::Binary(b) => (
                serde_json::json!({
                    "type": "ws_data",
                    "stream_id": &stream_id,
                    "data": BASE64.encode(&b),
                    "binary": true,
                }),
                false,
            ),
            Message::Close(_) => {
                let close = serde_json::json!({
                    "type": "ws_close",
                    "stream_id": &stream_id,
                });
                (close, true)
            }
            _ => continue,
        };
        let json = serde_json::to_string(&frame).unwrap_or_default();
        if mtd_tx.send(json.into_bytes()).is_err() {
            break;
        }
        if is_close {
            break;
        }
    }

    dtm_task.abort();
    info!(%stream_id, "ws proxy finished");
}

// ── HTTP proxy handler ──────────────────────────────────────────────

/// Forward a single HTTP request through the framed tunnel and wait for
/// the response.
async fn http_proxy_handler(
    state: super::AppState,
    desktop_id: String,
    req: Request<Body>,
) -> axum::response::Response<Body> {
    let (mut dtm_rx, mtd_tx) = {
        let entry = state.desktops.get(&desktop_id);
        match entry.as_ref() {
            Some(slot) => (
                slot.desktop_to_mobile.subscribe(),
                slot.mobile_to_desktop.clone(),
            ),
            None => {
                return (StatusCode::BAD_GATEWAY, "desktop not available").into_response();
            }
        }
    };

    // Read request body (max 10 MB).
    let (parts, body) = req.into_parts();
    let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "request body too large").into_response();
        }
    };

    let req_id = Uuid::new_v4().to_string();
    let uri_str = parts.uri.to_string();

    // Serialise headers (skip hop-by-hop headers).
    let mut headers_json = serde_json::Map::new();
    for (name, value) in &parts.headers {
        let n = name.as_str().to_lowercase();
        let skip = matches!(
            n.as_str(),
            "host"
                | "connection"
                | "upgrade"
                | "sec-websocket-key"
                | "sec-websocket-version"
                | "sec-websocket-extensions"
                | "transfer-encoding"
        );
        if skip {
            continue;
        }
        if let Ok(v) = value.to_str() {
            headers_json.insert(n, Value::String(v.to_string()));
        }
    }

    // Send `http_req`.
    let frame = serde_json::json!({
        "type": "http_req",
        "req_id": &req_id,
        "method": parts.method.as_str(),
        "path": uri_str,
        "headers": headers_json,
        "body": BASE64.encode(&body_bytes),
    });
    let raw = serde_json::to_string(&frame).unwrap_or_default();
    if mtd_tx.send(raw.into_bytes()).is_err() {
        return (StatusCode::BAD_GATEWAY, "desktop disconnected").into_response();
    }

    // Wait for `http_resp` (30 s timeout).
    let deadline = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            _ = &mut deadline => {
                return (StatusCode::GATEWAY_TIMEOUT, "desktop not responding").into_response();
            }
            result = dtm_rx.recv() => {
                match result {
                    Ok(frame) => {
                        let Ok(val) = serde_json::from_slice::<Value>(&frame) else {
                            continue;
                        };
                        if val.get("type").and_then(|v| v.as_str()) != Some("http_resp") {
                            continue;
                        }
                        if val.get("req_id").and_then(|v| v.as_str()) != Some(&req_id) {
                            continue;
                        }

                        let status = val["status"].as_u64().unwrap_or(500) as u16;
                        let body_raw = BASE64
                            .decode(val["body"].as_str().unwrap_or(""))
                            .unwrap_or_default();

                        let mut resp_headers = HeaderMap::new();
                        if let Some(obj) = val.get("headers").and_then(|v| v.as_object()) {
                            for (k, v) in obj {
                                if let Some(vs) = v.as_str() {
                                    if let (Ok(hk), Ok(hv)) = (
                                        header::HeaderName::from_bytes(k.as_bytes()),
                                        HeaderValue::from_str(vs),
                                    ) {
                                        resp_headers.insert(hk, hv);
                                    }
                                }
                            }
                        }

                        return (StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), resp_headers, Body::from(body_raw)).into_response();
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(lagged = n, req_id = %req_id, "http proxy lagged");
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return (StatusCode::BAD_GATEWAY, "desktop disconnected").into_response();
                    }
                }
            }
        }
    }
}
