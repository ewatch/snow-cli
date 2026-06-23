use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout};

use crate::snu::bridge::SnuBridge;
use crate::snu::protocol::{SnuInstance, SnuMessage};

pub const DEFAULT_SNU_BROKER_ADDR: &str = "127.0.0.1:1979";
const BROKER_READY_TIMEOUT_SECS: u64 = 5;
const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 300;

/// Banner shown in the SN-Utils helper tab when ServiceNow rejects the cached
/// `g_ck`, telling the user how to mint a fresh one.
const TOKEN_EXPIRED_BANNER: &str =
    "snow-cli: ServiceNow rejected the saved session token. Run /token in a ServiceNow tab to refresh it.";

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BrokerRequest {
    Ping,
    Status,
    Stop,
    SendBanner {
        message: String,
        timeout_secs: u64,
    },
    WaitSession {
        timeout_secs: u64,
        fresh: bool,
    },
    SendPayload {
        payload: Value,
        timeout_secs: u64,
    },
    SendAction {
        payload: Value,
        correlation_id: String,
        timeout_secs: u64,
    },
    SendActionForAction {
        payload: Value,
        expected_action: String,
        timeout_secs: u64,
    },
    RefreshSession {
        timeout_secs: u64,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct BrokerResponse {
    ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message: Option<SnuMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instance: Option<SnuInstance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<BrokerStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerStatus {
    pub version: String,
    pub ipc_addr: String,
    pub browser_connected: bool,
    pub session_count: usize,
    pub latest_instance_url: Option<String>,
    pub idle_timeout_secs: u64,
}

/// Shared broker handle. The live helper-tab socket lives behind its own mutex,
/// separate from the bookkeeping `state`, so a long-running bridge action holds
/// only the `bridge` lock — control requests (`status`/`stop`/`ping`) and
/// session bookkeeping take the short `state` lock and never queue behind it.
/// `bridge_connected` mirrors whether `bridge` holds a live socket so `status`
/// can report it without ever touching (and blocking on) the `bridge` lock.
struct Broker {
    state: Mutex<BrokerState>,
    bridge: Mutex<Option<SnuBridge>>,
    bridge_connected: AtomicBool,
}

impl Broker {
    fn new(idle_timeout: Duration) -> Self {
        Self {
            state: Mutex::new(BrokerState::new(idle_timeout)),
            bridge: Mutex::new(None),
            bridge_connected: AtomicBool::new(false),
        }
    }
}

struct BrokerState {
    sessions_by_origin: HashMap<String, SnuInstance>,
    latest_origin: Option<String>,
    last_activity: Instant,
    active_clients: usize,
    in_flight: usize,
    shutdown: bool,
    idle_timeout: Duration,
}

impl BrokerState {
    fn new(idle_timeout: Duration) -> Self {
        Self {
            sessions_by_origin: HashMap::new(),
            latest_origin: None,
            last_activity: Instant::now(),
            active_clients: 0,
            in_flight: 0,
            shutdown: false,
            idle_timeout,
        }
    }

    fn status(&self, browser_connected: bool) -> BrokerStatus {
        BrokerStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            ipc_addr: DEFAULT_SNU_BROKER_ADDR.to_string(),
            browser_connected,
            session_count: self.sessions_by_origin.len(),
            latest_instance_url: self
                .latest_origin
                .as_ref()
                .and_then(|origin| self.sessions_by_origin.get(origin))
                .map(|instance| instance.url.clone()),
            idle_timeout_secs: self.idle_timeout.as_secs(),
        }
    }

    fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    fn remember_session(&mut self, instance: &SnuInstance) {
        if let Some(origin) = normalize_instance_origin(&instance.url) {
            self.sessions_by_origin
                .insert(origin.clone(), instance.clone());
            self.latest_origin = Some(origin);
        }
    }

    fn latest_session(&self) -> Option<SnuInstance> {
        self.latest_origin
            .as_ref()
            .and_then(|origin| self.sessions_by_origin.get(origin))
            .cloned()
    }
}

pub struct BrokerBridge {
    addr: String,
}

impl BrokerBridge {
    pub async fn connect_or_spawn() -> anyhow::Result<Self> {
        if connect_once().await.is_err() {
            spawn_broker()?;
            wait_until_ready().await?;
        }

        Ok(Self {
            addr: DEFAULT_SNU_BROKER_ADDR.to_string(),
        })
    }

    pub async fn connect_existing() -> anyhow::Result<Self> {
        connect_once().await?;
        Ok(Self {
            addr: DEFAULT_SNU_BROKER_ADDR.to_string(),
        })
    }

    pub async fn send_banner(&self, message: &str, timeout_secs: u64) -> anyhow::Result<()> {
        self.request(BrokerRequest::SendBanner {
            message: message.to_string(),
            timeout_secs,
        })
        .await?;
        Ok(())
    }

    pub async fn wait_for_session(
        &self,
        timeout_secs: u64,
        fresh: bool,
    ) -> anyhow::Result<SnuInstance> {
        self.request(BrokerRequest::WaitSession {
            timeout_secs,
            fresh,
        })
        .await?
        .instance
        .ok_or_else(|| anyhow!("SN-Utils broker did not return browser session metadata"))
    }

    pub async fn send_payload_and_wait(
        &self,
        payload: &Value,
        timeout_secs: u64,
    ) -> anyhow::Result<SnuMessage> {
        self.request(BrokerRequest::SendPayload {
            payload: payload.clone(),
            timeout_secs,
        })
        .await?
        .message
        .ok_or_else(|| anyhow!("SN-Utils broker did not return a helper message"))
    }

    pub async fn send_action_and_wait(
        &self,
        payload: &Value,
        correlation_id: &str,
        timeout_secs: u64,
    ) -> anyhow::Result<SnuMessage> {
        self.request(BrokerRequest::SendAction {
            payload: payload.clone(),
            correlation_id: correlation_id.to_string(),
            timeout_secs,
        })
        .await?
        .message
        .ok_or_else(|| anyhow!("SN-Utils broker did not return an action response"))
    }

    pub async fn send_action_and_wait_for_action(
        &self,
        payload: &Value,
        expected_action: &str,
        timeout_secs: u64,
    ) -> anyhow::Result<SnuMessage> {
        self.request(BrokerRequest::SendActionForAction {
            payload: payload.clone(),
            expected_action: expected_action.to_string(),
            timeout_secs,
        })
        .await?
        .message
        .ok_or_else(|| anyhow!("SN-Utils broker did not return an action response"))
    }

    /// Evict the cached browser session and capture a fresh one (prompting the
    /// user via a helper-tab banner to re-run `/token`). Used by the direct-HTTP
    /// record paths to recover from an expired `g_ck`.
    pub async fn refresh_session(&self, timeout_secs: u64) -> anyhow::Result<SnuInstance> {
        self.request(BrokerRequest::RefreshSession { timeout_secs })
            .await?
            .instance
            .ok_or_else(|| anyhow!("SN-Utils broker did not return a refreshed session"))
    }

    async fn request(&self, request: BrokerRequest) -> anyhow::Result<BrokerResponse> {
        let stream = TcpStream::connect(&self.addr)
            .await
            .with_context(|| format!("failed to connect to SN-Utils broker at {}", self.addr))?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let raw = serde_json::to_string(&request)?;
        write_half.write_all(raw.as_bytes()).await?;
        write_half.write_all(b"\n").await?;
        write_half.flush().await?;

        let mut line = String::new();
        reader.read_line(&mut line).await?;
        if line.trim().is_empty() {
            return Err(anyhow!(
                "SN-Utils broker closed the connection without a response"
            ));
        }

        let response: BrokerResponse =
            serde_json::from_str(&line).context("invalid JSON response from SN-Utils broker")?;
        if response.ok {
            Ok(response)
        } else {
            Err(anyhow!(
                "{}",
                response
                    .error
                    .unwrap_or_else(|| "SN-Utils broker request failed".to_string())
            ))
        }
    }
}

pub async fn broker_status() -> anyhow::Result<BrokerStatus> {
    BrokerBridge::connect_existing()
        .await?
        .request(BrokerRequest::Status)
        .await?
        .status
        .ok_or_else(|| anyhow!("SN-Utils broker did not return status"))
}

pub async fn stop_broker() -> anyhow::Result<()> {
    BrokerBridge::connect_existing()
        .await?
        .request(BrokerRequest::Stop)
        .await?;
    Ok(())
}

pub async fn run_broker_server() -> anyhow::Result<()> {
    let idle_timeout = idle_timeout();
    let listener = TcpListener::bind(DEFAULT_SNU_BROKER_ADDR)
        .await
        .with_context(|| {
            format!("failed to bind SN-Utils broker IPC on {DEFAULT_SNU_BROKER_ADDR}")
        })?;
    let broker = Arc::new(Broker::new(idle_timeout));
    let idle_broker = Arc::clone(&broker);

    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(5)).await;
            let mut state = idle_broker.state.lock().await;
            if state.shutdown {
                break;
            }
            if state.active_clients == 0
                && state.in_flight == 0
                && state.last_activity.elapsed() >= state.idle_timeout
            {
                state.shutdown = true;
                break;
            }
        }
    });

    loop {
        if broker.state.lock().await.shutdown {
            break;
        }

        let accept = timeout(Duration::from_secs(1), listener.accept()).await;
        let Ok(Ok((stream, _peer_addr))) = accept else {
            continue;
        };
        let client_broker = Arc::clone(&broker);
        tokio::spawn(async move {
            if let Err(error) = handle_client(stream, client_broker).await {
                tracing::debug!(%error, "SN-Utils broker client failed");
            }
        });
    }

    Ok(())
}

