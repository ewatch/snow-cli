use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout};

use crate::snu::bridge::{BridgeConfig, BridgeError, BridgeManager, Matcher};
use crate::snu::protocol::{SnuInstance, SnuMessage, normalize_origin};

pub const DEFAULT_SNU_BROKER_ADDR: &str = "127.0.0.1:1979";
const BROKER_READY_TIMEOUT_SECS: u64 = 5;
const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 1800;
/// File name for the on-disk session cache under `~/.servicenow/`.
const SESSIONS_FILE_NAME: &str = "snu-broker-sessions.json";
/// Persisted sessions older than this are ignored on load: a `g_ck` that stale
/// is certainly dead server-side, and the refresh path would re-prompt anyway.
const PERSISTED_SESSION_MAX_AGE_SECS: u64 = 12 * 60 * 60;

/// Banner shown in the SN-Utils helper tab when ServiceNow rejects the cached
/// `g_ck`, telling the user how to mint a fresh one.
const TOKEN_EXPIRED_BANNER: &str = "snow-cli: ServiceNow rejected the saved session token. Run /token in a ServiceNow tab to refresh it.";

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
        #[serde(default)]
        origin: Option<String>,
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
        #[serde(default)]
        origin: Option<String>,
    },
    ClearSessions {
        #[serde(default)]
        origin: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cleared: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerStatus {
    pub version: String,
    pub ipc_addr: String,
    pub browser_connected: bool,
    pub session_count: usize,
    pub latest_instance_url: Option<String>,
    /// Every instance the broker currently holds a browser session for, so the
    /// caller can see which instances already have a live `g_ck` and which still
    /// need a `/token`.
    pub instances: Vec<InstanceSummary>,
    pub idle_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceSummary {
    pub url: String,
    pub origin: String,
    pub has_g_ck: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub is_latest: bool,
}

/// Shared broker handle. The helper-tab WebSocket is owned by the
/// [`BridgeManager`], which runs its own accept/read/heartbeat tasks for the
/// broker's whole lifetime, so dispatch never serializes on a bridge lock and
/// control requests (`status`/`stop`/`ping`) only ever take the short `state`
/// lock.
struct Broker {
    state: Mutex<BrokerState>,
    manager: BridgeManager,
}

impl Broker {
    fn new(idle_timeout: Duration, manager: BridgeManager) -> Self {
        Self {
            state: Mutex::new(BrokerState::new(idle_timeout)),
            manager,
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
        let mut instances: Vec<InstanceSummary> = self
            .sessions_by_origin
            .iter()
            .map(|(origin, instance)| InstanceSummary {
                url: instance.url.clone(),
                origin: origin.clone(),
                has_g_ck: instance
                    .g_ck
                    .as_deref()
                    .is_some_and(|token| !token.is_empty()),
                scope: instance.scope.clone(),
                is_latest: self.latest_origin.as_deref() == Some(origin.as_str()),
            })
            .collect();
        instances.sort_by(|a, b| a.origin.cmp(&b.origin));

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
            instances,
            idle_timeout_secs: self.idle_timeout.as_secs(),
        }
    }

    fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    fn remember_session(&mut self, instance: &SnuInstance) {
        if let Some(origin) = normalize_origin(&instance.url) {
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

    fn session_for_origin(&self, origin: &str) -> Option<SnuInstance> {
        self.sessions_by_origin.get(origin).cloned()
    }

    /// Drop the cached session for a single origin, repointing `latest_origin`
    /// at an arbitrary surviving session (or `None`) if it referenced the
    /// removed entry. Returns `true` when something was actually removed.
    fn clear_origin(&mut self, origin: &str) -> bool {
        let removed = self.sessions_by_origin.remove(origin).is_some();
        if self.latest_origin.as_deref() == Some(origin) {
            self.latest_origin = self.sessions_by_origin.keys().next().cloned();
        }
        removed
    }

    /// Drop every cached session, returning the origins that were cleared.
    fn clear_all(&mut self) -> Vec<String> {
        let cleared = self.sessions_by_origin.keys().cloned().collect();
        self.sessions_by_origin.clear();
        self.latest_origin = None;
        cleared
    }

    /// Seed the in-memory cache from a persisted snapshot at broker startup.
    /// `latest_origin` is honored when it still maps to a restored session, so
    /// the "most recently active instance" survives a broker restart; otherwise
    /// an arbitrary restored origin is used.
    fn restore_sessions(
        &mut self,
        sessions: HashMap<String, SnuInstance>,
        latest_origin: Option<String>,
    ) {
        if sessions.is_empty() {
            return;
        }
        self.latest_origin = latest_origin
            .filter(|origin| sessions.contains_key(origin))
            .or_else(|| sessions.keys().next().cloned());
        self.sessions_by_origin = sessions;
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
        origin: Option<String>,
    ) -> anyhow::Result<SnuInstance> {
        self.request(BrokerRequest::WaitSession {
            timeout_secs,
            fresh,
            origin,
        })
        .await?
        .instance
        .ok_or_else(|| anyhow!("SN-Utils broker did not return browser session metadata"))
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
    pub async fn refresh_session(
        &self,
        timeout_secs: u64,
        origin: Option<String>,
    ) -> anyhow::Result<SnuInstance> {
        self.request(BrokerRequest::RefreshSession {
            timeout_secs,
            origin,
        })
        .await?
        .instance
        .ok_or_else(|| anyhow!("SN-Utils broker did not return a refreshed session"))
    }

    /// Drop cached browser sessions from broker memory. `origin = None` clears
    /// every instance; `Some(origin)` clears just that one. Returns the origins
    /// that were actually cleared.
    pub async fn clear_sessions(&self, origin: Option<String>) -> anyhow::Result<Vec<String>> {
        Ok(self
            .request(BrokerRequest::ClearSessions { origin })
            .await?
            .cleared
            .unwrap_or_default())
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

/// Clear cached browser sessions from a running broker. Returns the cleared
/// origins, or an empty list when no broker is running (nothing to clear).
pub async fn clear_broker_sessions(origin: Option<String>) -> anyhow::Result<Vec<String>> {
    match BrokerBridge::connect_existing().await {
        Ok(bridge) => bridge.clear_sessions(origin).await,
        Err(_) => Ok(Vec::new()),
    }
}

pub async fn run_broker_server() -> anyhow::Result<()> {
    let idle_timeout = idle_timeout();
    let listener = TcpListener::bind(DEFAULT_SNU_BROKER_ADDR)
        .await
        .with_context(|| {
            format!("failed to bind SN-Utils broker IPC on {DEFAULT_SNU_BROKER_ADDR}")
        })?;
    let (manager, mut sessions_rx) = BridgeManager::start(BridgeConfig::default());
    let broker = Arc::new(Broker::new(idle_timeout, manager));

    // Reload any persisted browser sessions so a freshly (re)spawned broker
    // doesn't force a new `/token` for instances that already authenticated.
    if let Some(persisted) = load_persisted_sessions() {
        broker
            .state
            .lock()
            .await
            .restore_sessions(persisted.sessions, persisted.latest_origin);
    }

    // Cache every session the helper socket ever carries — including a `/token`
    // pushed while an unrelated request was in flight, which previously was
    // silently dropped.
    let session_broker = Arc::clone(&broker);
    tokio::spawn(async move {
        while let Some(instance) = sessions_rx.recv().await {
            session_broker
                .state
                .lock()
                .await
                .remember_session(&instance);
            persist_broker_sessions(&session_broker).await;
        }
    });

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
            cleared: None,
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
            // The manager tracks connectivity from its own accept/read loops,
            // so this reflects the actual socket state, not a guess.
            let browser_connected = broker.manager.is_connected();
            let state = broker.state.lock().await;
            Ok(BrokerResponse {
                ok: true,
                error: None,
                message: None,
                instance: None,
                status: Some(state.status(browser_connected)),
                cleared: None,
            })
        }
        BrokerRequest::Stop => {
            broker.state.lock().await.shutdown = true;
            Ok(ok_response())
        }
        BrokerRequest::SendBanner {
            message,
            timeout_secs: _,
        } => {
            // Strictly best-effort: shown immediately when a helper tab is
            // connected, queued for the next connection otherwise. Never waits
            // for a tab, so commands that can be served from the cached session
            // no longer stall here.
            broker.manager.send_banner(&message).await;
            Ok(ok_response())
        }
        BrokerRequest::WaitSession {
            timeout_secs,
            fresh,
            origin,
        } => {
            let cached = if !fresh {
                let state = broker.state.lock().await;
                match origin.as_deref() {
                    Some(origin) => state.session_for_origin(origin),
                    None => state.latest_session(),
                }
            } else {
                None
            };
            let instance = match cached {
                Some(instance) => instance,
                None => {
                    broker
                        .manager
                        .wait_for_session(timeout_secs, origin.as_deref())
                        .await?
                }
            };
            broker.state.lock().await.remember_session(&instance);
            persist_broker_sessions(broker).await;
            Ok(BrokerResponse {
                ok: true,
                error: None,
                message: None,
                instance: Some(instance),
                status: None,
                cleared: None,
            })
        }
        BrokerRequest::SendAction {
            mut payload,
            correlation_id,
            timeout_secs,
        } => {
            let message =
                send_action_with_refresh(broker, &mut payload, &correlation_id, timeout_secs)
                    .await?;
            Ok(message_response(message))
        }
        BrokerRequest::SendActionForAction {
            payload,
            expected_action,
            timeout_secs,
        } => {
            let message = broker
                .manager
                .request(&payload, Matcher::Action(expected_action), timeout_secs)
                .await?;
            Ok(message_response(message))
        }
        BrokerRequest::RefreshSession {
            timeout_secs,
            origin,
        } => {
            let instance = refresh_session(broker, timeout_secs, origin.as_deref()).await?;
            Ok(BrokerResponse {
                ok: true,
                error: None,
                message: None,
                instance: Some(instance),
                status: None,
                cleared: None,
            })
        }
        BrokerRequest::ClearSessions { origin } => {
            let cleared = {
                let mut state = broker.state.lock().await;
                match origin.as_deref() {
                    Some(origin) => {
                        if state.clear_origin(origin) {
                            vec![origin.to_string()]
                        } else {
                            Vec::new()
                        }
                    }
                    None => state.clear_all(),
                }
            };
            persist_broker_sessions(broker).await;
            Ok(BrokerResponse {
                ok: true,
                error: None,
                message: None,
                instance: None,
                status: None,
                cleared: Some(cleared),
            })
        }
    }
}

/// Send a bridge action, and if ServiceNow rejects the embedded `g_ck` as
/// expired, evict the cached session, prompt the user to re-run `/token`, splice
/// the fresh session into the payload, and retry exactly once.
async fn send_action_with_refresh(
    broker: &Broker,
    payload: &mut Value,
    correlation_id: &str,
    timeout_secs: u64,
) -> anyhow::Result<SnuMessage> {
    let first = broker
        .manager
        .request(
            payload,
            Matcher::Correlation(correlation_id.to_string()),
            timeout_secs,
        )
        .await;

    match first {
        Ok(message) => Ok(message),
        Err(BridgeError::ActionFailed(text))
            if is_stale_token_text(&text)
                && payload.get("instance").and_then(Value::as_object).is_some() =>
        {
            // Refresh only the instance this action targeted, so a `/token` from
            // a different tab in the same SN-Utils portal can't silently
            // redirect the retry to the wrong instance.
            let target_origin = payload
                .get("instance")
                .and_then(|instance| instance.get("url"))
                .and_then(Value::as_str)
                .and_then(normalize_origin);
            let fresh = refresh_session(broker, timeout_secs, target_origin.as_deref()).await?;
            if let Some(object) = payload.as_object_mut() {
                object.insert("instance".to_string(), serde_json::to_value(&fresh)?);
            }
            Ok(broker
                .manager
                .request(
                    payload,
                    Matcher::Correlation(correlation_id.to_string()),
                    timeout_secs,
                )
                .await?)
        }
        Err(error) => Err(error.into()),
    }
}

/// Evict a cached session, prompt the user via a helper-tab banner, and wait for
/// SN-Utils to push a fresh `/token`. When `origin` is `Some`, only that
/// instance's session is evicted and only a matching `/token` is accepted, so
/// other instances' cached tokens survive and the refresh can't return the wrong
/// instance. `None` falls back to evicting every session.
async fn refresh_session(
    broker: &Broker,
    timeout_secs: u64,
    origin: Option<&str>,
) -> anyhow::Result<SnuInstance> {
    {
        let mut state = broker.state.lock().await;
        match origin {
            Some(origin) => {
                state.clear_origin(origin);
            }
            None => {
                state.clear_all();
            }
        }
    }

    broker.manager.send_banner(TOKEN_EXPIRED_BANNER).await;
    let instance = broker.manager.wait_for_session(timeout_secs, origin).await?;
    broker.state.lock().await.remember_session(&instance);
    persist_broker_sessions(broker).await;
    Ok(instance)
}

/// `true` when the helper tab's error text says ServiceNow rejected the request
/// because the `g_ck` token is expired/invalid, meaning we should refresh it
/// from SN-Utils and retry. This classifies application-level error text, not
/// socket state — socket death is a typed [`BridgeError`] from the manager.
fn is_stale_token_text(text: &str) -> bool {
    let text = text.to_lowercase();
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
        cleared: None,
    }
}

fn message_response(message: SnuMessage) -> BrokerResponse {
    BrokerResponse {
        ok: true,
        error: None,
        message: Some(message),
        instance: None,
        status: None,
        cleared: None,
    }
}

/// Probe the broker IPC with a full Ping round-trip. Merely being able to
/// connect is not enough: any process could own the port, and treating a
/// foreign listener as a live broker would make every subsequent request fail
/// confusingly. Requiring a parseable `ok` response also flushes out a stale
/// broker that accepts but no longer answers.
async fn connect_once() -> anyhow::Result<()> {
    let stream = TcpStream::connect(DEFAULT_SNU_BROKER_ADDR).await?;
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let request = serde_json::to_string(&BrokerRequest::Ping)?;
    write_half.write_all(request.as_bytes()).await?;
    write_half.write_all(b"\n").await?;
    write_half.flush().await?;

    let mut line = String::new();
    timeout(Duration::from_secs(2), reader.read_line(&mut line))
        .await
        .map_err(|_| anyhow!("SN-Utils broker did not answer ping"))??;
    let response: BrokerResponse = serde_json::from_str(&line)
        .context("unexpected ping response on the SN-Utils broker port")?;
    if response.ok {
        Ok(())
    } else {
        Err(anyhow!("SN-Utils broker rejected ping"))
    }
}

fn spawn_broker() -> anyhow::Result<()> {
    let exe = std::env::current_exe().context("failed to resolve current snow-cli executable")?;
    let mut command = std::process::Command::new(exe);
    command
        .args(["snu", "broker", "serve"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    spawn_detached(command).context("failed to spawn SN-Utils broker")?;
    Ok(())
}

/// Spawn the broker detached from the spawning command's process group so it
/// outlives the `snow-cli` invocation that started it. Without this the broker
/// dies whenever the spawning command's process group is torn down (terminal
/// close, or a command runner that reaps its child group), forcing a fresh
/// `/token` on the next command.
#[cfg(unix)]
fn spawn_detached(mut command: std::process::Command) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;

    // SAFETY: `setsid` is async-signal-safe and is the only call made between
    // fork and exec. It places the child in a new session (and process group),
    // detaching it from the parent's controlling terminal and process group. It
    // cannot fail here because the freshly forked child is never already a
    // process-group leader.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    command.spawn().map(|_| ())
}

#[cfg(windows)]
fn spawn_detached(mut command: std::process::Command) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;

    // Detach from the console and start a new process group so console Ctrl
    // events don't reach the broker.
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Escape a parent Job object so the broker isn't killed when the job closes.
    // Only honored if the job sets JOB_OBJECT_LIMIT_BREAKAWAY_OK; otherwise
    // `CreateProcess` fails and we retry without it (persistence is the fallback).
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

    let base_flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
    command.creation_flags(base_flags | CREATE_BREAKAWAY_FROM_JOB);
    match command.spawn() {
        Ok(_) => Ok(()),
        Err(_) => {
            command.creation_flags(base_flags);
            command.spawn().map(|_| ())
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn spawn_detached(mut command: std::process::Command) -> std::io::Result<()> {
    command.spawn().map(|_| ())
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

/// On-disk shape of the broker's browser-session cache.
#[derive(Debug, Serialize, Deserialize)]
struct PersistedSessions {
    /// Unix epoch seconds the snapshot was written (used for the freshness guard).
    saved_at: u64,
    #[serde(default)]
    latest_origin: Option<String>,
    sessions: HashMap<String, SnuInstance>,
}

/// Session persistence is on by default; `SNOW_CLI_SNU_BROKER_PERSIST=0` opts out
/// (keeps the cache memory-only).
fn persistence_enabled() -> bool {
    !matches!(
        std::env::var("SNOW_CLI_SNU_BROKER_PERSIST").ok().as_deref(),
        Some("0")
    )
}

/// Path to the on-disk session cache, mirroring the `~/.servicenow/` convention
/// in `config::profile`. `SNOW_CLI_SNU_SESSIONS_FILE` overrides it (used by tests).
fn sessions_file_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SNOW_CLI_SNU_SESSIONS_FILE")
        && !path.is_empty()
    {
        return Some(PathBuf::from(path));
    }
    Some(
        sessions_home_dir()?
            .join(".servicenow")
            .join(SESSIONS_FILE_NAME),
    )
}

fn sessions_home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Snapshot the broker's sessions under the state lock, then write them to disk
/// outside the lock. Best-effort: persistence failures are logged, not fatal.
async fn persist_broker_sessions(broker: &Broker) {
    if !persistence_enabled() {
        return;
    }
    let (sessions, latest_origin) = {
        let state = broker.state.lock().await;
        (
            state.sessions_by_origin.clone(),
            state.latest_origin.clone(),
        )
    };
    if let Err(error) = persist_sessions(&sessions, latest_origin.as_deref()) {
        tracing::debug!(%error, "failed to persist SN-Utils broker sessions");
    }
}

fn persist_sessions(
    sessions: &HashMap<String, SnuInstance>,
    latest_origin: Option<&str>,
) -> anyhow::Result<()> {
    let Some(path) = sessions_file_path() else {
        return Ok(());
    };
    write_session_cache(&path, sessions, latest_origin, now_epoch_secs())
}

/// Write the session snapshot atomically (temp file + rename) with `0o600` on
/// unix. An empty snapshot removes the file so `snu broker clear` clears disk too.
fn write_session_cache(
    path: &Path,
    sessions: &HashMap<String, SnuInstance>,
    latest_origin: Option<&str>,
    saved_at: u64,
) -> anyhow::Result<()> {
    if sessions.is_empty() {
        if path.exists() {
            std::fs::remove_file(path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let payload = PersistedSessions {
        saved_at,
        latest_origin: latest_origin.map(str::to_string),
        sessions: sessions.clone(),
    };
    let contents = serde_json::to_string_pretty(&payload)?;
    let tmp = path.with_extension("json.tmp");
    write_private(&tmp, contents.as_bytes())?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("failed to persist sessions to {}", path.display()))?;
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes)
    }
}

/// Load the persisted session cache, or `None` when persistence is disabled, the
/// file is absent/unreadable, or the snapshot is older than the freshness guard.
fn load_persisted_sessions() -> Option<PersistedSessions> {
    if !persistence_enabled() {
        return None;
    }
    let path = sessions_file_path()?;
    let persisted = read_session_cache(&path)?;
    if !session_snapshot_is_fresh(persisted.saved_at, now_epoch_secs()) {
        return None;
    }
    Some(persisted)
}

fn read_session_cache(path: &Path) -> Option<PersistedSessions> {
    let contents = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&contents).ok()
}

fn session_snapshot_is_fresh(saved_at: u64, now: u64) -> bool {
    now.saturating_sub(saved_at) <= PERSISTED_SESSION_MAX_AGE_SECS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(url: &str) -> SnuInstance {
        SnuInstance {
            name: url.to_string(),
            url: url.to_string(),
            g_ck: Some("token".to_string()),
            scope: None,
        }
    }

    fn state() -> BrokerState {
        BrokerState::new(Duration::from_secs(300))
    }

    #[test]
    fn session_for_origin_resolves_specific_instance() {
        let mut state = state();
        state.remember_session(&instance("https://a.service-now.com"));
        state.remember_session(&instance("https://b.service-now.com"));

        let a = state
            .session_for_origin("https://a.service-now.com:443")
            .unwrap();
        assert_eq!(a.url, "https://a.service-now.com");
        // latest_session points at the most recently remembered instance...
        assert_eq!(
            state.latest_session().unwrap().url,
            "https://b.service-now.com"
        );
        // ...but origin lookup still returns the one that was asked for.
        assert!(
            state
                .session_for_origin("https://c.service-now.com:443")
                .is_none()
        );
    }

    #[test]
    fn clear_origin_keeps_other_sessions_and_fixes_latest() {
        let mut state = state();
        state.remember_session(&instance("https://a.service-now.com"));
        state.remember_session(&instance("https://b.service-now.com"));

        // b was remembered last, so it is the latest; clearing it must not wipe a.
        assert!(state.clear_origin("https://b.service-now.com:443"));
        assert!(
            state
                .session_for_origin("https://b.service-now.com:443")
                .is_none()
        );
        assert_eq!(
            state.latest_session().unwrap().url,
            "https://a.service-now.com"
        );
        // clearing a missing origin reports nothing removed.
        assert!(!state.clear_origin("https://missing.service-now.com:443"));
    }

    #[test]
    fn clear_all_empties_cache_and_reports_origins() {
        let mut state = state();
        state.remember_session(&instance("https://a.service-now.com"));
        state.remember_session(&instance("https://b.service-now.com"));

        let mut cleared = state.clear_all();
        cleared.sort();
        assert_eq!(
            cleared,
            vec![
                "https://a.service-now.com:443".to_string(),
                "https://b.service-now.com:443".to_string()
            ]
        );
        assert!(state.latest_session().is_none());
        assert_eq!(state.status(false).session_count, 0);
    }

    #[test]
    fn status_lists_all_instances_with_latest_flag() {
        let mut state = state();
        state.remember_session(&instance("https://a.service-now.com"));
        state.remember_session(&instance("https://b.service-now.com"));

        let status = state.status(true);
        assert_eq!(status.instances.len(), 2);
        let latest: Vec<&str> = status
            .instances
            .iter()
            .filter(|i| i.is_latest)
            .map(|i| i.url.as_str())
            .collect();
        assert_eq!(latest, vec!["https://b.service-now.com"]);
        assert!(status.instances.iter().all(|i| i.has_g_ck));
    }

    fn sessions_map(urls: &[&str]) -> HashMap<String, SnuInstance> {
        urls.iter()
            .map(|url| {
                let instance = instance(url);
                (normalize_origin(url).unwrap(), instance)
            })
            .collect()
    }

    #[test]
    fn session_cache_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snu-broker-sessions.json");
        let sessions = sessions_map(&["https://a.service-now.com", "https://b.service-now.com"]);
        let latest = normalize_origin("https://b.service-now.com");

        write_session_cache(&path, &sessions, latest.as_deref(), now_epoch_secs()).unwrap();
        let loaded = read_session_cache(&path).expect("cache should load");

        assert_eq!(loaded.sessions, sessions);
        assert_eq!(loaded.latest_origin, latest);
    }

    #[test]
    fn restore_sessions_prefers_persisted_latest_origin() {
        let sessions = sessions_map(&["https://a.service-now.com", "https://b.service-now.com"]);
        let latest = normalize_origin("https://a.service-now.com");

        let mut restored = state();
        restored.restore_sessions(sessions, latest.clone());

        assert_eq!(
            restored.latest_session().unwrap().url,
            "https://a.service-now.com"
        );
        // A latest_origin that no longer maps to a session falls back gracefully.
        let mut fallback = state();
        fallback.restore_sessions(
            sessions_map(&["https://c.service-now.com"]),
            Some("https://gone.service-now.com:443".to_string()),
        );
        assert_eq!(
            fallback.latest_session().unwrap().url,
            "https://c.service-now.com"
        );
    }

    #[test]
    fn empty_snapshot_removes_cache_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snu-broker-sessions.json");
        let sessions = sessions_map(&["https://a.service-now.com"]);

        write_session_cache(&path, &sessions, None, now_epoch_secs()).unwrap();
        assert!(path.exists());

        // Clearing (empty map) should remove the on-disk cache.
        write_session_cache(&path, &HashMap::new(), None, now_epoch_secs()).unwrap();
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn cache_file_is_owner_only_readable() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snu-broker-sessions.json");
        write_session_cache(
            &path,
            &sessions_map(&["https://a.service-now.com"]),
            None,
            now_epoch_secs(),
        )
        .unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn freshness_guard_rejects_stale_snapshots() {
        let now = 1_000_000;
        assert!(session_snapshot_is_fresh(now, now));
        assert!(session_snapshot_is_fresh(
            now - PERSISTED_SESSION_MAX_AGE_SECS,
            now
        ));
        assert!(!session_snapshot_is_fresh(
            now - PERSISTED_SESSION_MAX_AGE_SECS - 1,
            now
        ));
    }
}
