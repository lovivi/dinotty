#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]
use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use futures_util::StreamExt;
use notify::{PollWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn};

#[derive(Clone)]
pub struct HistoryState {
    inner: Arc<HistoryInner>,
}

struct HistoryInner {
    entries: RwLock<HashMap<String, HistoryEntry>>,
    deleted: RwLock<HashSet<String>>,
    shell_type: String,
    history_path: PathBuf,
    watcher: std::sync::Mutex<Option<PollWatcher>>,
    broadcast_tx: broadcast::Sender<String>,
}

#[derive(Clone, Copy)]
struct HistoryEntry {
    frequency: usize,
    last_used_at: i64, // unix seconds; tiebreaker for ranking when frequencies are equal
}

#[derive(Serialize, Clone)]
pub struct SuggestionItem {
    pub command: String,
    pub frequency: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<i64>,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    pub prefix: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct DeleteBody {
    pub command: String,
}

#[derive(Serialize)]
struct WsSuggestionMsg {
    r#type: String,
    items: Vec<SuggestionItem>,
}

impl Default for HistoryState {
    fn default() -> Self {
        Self::new()
    }
}

impl HistoryState {
    #[must_use]
    pub fn new() -> Self {
        let shell_type = crate::pty::get_shell_type(&crate::pty::get_shell());
        let history_path = get_history_path(&shell_type);
        let (broadcast_tx, _) = broadcast::channel(16);

        let state = Self {
            inner: Arc::new(HistoryInner {
                entries: RwLock::new(HashMap::new()),
                deleted: RwLock::new(HashSet::new()),
                shell_type,
                history_path,
                watcher: std::sync::Mutex::new(None),
                broadcast_tx,
            }),
        };

        let s = state.clone();
        tokio::spawn(async move {
            s.load_initial().await;
            s.start_watcher();
        });

        state
    }

    async fn load_initial(&self) {
        let content = match tokio::fs::read(&self.inner.history_path).await {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => {
                warn!("Failed to read history file {:?}: {}", self.inner.history_path, e);
                return;
            }
        };

        let mtime = tokio::fs::metadata(&self.inner.history_path)
            .await
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or_else(now_unix, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));

        let entries = parse_history(&self.inner.shell_type, &content, mtime);
        info!("Loaded {} unique history entries from {:?}", entries.len(), self.inner.history_path);
        *self.inner.entries.write().await = entries;
        self.broadcast_top().await;
    }