async fn handle_client(stream: TcpStream, broker: Arc<Broker>) -> anyhow::Result<()> {
    {
        let mut state = broker.state.lock().await;
        state.active_clients += 1;
        state.touch();
    }

    let result = handle_client_inner(stream, Arc::clone(&broker)).await;

    {
        let mut state = broker.state.lock().await;
        state.active_clients = state.active_clients.saturating_sub(1);
        state.touch();
    }

    result
}

async fn handle_client_inner(stream: TcpStream, broker: Arc<Broker>) -> anyhow::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    if line.trim().is_empty() {
        return Ok(());
    }

    let request: BrokerRequest =
        serde_json::from_str(&line).context("invalid SN-Utils broker request")?;
    let response = process_request(request, &broker).await;
    let response = match response {
        Ok(response) => response,
        Err(error) => BrokerResponse {
            ok: false,
            error: Some(error.to_string()),
            message: None,
            instance: None,
            status: None,
        },
    };

    write_half
        .write_all(serde_json::to_string(&response)?.as_bytes())
        .await?;
    write_half.write_all(b"\n").await?;
    write_half.flush().await?;
    Ok(())
}

async fn process_request(
    request: BrokerRequest,
    broker: &Broker,
) -> anyhow::Result<BrokerResponse> {
    {
        let mut state = broker.state.lock().await;
        state.in_flight += 1;
        state.touch();
    }

    let result = dispatch(request, broker).await;

    {
        let mut state = broker.state.lock().await;
        state.in_flight = state.in_flight.saturating_sub(1);
        state.touch();
    }
    result
}

