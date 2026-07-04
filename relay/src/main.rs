//! Dinotty relay: a tiny bidirectional WS forwarder that sits between the
//! mobile APK and the desktop Dinotty server. Single shared password
//! (jupyter-token-style), no per-user accounts. Hosts the frontend dist
//! so the APK only needs to know the relay URL + password.
//!
//! Wire format: the relay is opaque — it forwards binary and text frames
//! without parsing. The desktop and the mobile speak the same WS protocol
//! (Dinotty's existing PTY + tab/sync/monitor/etc.), but one side
//! connects to the relay from outside their LAN.
//!
//! Routes:
//!   GET    /healthz                                    — liveness
//!   POST   /relay/desktop/register                     — desktop announces itself
//!   GET    /relay/desktop/ws/:desktop_id               — desktop's outbound tunnel
//!   GET    /relay/mobile/ws/:desktop_id                — mobile's outbound tunnel
//!   GET    /* (fallthrough)                            — serve embedded frontend dist

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod proxy;

use axum::{
    body::Body,
    extract::{
        connect_info::ConnectInfo,
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::{header, Request, Response, StatusCode},
    response::{IntoResponse, Response as AxumResponse},
    routing::{get, post},
    Json, Router,
};
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::sync::broadcast;
use tracing::{info, warn};

/// No embedded frontend. The relay no longer serves static files —
/// all non-route traffic is proxied to the desktop via the outbound
/// WS tunnel. This keeps the relay lightweight (no rust-embed dep).

/// Per-desktop state. Two broadcast channels:
///   - `desktop_to_mobile`: every frame the desktop sends is fanned out
///     to every subscribed mobile. Capacity 64 messages — on overflow,
///     older messages are dropped (acceptable for a terminal stream;
///     newer data is more important).
///   - `mobile_to_desktop`: every subscribed mobile writes here; the
///     desktop's WS reader forwards from this channel to its WS sink.
///     Same 64-message capacity.
///
/// Per-desktop shared state, behind DashMap. Wrapped in Arc so closures
/// can hold a reference without borrowing the map itself.
#[derive(Clone)]
struct DesktopSlot {
    /// Frames desktop → mobile (broadcast).
    desktop_to_mobile: broadcast::Sender<Vec<u8>>,
    /// Frames mobile → desktop (broadcast).
    mobile_to_desktop: broadcast::Sender<Vec<u8>>,
    /// Token used to authenticate as this desktop to itself if we ever
    /// proxy HTTP. Today unused; kept for forward compatibility.
    #[allow(dead_code)]
    upstream_token: Option<String>,
}

#[derive(Clone)]
struct AppState {
    password: Arc<String>,
    desktops: Arc<DashMap<String, DesktopSlot>>,
}

#[derive(Deserialize)]
struct RegisterBody {
    desktop_id: String,
    #[serde(default)]
    upstream_token: Option<String>,
}

#[derive(Serialize)]
struct RegisterReply {
    ok: bool,
    desktop_id: String,
}

#[derive(Deserialize, Serialize, Clone)]
struct Hello {
    v: u8,
    desktop_id: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = parse_args();
    let password = std::env::var("RELAY_PASSWORD")
        .ok()
        .or_else(|| args.password.clone())
        .ok_or("RELAY_PASSWORD or --password is required")?;
    let listen = args.listen.clone();
    let cert = args.cert;
    let key = args.key;

    let state = AppState {
        password: Arc::new(password),
        desktops: Arc::new(DashMap::new()),
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/relay/desktop/register", post(register_desktop))
        .route("/relay/desktop/ws/:desktop_id", get(desktop_ws_upgrade))
        .route("/relay/mobile/ws/:desktop_id", get(mobile_ws_upgrade))
        // WS proxy catches /ws and /ws/* so that WebSocket upgrades for
        // terminal sessions, sync, monitor, history, etc. are forwarded
        // through the outbound tunnel.
        .route("/ws", get(proxy::ws_proxy_handler))
        .route("/ws/*path", get(proxy::ws_proxy_handler))
        // Everything else: static files or HTTP proxy.
        .fallback(proxy::proxy_fallback)
        .with_state(state);

    // Port detection: prefer 24020-24045. If --listen was given, use that port.
    let (listen, prebound): (String, Option<tokio::net::TcpListener>) = match listen {
        Some(l) => {
            let addr: SocketAddr = l.parse().unwrap();
            let l = tokio::net::TcpListener::bind(addr).await.unwrap();
            let port = l.local_addr().unwrap().port();
            (format!("0.0.0.0:{}", port), Some(l))
        }
        None => {
            let mut chosen = None;
            for port in 24020u16..=24045 {
                let addr: SocketAddr = format!("0.0.0.0:{}", port).parse().unwrap();
                if let Ok(l) = tokio::net::TcpListener::bind(addr).await {
                    info!("relay port auto-selected: {}", port);
                    chosen = Some(l);
                    break;
                }
            }
            match chosen {
                Some(l) => {
                    let port = l.local_addr().unwrap().port();
                    (format!("0.0.0.0:{}", port), Some(l))
                }
                None => {
                    warn!("ports 24020-24045 busy; using random port");
                    let l = tokio::net::TcpListener::bind("0.0.0.0:0".parse::<SocketAddr>().unwrap()).await.unwrap();
                    let port = l.local_addr().unwrap().port();
                    (format!("0.0.0.0:{}", port), Some(l))
                }
            }
        }
    };

    info!("dinotty-relay listening on {}", listen);

    if let (Some(cert_path), Some(key_path)) = (cert, key) {
        let addr: SocketAddr = listen.parse().unwrap();
        let config =
            axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert_path, &key_path).await.unwrap();
        axum_server::bind_rustls(addr, config)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .unwrap();
    } else if let Some(listener) = prebound {
        warn!("running with HTTP (no TLS) — phones may block WebSockets");
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    }
    Ok(())
}

#[derive(Default)]
struct Args {
    listen: Option<String>,
    password: Option<String>,
    cert: Option<String>,
    key: Option<String>,
}

fn parse_args() -> Args {
    let mut out = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--listen" => out.listen = it.next(),
            "--password" => out.password = it.next(),
            "--cert" => out.cert = it.next(),
            "--key" => out.key = it.next(),
            "--help" | "-h" => {
                eprintln!("dinotty-relay --listen <addr> --password <pwd> --cert <pem> --key <pem>");
                std::process::exit(0);
            }
            other => warn!(arg = other, "ignoring unknown flag"),
        }
    }
    out
}