    fn start_watcher(&self) {
        let path = self.inner.history_path.clone();
        if !path.exists() {
            return;
        }

        let state = self.clone();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mut watcher = match PollWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    if event.kind.is_modify() || event.kind.is_create() {
                        let _ = tx.send(());
                    }
                }
            },
            notify::Config::default().with_poll_interval(std::time::Duration::from_secs(1)),
        ) {
            Ok(w) => w,
            Err(e) => {
                warn!("Failed to create history watcher: {}", e);
                return;
            }
        };

        if let Err(e) = watcher.watch(&path, RecursiveMode::NonRecursive) {
            warn!("Failed to watch history file: {}", e);
            return;
        }

        *self.inner.watcher.lock().expect("mutex poisoned") = Some(watcher);

        tokio::spawn(async move {
            while rx.recv().await.is_some() {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                while rx.try_recv().is_ok() {}
                state.reload_incremental().await;
            }
        });
    }

    async fn reload_incremental(&self) {
        let content = match tokio::fs::read(&self.inner.history_path).await {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => return,
        };

        let mtime = tokio::fs::metadata(&self.inner.history_path)
            .await
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or_else(now_unix, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));

        let mut entries = parse_history(&self.inner.shell_type, &content, mtime);
        let deleted = self.inner.deleted.read().await;
        for cmd in deleted.iter() {
            entries.remove(cmd);
        }
        drop(deleted);
        *self.inner.entries.write().await = entries;
        self.broadcast_top().await;
    }

    async fn broadcast_top(&self) {
        let items = self.query(None, 20).await;
        let msg = WsSuggestionMsg { r#type: "suggestions".to_string(), items };
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = self.inner.broadcast_tx.send(json);
        }
    }

    pub async fn query(&self, prefix: Option<&str>, limit: usize) -> Vec<SuggestionItem> {
        let entries = self.inner.entries.read().await;
        let mut results: Vec<_> = match prefix {
            Some(p) if !p.is_empty() => entries
                .iter()
                .filter(|(cmd, _)| cmd.contains(p))
                .map(|(cmd, &e)| SuggestionItem {
                    command: cmd.clone(),
                    frequency: e.frequency,
                    last_used_at: Some(e.last_used_at),
                })
                .collect(),
            _ => entries
                .iter()
                .map(|(cmd, &e)| SuggestionItem {
                    command: cmd.clone(),
                    frequency: e.frequency,
                    last_used_at: Some(e.last_used_at),
                })
                .collect(),
        };
        // Primary: frequency desc. Tiebreaker: recency desc (more recently
        // used wins when commands are equally frequent). This matches the
        // fish/zsh-autosuggestions behaviour where recent commands outrank
        // older ones of the same frequency.
        results.sort_by(|a, b| {
            b.frequency
                .cmp(&a.frequency)
                .then(b.last_used_at.unwrap_or(0).cmp(&a.last_used_at.unwrap_or(0)))
        });
        results.truncate(limit);
        results
    }

    pub async fn push_realtime(&self, command: &str) {
        let cmd = command.trim().to_string();
        if cmd.is_empty() {
            return;
        }
        let deleted = self.inner.deleted.read().await;
        if deleted.contains(&cmd) {
            return;
        }
        drop(deleted);
        let now = now_unix();
        let mut entries = self.inner.entries.write().await;
        let entry = entries.entry(cmd).or_insert(HistoryEntry { frequency: 0, last_used_at: now });
        entry.frequency += 1;
        entry.last_used_at = now;
        drop(entries);
        self.broadcast_top().await;
    }

    pub async fn delete(&self, command: &str) {
        self.inner.deleted.write().await.insert(command.to_string());
        self.inner.entries.write().await.remove(command);
        self.broadcast_top().await;
    }
}

fn get_history_path(shell_type: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    match shell_type {
        "zsh" => {
            if let Ok(histfile) = std::env::var("HISTFILE") {
                return PathBuf::from(histfile);
            }
            home.join(".zsh_history")
        }
        "bash" => home.join(".bash_history"),
        _ => home.join(".sh_history"),
    }
}

