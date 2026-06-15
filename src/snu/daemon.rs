use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, UnixListener, UnixStream};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::timeout;
use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite::Message};

use crate::snu::bridge::DEFAULT_SNU_WS_ADDR;
use crate::snu::protocol::{SnuInstance, SnuMessage, redact_session_for_output};

// ---------------------------------------------------------------------------
// State file
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeState {
    pub pid: u32,
    pub socket_path: String,
    pub ws_port: u16,
    pub started_at_secs: u64,
    pub heartbeat_at_secs: u64,
}

// ---------------------------------------------------------------------------
// IPC protocol
// ---------------------------------------------------------------------------

/// Request from CLI to daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonRequest {
    pub id: String,
    pub cmd: String,
    #[serde(default)]
    pub payload: Value,
}

/// Response from daemon to CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonResponse {
    pub id: String,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl DaemonResponse {
    pub fn ok(id: &str, data: Value) -> Self {
        Self { id: id.to_string(), success: true, data: Some(data), error: None }
    }
    pub fn err(id: &str, msg: impl Into<String>) -> Self {
        Self { id: id.to_string(), success: false, data: None, error: Some(msg.into()) }
    }
}

/// Internal message type for the daemon actor loop.
enum DaemonMessage {
    /// A CLI request that needs to be processed serially with the WS.
    Request {
        req: DaemonRequest,
        respond: oneshot::Sender<DaemonResponse>,
    },
    /// Shut down the daemon.
    Shutdown,
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

fn servicenow_dir() -> PathBuf {
    if let Some(home) = home_dir() {
        home.join(".servicenow")
    } else {
        PathBuf::from(".servicenow")
    }
}

pub fn socket_path() -> PathBuf {
    servicenow_dir().join("snu-bridge.sock")
}

fn state_path() -> PathBuf {
    servicenow_dir().join("bridge.json")
}

fn pid_path() -> PathBuf {
    servicenow_dir().join("bridge.pid")
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    { std::env::var("USERPROFILE").ok().map(PathBuf::from) }
    #[cfg(not(target_os = "windows"))]
    { std::env::var("HOME").ok().map(PathBuf::from) }
}

// ---------------------------------------------------------------------------
// State file I/O
// ---------------------------------------------------------------------------

pub fn read_state() -> Option<BridgeState> {
    let content = std::fs::read_to_string(state_path()).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_state(state: &BridgeState) -> anyhow::Result<()> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(state)?)?;
    Ok(())
}

fn clear_state() {
    let _ = std::fs::remove_file(state_path());
    let _ = std::fs::remove_file(pid_path());
}

// ---------------------------------------------------------------------------
// PID file
// ---------------------------------------------------------------------------

fn write_pid() -> anyhow::Result<()> {
    let path = pid_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, format!("{}\n", std::process::id()))?;
    Ok(())
}

fn read_pid() -> Option<u32> {
    let content = std::fs::read_to_string(pid_path()).ok()?;
    content.trim().parse().ok()
}

fn pid_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .output()
            .ok()
            .is_some_and(|o| o.status.success())
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

fn stop_pid(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .arg(pid.to_string())
            .output()
            .ok()
            .is_some_and(|o| o.status.success())
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

// ---------------------------------------------------------------------------
// Public API: check if daemon is running
// ---------------------------------------------------------------------------

pub fn is_running() -> bool {
    let sock = socket_path();
    if sock.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::net::UnixStream as SyncUnixStream;
            if let Ok(mut s) = SyncUnixStream::connect(&sock) {
                use std::io::Write;
                let _ = s.write_all(b"\n");
                return true;
            }
        }
        #[cfg(not(unix))]
        {
            // On non-Unix platforms, just check the PID file
        }
        // Socket exists but nobody listening — stale
        let _ = std::fs::remove_file(&sock);
    }

    if let Some(pid) = read_pid() {
        if pid_is_alive(pid) {
            // PID alive but socket dead — stale
            clear_state();
            return false;
        }
        clear_state();
    }
    false
}

// ---------------------------------------------------------------------------
// Public API: client sends a request to the daemon
// ---------------------------------------------------------------------------