async fn dispatch(request: BrokerRequest, broker: &Broker) -> anyhow::Result<BrokerResponse> {
    match request {
        BrokerRequest::Ping => Ok(ok_response()),
        BrokerRequest::Status => {
            // Read connectivity from the atomic mirror so `status` never blocks
            // on the bridge lock while a long-running action holds it.
            let browser_connected = broker.bridge_connected.load(Ordering::Relaxed);
            let state = broker.state.lock().await;
            Ok(BrokerResponse {
                ok: true,
                error: None,
                message: None,
                instance: None,
                status: Some(state.status(browser_connected)),
            })
        }
        BrokerRequest::Stop => {
            broker.state.lock().await.shutdown = true;
            Ok(ok_response())
        }
        BrokerRequest::SendBanner {
            message,
            timeout_secs,
        } => {
            let mut guard = broker.bridge.lock().await;
            let result = {
                let bridge = ensure_bridge(broker, &mut guard, timeout_secs).await?;
                bridge.send_banner(&message).await
            };
            clear_bridge_on_disconnect(broker, &mut guard, result)?;
            Ok(ok_response())
        }
        BrokerRequest::WaitSession {
            timeout_secs,
            fresh,
        } => {
            let cached = if !fresh {
                broker.state.lock().await.latest_session()
            } else {
                None
            };
            let instance = match cached {
                Some(instance) => instance,
                None => {
                    let mut guard = broker.bridge.lock().await;
                    let result = {
                        let bridge = ensure_bridge(broker, &mut guard, timeout_secs).await?;
                        bridge.wait_for_session(timeout_secs).await
                    };
                    clear_bridge_on_disconnect(broker, &mut guard, result)?
                }
            };
            broker.state.lock().await.remember_session(&instance);
            Ok(BrokerResponse {
                ok: true,
                error: None,
                message: None,
                instance: Some(instance),
                status: None,
            })
        }
        BrokerRequest::SendPayload {
            payload,
            timeout_secs,
        } => {
            let message = {
                let mut guard = broker.bridge.lock().await;
                let result = {
                    let bridge = ensure_bridge(broker, &mut guard, timeout_secs).await?;
                    bridge.send_payload_and_wait(&payload, timeout_secs).await
                };
                clear_bridge_on_disconnect(broker, &mut guard, result)?
            };
            remember_message_session(broker, &message).await;
            Ok(message_response(message))
        }
        BrokerRequest::SendAction {
            mut payload,
            correlation_id,
            timeout_secs,
        } => {
            let message =
                send_action_with_refresh(broker, &mut payload, &correlation_id, timeout_secs)
                    .await?;
            remember_message_session(broker, &message).await;
            Ok(message_response(message))
        }
        BrokerRequest::SendActionForAction {
            payload,
            expected_action,
            timeout_secs,
        } => {
            let message = {
                let mut guard = broker.bridge.lock().await;
                let result = {
                    let bridge = ensure_bridge(broker, &mut guard, timeout_secs).await?;
                    bridge
                        .send_action_and_wait_for_action(&payload, &expected_action, timeout_secs)
                        .await
                };
                clear_bridge_on_disconnect(broker, &mut guard, result)?
            };
            remember_message_session(broker, &message).await;
            Ok(message_response(message))
        }
        BrokerRequest::RefreshSession { timeout_secs } => {
            let mut guard = broker.bridge.lock().await;
            let instance = refresh_session(broker, &mut guard, timeout_secs).await?;
            Ok(BrokerResponse {
                ok: true,
                error: None,
                message: None,
                instance: Some(instance),
                status: None,
            })
        }
    }
}