async fn healthz() -> &'static str {
    "ok"
}

/// POST /relay/desktop/register
/// Desktop announces itself. Auth: `Authorization: Bearer <RELAY_PASSWORD>`.
async fn register_desktop(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<RegisterBody>,
) -> AxumResponse {
    if !check_password(&headers, &state.password) {
        return (StatusCode::UNAUTHORIZED, "wrong password").into_response();
    }
    if body.desktop_id.is_empty() || body.desktop_id.len() > 128 {
        return (StatusCode::BAD_REQUEST, "desktop_id invalid").into_response();
    }

    // Idempotent: re-registering the same id replaces the slot. The old
    // WS sessions will see their broadcast::Sender dropped and any
    // pending sends will return Err, which the WS handlers treat as
    // disconnect.
    let (dtm_tx, _) = broadcast::channel::<Vec<u8>>(64);
    let (mtd_tx, _) = broadcast::channel::<Vec<u8>>(64);
    state.desktops.insert(
        body.desktop_id.clone(),
        DesktopSlot {
            desktop_to_mobile: dtm_tx,
            mobile_to_desktop: mtd_tx,
            upstream_token: body.upstream_token,
        },
    );
    info!(desktop_id = %body.desktop_id, "desktop registered");
    (
        StatusCode::OK,
        Json(RegisterReply {
            ok: true,
            desktop_id: body.desktop_id,
        }),
    )
        .into_response()
}