fn parse_history(
    shell_type: &str,
    content: &str,
    default_mtime: i64,
) -> HashMap<String, HistoryEntry> {
    let mut entries = HashMap::new();
    let bump = |entries: &mut HashMap<String, HistoryEntry>, cmd: String| {
        if !cmd.is_empty() {
            let entry = entries
                .entry(cmd)
                .or_insert(HistoryEntry { frequency: 0, last_used_at: default_mtime });
            entry.frequency += 1;
        }
    };
    match shell_type {
        "zsh" => {
            let mut continuation = String::new();
            for line in content.lines() {
                if !continuation.is_empty() {
                    if let Some(stripped) = line.strip_suffix('\\') {
                        continuation.push('\n');
                        continuation.push_str(stripped);
                        continue;
                    }
                    continuation.push('\n');
                    continuation.push_str(line);
                    bump(&mut entries, continuation.trim().to_string());
                    continuation.clear();
                    continue;
                }

                let raw = if line.starts_with(": ") {
                    line.find(';').map_or("", |i| &line[i + 1..])
                } else {
                    line
                };

                if let Some(stripped) = raw.strip_suffix('\\') {
                    continuation = stripped.to_string();
                    continue;
                }

                bump(&mut entries, raw.trim().to_string());
            }
            if !continuation.is_empty() {
                bump(&mut entries, continuation.trim().to_string());
            }
        }
        _ => {
            for line in content.lines() {
                bump(&mut entries, line.trim().to_string());
            }
        }
    }
    entries
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bash_history_counts_frequencies() {
        let content = "ls -la\ngit status\ngit push\nls -la\ngit status\n";
        let entries = parse_history("bash", content, 1_000);
        assert_eq!(entries.get("ls -la").unwrap().frequency, 2);
        assert_eq!(entries.get("git status").unwrap().frequency, 2);
        assert_eq!(entries.get("git push").unwrap().frequency, 1);
        // All entries seeded with the supplied mtime on first load.
        assert!(entries.values().all(|e| e.last_used_at == 1_000));
    }

    #[test]
    fn parse_zsh_history_strips_metadata_line() {
        // zsh extended history format: ": <ts>:0;<cmd>"
        let content = ": 1700000000:0;git pull\n: 1700000001:0;git push\n";
        let entries = parse_history("zsh", content, 1_000);
        assert_eq!(entries.get("git pull").unwrap().frequency, 1);
        assert_eq!(entries.get("git push").unwrap().frequency, 1);
    }

    #[test]
    fn parse_history_merges_continuation_lines() {
        // zsh extended format with backslash-continuation.
        // The first line ends with `\`, the second is `world`. After
        // continuation merging the entry is `echo hello \nworld` (with a
        // literal newline char between `hello` and `world`).
        let content = ": 1700000000:0;echo hello \\\nworld\n";
        let entries = parse_history("zsh", content, 1_000);
        assert_eq!(entries.len(), 1);
        let expected = "echo hello \nworld".to_string();
        assert!(entries.contains_key(&expected));
    }

    #[test]
    fn recency_tiebreaker_picks_more_recent() {
        // Build entries manually so we can pin last_used_at.
        let mtime = 1_000;
        let mut entries: HashMap<String, HistoryEntry> = HashMap::new();
        entries.insert("git status".into(), HistoryEntry { frequency: 5, last_used_at: mtime });
        entries.insert("git pull".into(), HistoryEntry { frequency: 5, last_used_at: mtime + 100 });
        // Simulate the sort key the way `query()` does.
        let mut list: Vec<_> = entries
            .iter()
            .map(|(cmd, e)| SuggestionItem {
                command: cmd.clone(),
                frequency: e.frequency,
                last_used_at: Some(e.last_used_at),
            })
            .collect();
        list.sort_by(|a, b| {
            b.frequency
                .cmp(&a.frequency)
                .then(b.last_used_at.unwrap_or(0).cmp(&a.last_used_at.unwrap_or(0)))
        });
        // Same frequency → more recent wins.
        assert_eq!(list[0].command, "git pull");
        assert_eq!(list[1].command, "git status");
    }

    #[test]
    fn frequency_still_beats_recency() {
        // Even if `git status` is older, much higher frequency should win.
        let mut entries: HashMap<String, HistoryEntry> = HashMap::new();
        entries.insert("git status".into(), HistoryEntry { frequency: 100, last_used_at: 1_000 });
        entries.insert("git pull".into(), HistoryEntry { frequency: 5, last_used_at: 9_999 });
        let mut list: Vec<_> = entries
            .iter()
            .map(|(cmd, e)| SuggestionItem {
                command: cmd.clone(),
                frequency: e.frequency,
                last_used_at: Some(e.last_used_at),
            })
            .collect();
        list.sort_by(|a, b| {
            b.frequency
                .cmp(&a.frequency)
                .then(b.last_used_at.unwrap_or(0).cmp(&a.last_used_at.unwrap_or(0)))
        });
        assert_eq!(list[0].command, "git status");
    }
}

pub async fn get_history(
    State(state): State<HistoryState>,
    axum::extract::Query(query): axum::extract::Query<HistoryQuery>,
) -> Json<Vec<SuggestionItem>> {
    let limit = query.limit.unwrap_or(20).min(100);
    let results = state.query(query.prefix.as_deref(), limit).await;
    Json(results)
}

pub async fn delete_history(
    State(state): State<HistoryState>,
    Json(body): Json<DeleteBody>,
) -> StatusCode {
    state.delete(&body.command).await;
    StatusCode::NO_CONTENT
}

#[allow(clippy::unused_async)]
pub async fn ws_history_handler(
    ws: WebSocketUpgrade,
    State(state): State<HistoryState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_history_ws(socket, state))
}

async fn handle_history_ws(mut socket: WebSocket, state: HistoryState) {
    let mut rx = state.inner.broadcast_tx.subscribe();

    // Send current top suggestions immediately
    let items = state.query(None, 20).await;
    let msg = WsSuggestionMsg { r#type: "suggestions".to_string(), items };
    if let Ok(json) = serde_json::to_string(&msg) {
        if socket.send(Message::Text(json)).await.is_err() {
            return;
        }
    }

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(json) => {
                        if socket.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {},
                    Err(_) => break,
                }
            }
            msg = socket.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}