pub async fn send_request(request: &DaemonRequest) -> anyhow::Result<DaemonResponse> {
    let sock = socket_path();
    let mut stream = UnixStream::connect(&sock)
        .await
        .with_context(|| format!("bridge daemon not running (socket not found at {})", sock.display()))?;

    let line = serde_json::to_string(request)?;
    stream.write_all(line.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;

    let mut reader = BufReader::new(&mut stream);
    let mut response_line = String::new();
    reader.read_line(&mut response_line).await?;

    if response_line.trim().is_empty() {
        anyhow::bail!("bridge daemon closed connection without response");
    }

    let response: DaemonResponse = serde_json::from_str(response_line.trim())
        .with_context(|| format!("invalid JSON from bridge daemon: {response_line}"))?;

    Ok(response)
}

// ---------------------------------------------------------------------------
// Public API: start the daemon (blocking, runs forever)
// ---------------------------------------------------------------------------

/// Run the bridge daemon process.
/// 1. Binds WebSocket on :1978
/// 2. Binds Unix socket for CLI IPC
/// 3. Accepts one browser connection over WebSocket
/// 4. Processes CLI requests through an actor loop
/// 5. Cleans up on exit
pub async fn run_daemon(timeout_secs: u64) -> anyhow::Result<()> {
    write_pid().context("failed to write PID file")?;

    // --- Bind WebSocket server ---
    let ws_listener = TcpListener::bind(DEFAULT_SNU_WS_ADDR)
        .await
        .with_context(|| format!("could not bind SN-Utils bridge on {DEFAULT_SNU_WS_ADDR}; another bridge or sn-scriptsync may be running"))?;
    eprintln!("SN-Utils bridge listening on ws://{DEFAULT_SNU_WS_ADDR}");

    // --- Bind Unix socket ---
    let sock_path = socket_path();
    let _ = std::fs::remove_file(&sock_path);
    if let Some(parent) = sock_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let unix_listener = UnixListener::bind(&sock_path)
        .with_context(|| format!("failed to bind Unix socket at {}", sock_path.display()))?;

    // --- Write state ---
    write_state(&BridgeState {
        pid: std::process::id(),
        socket_path: sock_path.to_string_lossy().to_string(),
        ws_port: 1978,
        started_at_secs: now_secs(),
        heartbeat_at_secs: now_secs(),
    })?;

    // --- Accept browser WebSocket connection ---
    eprintln!("Waiting for SN-Utils helper tab to connect...");
    let ws = accept_ws_with_timeout(&ws_listener, timeout_secs).await
        .context("timed out waiting for SN-Utils helper tab to connect")?;
    let peer_addr = ws.peer_addr;
    eprintln!("SN-Utils helper tab connected from {peer_addr}");

    // Wrap WS in a Mutex so the actor can access it
    let ws_mutex = Arc::new(Mutex::new(ws.socket));

    // Channel between CLI connection handlers and the actor
    let (tx, mut rx) = mpsc::unbounded_channel::<DaemonMessage>();

    // Track the cached session for quick lookup
    let cached_instance = Arc::new(tokio::sync::watch::Sender::new(
        crate::snu::session_cache::load_session()
            .ok()
            .flatten()
            .map(|c| c.instance),
    ));
    let cached_instance_watch = cached_instance.subscribe();

    // --- Actor task: processes one CLI request at a time ---
    let actor_ws = ws_mutex.clone();
    let actor_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match msg {
                DaemonMessage::Shutdown => break,
                DaemonMessage::Request { req, respond } => {
                    let result = process_request(&req, &actor_ws, &cached_instance_watch).await;
                    let _ = respond.send(result);
                }
            }
        }
    });

    // --- Unix socket listener: accept CLI connections ---
    let tx_for_unix = tx.clone();
    let tx_for_ws = tx.clone();
    let sock_path_for_unix = sock_path.clone();
    let sock_path_for_cleanup = sock_path.clone();
    let unix_handle = tokio::spawn(async move {
        loop {
            let stream = match timeout(Duration::from_secs(1), unix_listener.accept()).await {
                Ok(Ok((s, _))) => s,
                Ok(Err(_)) => continue,
                Err(_) => continue,
            };

            // Update heartbeat
            let _ = write_state(&BridgeState {
                pid: std::process::id(),
                socket_path: sock_path_for_unix.to_string_lossy().to_string(),
                ws_port: 1978,
                started_at_secs: now_secs(),
                heartbeat_at_secs: now_secs(),
            });

            let tx = tx_for_unix.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_cli_connection(stream, tx).await {
                    tracing::warn!("CLI connection handler error: {e}");
                }
            });
        }
    });

    // --- WebSocket keep-alive: handle Ping/Pong/Close ---
    let ws_keepalive = ws_mutex.clone();
    let ws_handle = tokio::spawn(async move {
        loop {
            let mut ws = ws_keepalive.lock().await;
            let msg = timeout(Duration::from_secs(30), ws.next()).await;
            match msg {
                Ok(Some(Ok(Message::Ping(bytes)))) => {
                    let _ = ws.send(Message::Pong(bytes)).await;
                }
                Ok(Some(Ok(Message::Pong(_)))) => {}
                Ok(Some(Ok(Message::Close(frame)))) => {
                    tracing::info!("Browser helper tab closed connection: {frame:?}");
                    break;
                }
                Ok(Some(Ok(Message::Text(text)))) => {
                    try_cache_token_message(&text, &cached_instance);
                    tracing::debug!("Browser message (not handled): {text}");
                }
                Ok(Some(Ok(Message::Binary(data)))) => {
                    if let Ok(text) = String::from_utf8(data.to_vec()) {
                        try_cache_token_message(&text, &cached_instance);
                    }
                }
                Ok(Some(Ok(Message::Frame(_)))) => {}
                Ok(Some(Err(e))) => {
                    tracing::error!("WebSocket error: {e}");
                    break;
                }
                Ok(None) => {
                    tracing::info!("Browser helper tab disconnected");
                    break;
                }
                Err(_) => {} // timeout, continue loop
            }
            drop(ws); // release lock before next iteration
        }
        // Signal shutdown
        let _ = tx_for_ws.send(DaemonMessage::Shutdown);
    });

    // Wait for any handle to finish
    tokio::select! {
        _ = actor_handle => {}
        _ = unix_handle => {}
        _ = ws_handle => {}
    }

    // Cleanup
    let _ = std::fs::remove_file(&sock_path_for_cleanup);
    clear_state();
    eprintln!("SN-Utils bridge daemon stopped");
    Ok(())
}