/// Send a bridge action, and if ServiceNow rejects the embedded `g_ck` as
/// expired, evict the cached session, prompt the user to re-run `/token`, splice
/// the fresh session into the payload, and retry exactly once. Holds the bridge
/// lock for the whole exchange so the single helper socket is used exclusively.
async fn send_action_with_refresh(
    broker: &Broker,
    payload: &mut Value,
    correlation_id: &str,
    timeout_secs: u64,
) -> anyhow::Result<SnuMessage> {
    let mut guard = broker.bridge.lock().await;

    let first = {
        let bridge = ensure_bridge(broker, &mut guard, timeout_secs).await?;
        bridge
            .send_action_and_wait(payload, correlation_id, timeout_secs)
            .await
    };
    let first = clear_bridge_on_disconnect(broker, &mut guard, first);

    match first {
        Ok(message) => Ok(message),
        Err(error)
            if is_stale_token_error(&error)
                && payload.get("instance").and_then(Value::as_object).is_some() =>
        {
            let fresh = refresh_session(broker, &mut guard, timeout_secs).await?;
            if let Some(object) = payload.as_object_mut() {
                object.insert("instance".to_string(), serde_json::to_value(&fresh)?);
            }
            let retry = {
                let bridge = ensure_bridge(broker, &mut guard, timeout_secs).await?;
                bridge
                    .send_action_and_wait(payload, correlation_id, timeout_secs)
                    .await
            };
            clear_bridge_on_disconnect(broker, &mut guard, retry)
        }
        Err(error) => Err(error),
    }
}

/// Evict every cached session, prompt the user via a helper-tab banner, and wait
/// for SN-Utils to push a fresh `/token`. Operates on the caller-held bridge
/// `guard`.
async fn refresh_session(
    broker: &Broker,
    guard: &mut Option<SnuBridge>,
    timeout_secs: u64,
) -> anyhow::Result<SnuInstance> {
    {
        let mut state = broker.state.lock().await;
        state.sessions_by_origin.clear();
        state.latest_origin = None;
    }

    let result = {
        let bridge = ensure_bridge(broker, guard, timeout_secs).await?;
        let _ = bridge.send_banner(TOKEN_EXPIRED_BANNER).await;
        bridge.wait_for_session(timeout_secs).await
    };
    let instance = clear_bridge_on_disconnect(broker, guard, result)?;
    broker.state.lock().await.remember_session(&instance);
    Ok(instance)
}