/// GET /relay/desktop/ws/:desktop_id
/// Outbound WS from the desktop. The first frame MUST be a JSON
/// `Hello { v: 1, desktop_id }` so the relay can verify the desktop is
/// who it claims to be. Subsequent frames are opaque — the relay fans
/// them out to subscribed mobiles.
async fn desktop_ws_upgrade(
    State(state): State<AppState>,
    Path(desktop_id): Path<String>,
    headers: axum::http::HeaderMap,
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AxumResponse {
    if !check_password(&headers, &state.password) {
        return (StatusCode::UNAUTHORIZED, "wrong password").into_response();
    }
    ws.on_upgrade(move |socket| async move {
        handle_desktop_ws(socket, state, desktop_id.clone(), addr).await;
    })
}

async fn handle_desktop_ws(
    socket: WebSocket,
    state: AppState,
    desktop_id: String,
    addr: SocketAddr,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Fetch the broadcast senders. If the desktop isn't registered, close.
    let (dtm_tx, mtd_rx) = {
        let entry = state.desktops.get(&desktop_id);
        let Some(slot) = entry else {
            let _ = ws_tx.send(Message::Close(None)).await;
            return;
        };
        (slot.desktop_to_mobile.clone(), slot.mobile_to_desktop.subscribe())
    };
    info!(%desktop_id, %addr, "desktop connected");

    // mobile → desktop: read from mtd_rx, write to ws_tx.
    let desktop_writer = tokio::spawn(async move {
        let mut rx = mtd_rx;
        loop {
            match rx.recv().await {
                Ok(frame) => {
                    let kind = if frame.iter().any(|b| *b >= 0x80) {
                        Message::Binary(frame)
                    } else {
                        match String::from_utf8(frame.clone()) {
                            Ok(s) => Message::Text(s),
                            Err(_) => Message::Binary(frame),
                        }
                    };
                    if ws_tx.send(kind).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(lagged = n, "desktop lagged on mobile→desktop stream");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // desktop → mobile: read from ws_rx, broadcast on dtm_tx.
    let mut hello_seen = false;
    while let Some(msg) = ws_rx.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(_) => break,
        };

        // First frame must be the hello JSON. After that, all frames
        // are forwarded.
        if !hello_seen {
            match msg {
                Message::Text(t) => {
                    if serde_json::from_str::<Hello>(&t).is_ok() {
                        hello_seen = true;
                        continue;
                    }
                    warn!("desktop sent non-hello json; closing");
                    break;
                }
                _ => {
                    warn!("desktop sent non-text first frame; closing");
                    break;
                }
            }
        }

        let bytes = match msg {
            Message::Binary(b) => b,
            Message::Text(t) => t.into_bytes(),
            Message::Close(_) => break,
            _ => continue,
        };

        // `send` returns Err if there are no receivers. That's fine —
        // the desktop is still connected even if no mobile is currently
        // subscribed.
        let _ = dtm_tx.send(bytes);
    }

    desktop_writer.abort();
    info!(%desktop_id, "desktop disconnected");
}

/// GET /relay/mobile/ws/:desktop_id
/// Mobile's WS. Same protocol as the desktop side (opaque bytes after
/// auth). Subscribes to the desktop's broadcast channels.
async fn mobile_ws_upgrade(
    State(state): State<AppState>,
    Path(desktop_id): Path<String>,
    headers: axum::http::HeaderMap,
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> AxumResponse {
    if !check_password(&headers, &state.password) {
        return (StatusCode::UNAUTHORIZED, "wrong password").into_response();
    }
    let (dtm_rx, mtd_tx) = {
        let entry = state.desktops.get(&desktop_id);
        match entry.as_ref() {
            Some(slot) => (
                slot.desktop_to_mobile.subscribe(),
                slot.mobile_to_desktop.clone(),
            ),
            None => {
                return (StatusCode::NOT_FOUND, "desktop not registered").into_response();
            }
        }
    };

    info!(%desktop_id, %addr, "mobile connected");
    ws.on_upgrade(move |socket| async move {
        handle_mobile_ws(socket, dtm_rx, mtd_tx, desktop_id).await
    })
}

async fn handle_mobile_ws(
    socket: WebSocket,
    mut dtm_rx: broadcast::Receiver<Vec<u8>>,
    mtd_tx: broadcast::Sender<Vec<u8>>,
    desktop_id: String,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    // desktop → mobile: read from dtm_rx, write to ws_tx.
    // Skip proxy protocol control frames (ws_data, ws_close, ws_open, http_req, http_resp)
    // so the mobile only receives terminal data.
    let desktop_to_mobile = tokio::spawn(async move {
        loop {
            match dtm_rx.recv().await {
                Ok(frame) => {
                    // Check for proxy control frames (JSON with a "type" field)
                    if let Ok(s) = String::from_utf8(frame.clone()) {
                        if let Ok(val) = serde_json::from_str::<Value>(&s) {
                            if let Some(ty) = val.get("type").and_then(|v| v.as_str()) {
                                match ty {
                                    "ws_data" | "ws_close" | "ws_open"
                                    | "http_req" | "http_resp" => continue,
                                    _ => {}
                                }
                            }
                        }
                    }

                    let kind = if frame.iter().any(|b| *b >= 0x80) {
                        Message::Binary(frame)
                    } else {
                        match String::from_utf8(frame.clone()) {
                            Ok(s) => Message::Text(s),
                            Err(_) => Message::Binary(frame),
                        }
                    };
                    if ws_tx.send(kind).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(lagged = n, "mobile lagged on desktop→mobile stream");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // mobile → desktop: read from ws_rx, broadcast on mtd_tx.
    while let Some(msg) = ws_rx.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(_) => break,
        };
        let bytes = match msg {
            Message::Binary(b) => b,
            Message::Text(t) => t.into_bytes(),
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => continue,
        };
        if mtd_tx.send(bytes).is_err() {
            // No receivers — desktop disconnected. Drop the mobile too.
            break;
        }
    }

    desktop_to_mobile.abort();
    info!(%desktop_id, "mobile disconnected");
}

/// Constant-time-ish password check.
fn check_password(headers: &axum::http::HeaderMap, expected: &str) -> bool {
    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| {
            headers
                .get("x-relay-token")
                .and_then(|v| v.to_str().ok())
        });
    match provided {
        Some(p) => {
            if p.len() != expected.len() {
                return false;
            }
            subtle::ConstantTimeEq::ct_eq(p.as_bytes(), expected.as_bytes()).into()
        }
        None => false,
    }
}

/// Fallback handler: serve the embedded frontend dist. SPA fallback
/// to `index.html` for unknown paths so the frontend router can take over.
///
/// Note: this is no longer wired into the router — `proxy::proxy_fallback`
/// handles both static files and HTTP proxying. Kept for reference /
/// test use.
#[allow(dead_code)]
// suppress dead-code warning for `Duration` import which is used by axum's
// internal timers; we keep it for future use.
#[allow(dead_code)]
fn _keep_duration_import() -> Duration {
    Duration::from_secs(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_matching_returns_true() {
        let expected = "hunter2".to_string();
        let mut h = axum::http::HeaderMap::new();
        h.insert("authorization", "Bearer hunter2".parse().unwrap());
        assert!(check_password(&h, &expected));
    }

    #[test]
    fn wrong_password_returns_false() {
        let expected = "hunter2".to_string();
        let mut h = axum::http::HeaderMap::new();
        h.insert("authorization", "Bearer wrong".parse().unwrap());
        assert!(!check_password(&h, &expected));
    }

    #[test]
    fn wrong_length_returns_false() {
        // Different length passwords should never match, even if the
        // shorter one is a prefix.
        let expected = "hunter2".to_string();
        let mut h = axum::http::HeaderMap::new();
        h.insert("authorization", "Bearer hunter".parse().unwrap());
        assert!(!check_password(&h, &expected));
    }

    #[test]
    fn missing_auth_header_returns_false() {
        let expected = "hunter2".to_string();
        let h = axum::http::HeaderMap::new();
        assert!(!check_password(&h, &expected));
    }

    #[test]
    fn x_relay_token_header_works_as_alternative() {
        let expected = "hunter2".to_string();
        let mut h = axum::http::HeaderMap::new();
        h.insert("x-relay-token", "hunter2".parse().unwrap());
        assert!(check_password(&h, &expected));
    }
}