// ---------------------------------------------------------------------------
// Accept a WebSocket connection with timeout
// ---------------------------------------------------------------------------

struct AcceptedWs {
    socket: WebSocketStream<tokio::net::TcpStream>,
    peer_addr: std::net::SocketAddr,
}

async fn accept_ws_with_timeout(
    listener: &TcpListener,
    timeout_secs: u64,
) -> anyhow::Result<AcceptedWs> {
    let accept_future = async {
        let (stream, peer_addr) = listener.accept().await?;
        let socket = accept_async(stream).await?;
        anyhow::Ok(AcceptedWs { socket, peer_addr })
    };
    timeout(Duration::from_secs(timeout_secs), accept_future)
        .await
        .map_err(|_| anyhow!("timed out waiting for SN-Utils helper tab connection"))?
}

// ---------------------------------------------------------------------------
// Handle a single CLI connection
// ---------------------------------------------------------------------------

async fn handle_cli_connection(
    mut stream: UnixStream,
    tx: mpsc::UnboundedSender<DaemonMessage>,
) -> anyhow::Result<()> {
    let mut reader = BufReader::new(&mut stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;

    if line.trim().is_empty() {
        return Ok(());
    }

    let request: DaemonRequest = serde_json::from_str(line.trim())
        .with_context(|| format!("invalid request from CLI: {line}"))?;

    // Handle commands that don't need the WebSocket immediately
    let response = match request.cmd.as_str() {
        "stop" => {
            let resp = DaemonResponse::ok(&request.id, json!({"status": "stopping"}));
            write_line_response(&mut stream, &resp).await?;
            std::process::exit(0);
        }
        "status" => {
            let state = read_state();
            let session = crate::snu::session_cache::load_session().ok().flatten();
            let data = json!({
                "running": true,
                "pid": std::process::id(),
                "state": state,
                "session": session.map(|s| redact_session_for_output(&s.instance)),
            });
            DaemonResponse::ok(&request.id, data)
        }
        "session" => {
            match crate::snu::session_cache::load_session() {
                Ok(Some(cached)) => {
                    let data = json!({
                        "has_session": true,
                        "instance_url": cached.instance.url,
                        "instance_name": cached.instance.name,
                        "has_g_ck": true,
                        "scope": cached.instance.scope,
                    });
                    DaemonResponse::ok(&request.id, data)
                }
                Ok(None) => DaemonResponse::ok(&request.id, json!({"has_session": false})),
                Err(e) => DaemonResponse::err(&request.id, format!("session error: {e}")),
            }
        }
        // These need to go through the actor for WS access
        "wait-for-session" | "bridge-action" => {
            let (resp_tx, resp_rx) = oneshot::channel();
            tx.send(DaemonMessage::Request {
                req: request,
                respond: resp_tx,
            })?;
            match resp_rx.await {
                Ok(response) => response,
                Err(_) => DaemonResponse::err("", "daemon actor dropped the request"),
            }
        }
        other => DaemonResponse::err(&request.id, format!("unknown daemon command: {other}")),
    };

    write_line_response(&mut stream, &response).await
}

async fn write_line_response(stream: &mut UnixStream, response: &DaemonResponse) -> anyhow::Result<()> {
    let line = serde_json::to_string(response)?;
    stream.write_all(line.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Process a request inside the actor (serialized access to WS)
// ---------------------------------------------------------------------------

async fn process_request(
    req: &DaemonRequest,
    ws: &Arc<Mutex<WebSocketStream<tokio::net::TcpStream>>>,
    _cached_instance: &tokio::sync::watch::Receiver<Option<SnuInstance>>,
) -> DaemonResponse {
    match req.cmd.as_str() {
        "wait-for-session" => {
            // Check cache first
            if let Ok(Some(cached)) = crate::snu::session_cache::load_session() {
                return DaemonResponse::ok(&req.id, json!({
                    "has_session": true,
                    "instance_url": cached.instance.url,
                    "instance_name": cached.instance.name,
                    "has_g_ck": cached.instance.g_ck.as_deref().is_some_and(|t| !t.is_empty()),
                    "scope": cached.instance.scope,
                }));
            }

            // Wait for session from the WS
            let timeout_secs = req.payload.get("timeout_secs").and_then(Value::as_u64).unwrap_or(180);
            let mut ws_lock = ws.lock().await;
            let read_loop = async {
                loop {
                    let msg = read_ws_json_message(&mut ws_lock).await?;
                    if let Some(instance) = msg.instance
                        && instance.g_ck.as_deref().is_some_and(|t| !t.is_empty())
                    {
                        let _ = crate::snu::session_cache::store_session(&instance);
                        return Ok::<_, anyhow::Error>(instance);
                    }
                }
            };
            match timeout(Duration::from_secs(timeout_secs), read_loop).await {
                Ok(Ok(instance)) => DaemonResponse::ok(&req.id, json!({
                    "has_session": true,
                    "instance_url": instance.url,
                    "instance_name": instance.name,
                    "has_g_ck": true,
                    "scope": instance.scope,
                })),
                Ok(Err(e)) => DaemonResponse::err(&req.id, e.to_string()),
                Err(_) => DaemonResponse::err(&req.id, format!("timed out waiting {timeout_secs}s for /token")),
            }
        }
        "bridge-action" => {
            // Forward a bridge action over the WebSocket
            let action = req.payload.get("action").and_then(Value::as_str).unwrap_or("unknown");
            let correlation_id = req.payload.get("agentRequestId")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let timeout_secs = req.payload.get("timeout_secs").and_then(Value::as_u64).unwrap_or(180);

            let mut ws_lock = ws.lock().await;
            // Send the payload
            let payload_text = match serde_json::to_string(&req.payload) {
                Ok(t) => t,
                Err(e) => return DaemonResponse::err(&req.id, format!("failed to serialize payload: {e}")),
            };
            if let Err(e) = ws_lock.send(Message::Text(payload_text)).await {
                return DaemonResponse::err(&req.id, format!("failed to send WebSocket message: {e}"));
            }

            // Wait for response with matching correlation_id
            let read_loop = async {
                loop {
                    let msg = read_ws_json_message(&mut ws_lock).await?;
                    if msg.agent_request_id.as_deref() == Some(correlation_id)
                        || msg.action.as_deref() == Some(action)
                    {
                        if msg.success == Some(false) || msg.error.is_some() {
                            return Err(anyhow!("SN-Utils action failed: {}", msg.error_text().unwrap_or_else(|| "unknown error".to_string())));
                        }
                        return Ok(msg);
                    }
                }
            };

            match timeout(Duration::from_secs(timeout_secs), read_loop).await {
                Ok(Ok(msg)) => {
                    let mut value = serde_json::to_value(&msg).unwrap_or(json!({}));
                    if let Value::Object(ref mut map) = value {
                        map.remove("agentRequestId");
                    }
                    DaemonResponse::ok(&req.id, value)
                }
                Ok(Err(e)) => DaemonResponse::err(&req.id, e.to_string()),
                Err(_) => DaemonResponse::err(&req.id, format!("timed out waiting {timeout_secs}s for response to {action}")),
            }
        }
        _ => DaemonResponse::err(&req.id, format!("unknown actor command: {}", req.cmd)),
    }
}

// ---------------------------------------------------------------------------
// Read a JSON message from the WebSocket (same logic as bridge.rs)
// ---------------------------------------------------------------------------

async fn read_ws_json_message(
    socket: &mut WebSocketStream<tokio::net::TcpStream>,
) -> anyhow::Result<SnuMessage> {
    loop {
        let Some(message) = socket.next().await else {
            return Err(anyhow!("SN-Utils helper tab disconnected"));
        };
        let message = message?;
        match message {
            Message::Text(text) => {
                let value: Value = serde_json::from_str(&text)
                    .with_context(|| format!("invalid JSON from SN-Utils: {text}"))?;
                if value.is_array() {
                    tracing::debug!("ignoring SN-Utils informational array message");
                    continue;
                }
                return SnuMessage::from_value(value);
            }
            Message::Binary(bytes) => {
                let value: Value = serde_json::from_slice(&bytes)
                    .context("invalid binary JSON from SN-Utils")?;
                return SnuMessage::from_value(value);
            }
            Message::Ping(bytes) => {
                socket.send(Message::Pong(bytes)).await?;
            }
            Message::Pong(_) => {}
            Message::Close(frame) => {
                return Err(anyhow!("SN-Utils helper tab closed connection: {frame:?}"));
            }
            Message::Frame(_) => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

// ---------------------------------------------------------------------------
// CLI helpers
// ---------------------------------------------------------------------------

/// Stop the running daemon.
pub async fn stop_daemon() -> anyhow::Result<()> {
    // Try sending stop command via Unix socket
    if let Ok(resp) = send_request(&DaemonRequest {
        id: "stop".into(),
        cmd: "stop".into(),
        payload: Value::Null,
    }).await
        && resp.success
    {
        return Ok(());
    }

    // Fallback: kill by PID
    if let Some(pid) = read_pid() {
        if pid_is_alive(pid) {
            if stop_pid(pid) {
                clear_state();
                eprintln!("Stopped bridge daemon (PID {pid})");
                return Ok(());
            }
            anyhow::bail!("failed to stop bridge daemon (PID {pid})");
        }
        clear_state();
        return Ok(());
    }

    anyhow::bail!("bridge daemon is not running");
}

/// Try to extract a /token session message from WebSocket text and cache it.
fn try_cache_token_message(text: &str, cached_instance: &tokio::sync::watch::Sender<Option<SnuInstance>>) {
    let Ok(value) = serde_json::from_str::<Value>(text) else { return };
    let Some(instance_val) = value.get("instance") else { return };
    let Ok(inst) = serde_json::from_value::<SnuInstance>(instance_val.clone()) else { return };
    if inst.g_ck.as_deref().is_none_or(|t| t.is_empty()) { return }
    let _ = crate::snu::session_cache::store_session(&inst);
    let _ = cached_instance.send(Some(inst));
}