async fn ensure_bridge<'a>(
    broker: &Broker,
    guard: &'a mut Option<SnuBridge>,
    timeout_secs: u64,
) -> anyhow::Result<&'a mut SnuBridge> {
    if guard.is_none() {
        *guard = Some(SnuBridge::accept(timeout_secs).await?);
        broker.bridge_connected.store(true, Ordering::Relaxed);
    }
    guard
        .as_mut()
        .ok_or_else(|| anyhow!("SN-Utils broker has no active helper bridge"))
}

/// Drop the cached helper bridge when an operation fails because the socket
/// itself is gone (the SN-Utils tab reloaded or closed), so the next request
/// re-`accept`s the freshly reconnected tab instead of reusing a dead socket.
/// Without this a single tab reload would wedge the broker until its idle
/// timeout. Logical failures (action rejected, response timeout, expired token)
/// are deliberately *not* treated as disconnects: clearing a still-connected
/// bridge would block on an `accept` that never gets a second connection.
/// `ensure_bridge` failures don't reach here because they never cache a bridge.
fn clear_bridge_on_disconnect<T>(
    broker: &Broker,
    guard: &mut Option<SnuBridge>,
    result: anyhow::Result<T>,
) -> anyhow::Result<T> {
    if let Err(error) = &result
        && is_bridge_disconnect_error(error)
    {
        *guard = None;
        broker.bridge_connected.store(false, Ordering::Relaxed);
    }
    result
}

/// `true` when an error indicates the helper-tab WebSocket is gone (as opposed
/// to a logical/application error over a still-live socket).
fn is_bridge_disconnect_error(error: &anyhow::Error) -> bool {
    let text = error.to_string().to_lowercase();
    text.contains("disconnected")
        || text.contains("connection reset")
        || text.contains("reset by peer")
        || text.contains("broken pipe")
        || text.contains("closed connection")
        || text.contains("connection closed")
        || text.contains("not connected")
        || text.contains("connection refused")
}

/// `true` when ServiceNow rejected the request because the `g_ck` token is
/// expired/invalid, meaning we should refresh it from SN-Utils and retry.
fn is_stale_token_error(error: &anyhow::Error) -> bool {
    let text = error.to_string().to_lowercase();
    text.contains("http 401")
        || text.contains("http 403")
        || text.contains("not authenticated")
        || text.contains("unauthorized")
        || text.contains("forbidden")
}

fn ok_response() -> BrokerResponse {
    BrokerResponse {
        ok: true,
        error: None,
        message: None,
        instance: None,
        status: None,
    }
}

fn message_response(message: SnuMessage) -> BrokerResponse {
    BrokerResponse {
        ok: true,
        error: None,
        message: Some(message),
        instance: None,
        status: None,
    }
}

async fn remember_message_session(broker: &Broker, message: &SnuMessage) {
    if let Some(instance) = &message.instance {
        broker.state.lock().await.remember_session(instance);
    }
}

async fn connect_once() -> anyhow::Result<()> {
    let mut stream = TcpStream::connect(DEFAULT_SNU_BROKER_ADDR).await?;
    let request = serde_json::to_string(&BrokerRequest::Ping)?;
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;
    Ok(())
}

fn spawn_broker() -> anyhow::Result<()> {
    let exe = std::env::current_exe().context("failed to resolve current snow-cli executable")?;
    std::process::Command::new(exe)
        .args(["snu", "broker", "serve"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to spawn SN-Utils broker")?;
    Ok(())
}

async fn wait_until_ready() -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(BROKER_READY_TIMEOUT_SECS);
    loop {
        if connect_once().await.is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow!(
                "timed out waiting for SN-Utils broker on {DEFAULT_SNU_BROKER_ADDR}"
            ));
        }
        sleep(Duration::from_millis(100)).await;
    }
}

fn idle_timeout() -> Duration {
    std::env::var("SNOW_CLI_SNU_BROKER_IDLE_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECS))
}

fn normalize_instance_origin(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let port = parsed.port_or_known_default()?;
    Some(format!("{}://{}:{}", parsed.scheme(), host, port))
}
