use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, bail};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep, timeout};
use tokio_tungstenite::{
    WebSocketStream, accept_async,
    tungstenite::{Bytes, Message},
};

use crate::snu::gates::{
    CapabilityNegotiation, GateDecision, GateMode, GateSnapshot, HelperBuildInfo,
    HelperCapabilitiesReport, HelperLicenseInfo, HelperSecurityGateSupport, HelperStatus,
    PreflightResult, gate_for_payload, payload_origin,
};
use crate::snu::protocol::{SnuInstance, SnuMessage, normalize_origin};

pub const DEFAULT_SNU_WS_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1978);

/// Resolves the SN-Utils bridge WebSocket address, honoring `SNOW_CLI_SNU_WS_ADDR`
/// when set. This is a single machine-wide port by default (the helper tab has
/// no way to discover a non-default one), so an override is the only way to run
/// an isolated broker instance — e.g. driving a headless browser in tests
/// without evicting a real, already-connected ScriptSync tab.
pub fn ws_addr() -> anyhow::Result<SocketAddr> {
    env_loopback_socket_addr("SNOW_CLI_SNU_WS_ADDR", DEFAULT_SNU_WS_ADDR)
}

pub(crate) fn env_loopback_socket_addr(
    variable: &str,
    default: SocketAddr,
) -> anyhow::Result<SocketAddr> {
    match std::env::var(variable) {
        Ok(value) => loopback_socket_addr(&value, variable),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error).with_context(|| format!("failed to read {variable}")),
    }
}

fn loopback_socket_addr(value: &str, source: &str) -> anyhow::Result<SocketAddr> {
    let addr = value
        .parse::<SocketAddr>()
        .with_context(|| format!("{source} must be an IP socket address, got '{value}'"))?;
    if !addr.ip().is_loopback() {
        bail!("{source} must use a loopback IP address, got '{addr}'");
    }
    Ok(addr)
}

/// Maximum time to wait for the SN-Utils ScriptSync helper tab to *connect* to
/// the bridge. This is deliberately short and separate from the per-action
/// response timeout: an installed, open helper tab reconnects within ~1s, so a
/// long wait here only ever penalizes the "SN-Utils is not running" case. The
/// full `timeout_secs` budget still applies to waiting for `/token` and action
/// replies.
pub const HELPER_CONNECT_TIMEOUT_SECS: u64 = 20;

const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
const DEFAULT_REBIND_DELAY: Duration = Duration::from_secs(3);

/// Bridge failure classification. Connection-state errors (`NotConnected`,
/// `Disconnected`, `PortConflict`) are produced by the connection manager
/// itself; `ActionFailed` carries whatever error text the helper tab reported
/// over a perfectly healthy socket.
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error(
        "timed out waiting {0}s for the SN-Utils ScriptSync helper tab to connect on ws://{1}. Is SN-Utils installed and the ScriptSync helper tab open? It auto-connects within ~1s when running."
    )]
    NotConnected(u64, SocketAddr),
    #[error("SN-Utils helper tab disconnected")]
    Disconnected,
    #[error("timed out waiting {secs}s for {what}")]
    Timeout { secs: u64, what: String },
    #[error("SN-Utils action failed: {0}")]
    ActionFailed(String),
    #[error("SN-Utils agent access is paused in the helper tab; resume agents there and retry")]
    Paused,
    #[error("SN-Utils security-gate state is unavailable: {0}")]
    GateStateUnavailable(String),
    #[error("SN-Utils helper has not authorized instance {0}")]
    GateUnauthorized(String),
    #[error("SN-Utils security gate {gate} blocks actions for {origin}")]
    GateBlocked { gate: String, origin: String },
    #[error("timed out awaiting browser approval for SN-Utils security gate {gate} on {origin}")]
    ApprovalTimeout { gate: String, origin: String },
    #[error(
        "SN-Utils bridge port {0} is already in use. Only one bridge can own this port at a time, so this usually means the `sn-scriptsync` VS Code extension or another process is bound to it. Stop the other owner and retry; the broker keeps retrying the port automatically."
    )]
    PortConflict(SocketAddr),
    #[error("failed to encode SN-Utils payload: {0}")]
    Encode(#[from] serde_json::Error),
}

impl BridgeError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Paused => "SNU_PAUSED",
            Self::GateStateUnavailable(_) => "SNU_GATE_STATE_UNAVAILABLE",
            Self::GateUnauthorized(_) => "SNU_GATE_UNAUTHORIZED",
            Self::GateBlocked { .. } => "SNU_GATE_BLOCKED",
            Self::ApprovalTimeout { .. } => "SNU_APPROVAL_TIMEOUT",
            _ => "SNU_BRIDGE_ERROR",
        }
    }
}

/// What a pending request is waiting for on the helper-tab socket.
pub enum Matcher {
    /// A reply whose `agentRequestId` equals the given correlation id.
    Correlation(String),
    /// A message whose `action` equals the given name.
    Action(String),
    /// The very next parseable message (legacy `check_connection`-style flows).
    NextMessage,
    /// An instance push carrying a non-empty `g_ck` (`/token`), optionally
    /// restricted to one normalized origin.
    Session { origin: Option<String> },
}

impl Matcher {
    fn matches(&self, msg: &SnuMessage) -> bool {
        match self {
            Matcher::Correlation(id) => msg.is_response_for(id),
            Matcher::Action(action) => msg.action.as_deref() == Some(action.as_str()),
            Matcher::NextMessage => true,
            Matcher::Session { origin } => msg
                .instance
                .as_ref()
                .is_some_and(|instance| instance_matches_session(instance, origin.as_deref())),
        }
    }

    /// Human description used in timeout error messages.
    fn describe(&self) -> String {
        match self {
            Matcher::Correlation(id) => format!("SN-Utils response {id}"),
            Matcher::Action(action) => format!("SN-Utils action {action}"),
            Matcher::NextMessage => "SN-Utils response".to_string(),
            Matcher::Session {
                origin: Some(origin),
            } => format!("/token from SN-Utils for {origin}"),
            Matcher::Session { origin: None } => "/token from SN-Utils".to_string(),
        }
    }

    /// Whether a matched message should still be inspected for the helper's
    /// `success == false` / `error` action-failure convention.
    fn checks_action_failure(&self) -> bool {
        matches!(self, Matcher::Correlation(_) | Matcher::Action(_))
    }
}

fn instance_matches_session(instance: &SnuInstance, target_origin: Option<&str>) -> bool {
    let has_token = instance
        .g_ck
        .as_deref()
        .is_some_and(|token| !token.is_empty());
    if !has_token {
        return false;
    }
    match target_origin {
        Some(target) => normalize_origin(&instance.url).as_deref() == Some(target),
        None => true,
    }
}

pub struct BridgeConfig {
    pub addr: SocketAddr,
    pub heartbeat_interval: Duration,
    pub connect_timeout: Duration,
    pub rebind_delay: Duration,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            addr: DEFAULT_SNU_WS_ADDR,
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
            connect_timeout: Duration::from_secs(HELPER_CONNECT_TIMEOUT_SECS),
            rebind_delay: DEFAULT_REBIND_DELAY,
        }
    }
}

impl BridgeConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            addr: ws_addr()?,
            ..Self::default()
        })
    }
}

struct Waiter {
    /// `Some(gen)` waiters die with that connection (request/reply pairs);
    /// `None` waiters (session waiters) survive helper-tab reconnects.
    generation: Option<u64>,
    matcher: Matcher,
    tx: oneshot::Sender<Result<SnuMessage, BridgeError>>,
}

struct Conn {
    generation: u64,
    outbound: mpsc::UnboundedSender<Message>,
    reader: JoinHandle<()>,
    writer: JoinHandle<()>,
    /// Last time any frame arrived from the helper tab; the heartbeat task
    /// kills the connection when this goes stale (half-open socket).
    last_seen: Arc<std::sync::Mutex<Instant>>,
}

struct Inner {
    config: BridgeConfig,
    connected_tx: watch::Sender<bool>,
    bound_addr_tx: watch::Sender<Option<SocketAddr>>,
    conn: Mutex<Option<Conn>>,
    waiters: Mutex<Vec<Waiter>>,
    /// Every instance+`g_ck` observed on the socket — regardless of which
    /// request (if any) is in flight — so a `/token` is never dropped.
    sessions_tx: mpsc::UnboundedSender<SnuInstance>,
    /// `false` while another process owns the WebSocket port.
    ws_bound: AtomicBool,
    generation: AtomicU64,
    /// Metadata and gate snapshots belonging only to the active WebSocket
    /// generation. Replaced atomically whenever a helper reconnects.
    helper_status: Mutex<HelperStatus>,
    /// Banner queued while no helper tab is connected, delivered on the next
    /// connection so banners never block waiting for an accept.
    pending_banner: Mutex<Option<String>>,
}

/// Broker-owned SN-Utils connection manager. Owns the WebSocket listener for
/// the broker's whole lifetime: the helper tab (a reconnecting client) can
/// attach at any moment, a new connection transparently replaces a dead one,
/// and a heartbeat reaps half-open sockets. Requests register a [`Matcher`]
/// and are answered by whichever reply the read loop routes to them.
#[derive(Clone)]
pub struct BridgeManager {
    inner: Arc<Inner>,
}

impl BridgeManager {
    /// Start the manager's background tasks (accept loop + heartbeat) and
    /// return it together with the stream of sessions observed on the socket.
    /// Must be called from within a tokio runtime.
    pub fn start(config: BridgeConfig) -> (Self, mpsc::UnboundedReceiver<SnuInstance>) {
        let (sessions_tx, sessions_rx) = mpsc::unbounded_channel();
        let (connected_tx, _) = watch::channel(false);
        let (bound_addr_tx, _) = watch::channel(None);
        let inner = Arc::new(Inner {
            config,
            connected_tx,
            bound_addr_tx,
            conn: Mutex::new(None),
            waiters: Mutex::new(Vec::new()),
            sessions_tx,
            // Optimistic until the first bind attempt (which happens
            // immediately) reports otherwise.
            ws_bound: AtomicBool::new(true),
            generation: AtomicU64::new(0),
            helper_status: Mutex::new(HelperStatus::default()),
            pending_banner: Mutex::new(None),
        });

        tokio::spawn(accept_loop(Arc::clone(&inner)));
        tokio::spawn(heartbeat_loop(Arc::clone(&inner)));

        (Self { inner }, sessions_rx)
    }

    pub fn is_connected(&self) -> bool {
        *self.inner.connected_tx.borrow()
    }

    /// Return redacted metadata for the active helper connection generation.
    pub async fn helper_status(&self) -> HelperStatus {
        self.inner.helper_status.lock().await.clone()
    }

    /// Wait until the listener is bound, returning the actual local address.
    /// Mainly for tests binding port 0.
    pub async fn wait_bound(&self, max_wait: Duration) -> Option<SocketAddr> {
        let mut rx = self.inner.bound_addr_tx.subscribe();
        timeout(max_wait, async {
            loop {
                if let Some(addr) = *rx.borrow_and_update() {
                    return addr;
                }
                if rx.changed().await.is_err() {
                    std::future::pending::<()>().await;
                }
            }
        })
        .await
        .ok()
    }

    /// Wait for a helper tab to be connected, up to `max_wait`.
    pub async fn wait_connected(&self, max_wait: Duration) -> Result<(), BridgeError> {
        if !self.inner.ws_bound.load(Ordering::Relaxed) {
            return Err(BridgeError::PortConflict(self.inner.config.addr));
        }
        let mut rx = self.inner.connected_tx.subscribe();
        let wait = async {
            loop {
                if *rx.borrow_and_update() {
                    return;
                }
                if rx.changed().await.is_err() {
                    std::future::pending::<()>().await;
                }
            }
        };
        timeout(max_wait, wait).await.map_err(|_| {
            if self.inner.ws_bound.load(Ordering::Relaxed) {
                BridgeError::NotConnected(max_wait.as_secs(), self.inner.config.addr)
            } else {
                BridgeError::PortConflict(self.inner.config.addr)
            }
        })
    }

    /// Actively refresh helper capabilities for the current generation. Helpers
    /// that answer without security-gate support are explicitly legacy; a
    /// current helper advertising gates fails closed when refresh is unavailable.
    pub async fn negotiate_capabilities(
        &self,
        timeout_secs: u64,
    ) -> Result<CapabilityNegotiation, BridgeError> {
        let connect_budget = Duration::from_secs(
            timeout_secs.min(self.inner.config.connect_timeout.as_secs().max(1)),
        );
        self.wait_connected(connect_budget).await?;
        let before = self.helper_status().await;
        let correlation_id = format!("snow_caps_{}", uuid::Uuid::new_v4().simple());
        let payload = serde_json::json!({
            "action": "agentGetCapabilities",
            "agentRequestId": correlation_id,
            "appName": "snow-cli",
        });
        let negotiation_timeout = timeout_secs.clamp(1, 2);
        let response = self
            .request(
                &payload,
                Matcher::Correlation(correlation_id),
                negotiation_timeout,
            )
            .await;
        let response = match response {
            Ok(response) => response,
            Err(BridgeError::Timeout { .. }) => {
                let after = self.helper_status().await;
                if after.generation != before.generation {
                    return Err(BridgeError::Disconnected);
                }
                if before.advertises_instance_gates() || after.advertises_instance_gates() {
                    return Err(BridgeError::GateStateUnavailable(
                        "helper capability refresh timed out".to_string(),
                    ));
                }
                let mut status = self.inner.helper_status.lock().await;
                if status.generation != before.generation {
                    return Err(BridgeError::Disconnected);
                }
                if status.advertises_instance_gates()
                    || status.security_gate_support == HelperSecurityGateSupport::Gated
                {
                    return Err(BridgeError::GateStateUnavailable(
                        "helper advertised instance security gates while capability refresh timed out"
                            .to_string(),
                    ));
                }
                status.security_gate_support = HelperSecurityGateSupport::LegacyUnrestricted;
                return Ok(CapabilityNegotiation::Legacy {
                    generation: status.generation,
                });
            }
            Err(BridgeError::Paused) => return Err(BridgeError::Paused),
            Err(error) => {
                let after = self.helper_status().await;
                if before.advertises_instance_gates() || after.advertises_instance_gates() {
                    return Err(BridgeError::GateStateUnavailable(
                        "helper capability refresh failed".to_string(),
                    ));
                }
                return Err(error);
            }
        };
        let report = HelperCapabilitiesReport::from_value(Value::Object(response.extra))
            .map_err(|error| BridgeError::GateStateUnavailable(error.to_string()))?;
        if !report.supports_instance_gates() {
            if before.advertises_instance_gates()
                || before.security_gate_support == HelperSecurityGateSupport::Gated
            {
                return Err(BridgeError::GateStateUnavailable(
                    "helper advertised instance security gates but its capability refresh omitted them"
                        .to_string(),
                ));
            }
            let mut status = self.inner.helper_status.lock().await;
            if status.generation != before.generation {
                return Err(BridgeError::Disconnected);
            }
            if status.advertises_instance_gates()
                || status.security_gate_support == HelperSecurityGateSupport::Gated
            {
                return Err(BridgeError::GateStateUnavailable(
                    "helper advertised instance security gates during capability refresh but its response omitted them"
                        .to_string(),
                ));
            }
            status.security_gate_support = HelperSecurityGateSupport::LegacyUnrestricted;
            return Ok(CapabilityNegotiation::Legacy {
                generation: status.generation,
            });
        }

        let mut status = self.inner.helper_status.lock().await;
        if status.generation != before.generation {
            return Err(BridgeError::Disconnected);
        }
        status.security_gate_support = HelperSecurityGateSupport::Gated;
        let refreshed_instance_gates = report.instance_gates;
        for (origin, snapshot) in &refreshed_instance_gates {
            status.apply_gate_snapshot(origin.clone(), snapshot.clone());
        }
        let mut negotiated = status.clone();
        // Authorization for this dispatch comes only from the active refresh.
        // Cached push snapshots remain useful for status display, but an origin
        // omitted by the refresh must not remain authorized from old state.
        negotiated.instance_gates = refreshed_instance_gates;
        Ok(CapabilityNegotiation::Gated(Box::new(negotiated)))
    }

    /// Produce a redacted, advisory decision immediately before dispatch. A
    /// gated helper omitting the selected origin is unauthorized, not legacy.
    pub async fn preflight(
        &self,
        payload: &Value,
        timeout_secs: u64,
    ) -> Result<PreflightResult, BridgeError> {
        let Some(gate) = gate_for_payload(payload) else {
            return Ok(PreflightResult {
                generation: None,
                origin: payload_origin(payload),
                gate: None,
                decision: GateDecision::NotGated,
            });
        };
        let origin = payload_origin(payload);
        match self.negotiate_capabilities(timeout_secs).await? {
            CapabilityNegotiation::Legacy { generation } => Ok(PreflightResult {
                generation: Some(generation),
                origin,
                gate: Some(gate),
                decision: GateDecision::LegacyUnrestricted,
            }),
            CapabilityNegotiation::Gated(status) => {
                let origin = origin.ok_or_else(|| {
                    BridgeError::GateStateUnavailable(format!(
                        "action requires {gate}, but its instance origin is unknown"
                    ))
                })?;
                let snapshot = status
                    .instance_gates
                    .get(&origin)
                    .ok_or_else(|| BridgeError::GateUnauthorized(origin.clone()))?;
                let decision = match snapshot.gates.mode(gate) {
                    GateMode::Off => GateDecision::Blocked,
                    GateMode::Approve => GateDecision::ApprovalRequired,
                    GateMode::Auto => GateDecision::Automatic,
                };
                Ok(PreflightResult {
                    generation: Some(status.generation),
                    origin: Some(origin),
                    gate: Some(gate),
                    decision,
                })
            }
        }
    }

    /// Show a banner in the helper tab, strictly best-effort: sent immediately
    /// when a tab is connected, otherwise queued for the next connection.
    /// Never waits for a connection and never fails the caller.
    pub async fn send_banner(&self, message: &str) {
        let conn = self.inner.conn.lock().await;
        match conn.as_ref() {
            Some(conn) => {
                let _ = conn.outbound.send(banner_message(message));
            }
            None => {
                *self.inner.pending_banner.lock().await = Some(message.to_string());
            }
        }
    }

    /// Send a payload and wait for the message selected by `matcher`.
    /// Waits up to `min(timeout_secs, connect_timeout)` for a helper tab to be
    /// connected first, then up to `timeout_secs` for the reply.
    pub async fn request(
        &self,
        payload: &Value,
        matcher: Matcher,
        timeout_secs: u64,
    ) -> Result<SnuMessage, BridgeError> {
        self.request_on_generation(payload, matcher, timeout_secs, None)
            .await
    }

    /// Send only if the active helper is still the generation whose gate state
    /// was preflighted. The generation check and socket enqueue happen under
    /// the same connection lock, closing the reconnect race between them.
    pub async fn request_on_generation(
        &self,
        payload: &Value,
        matcher: Matcher,
        timeout_secs: u64,
        expected_generation: Option<u64>,
    ) -> Result<SnuMessage, BridgeError> {
        let connect_budget = Duration::from_secs(
            timeout_secs.min(self.inner.config.connect_timeout.as_secs().max(1)),
        );
        self.wait_connected(connect_budget).await?;

        let what = matcher.describe();
        let checks_failure = matcher.checks_action_failure();
        let text = serde_json::to_string(payload)?;

        let rx = {
            // Register the waiter and enqueue the send under the conn lock so a
            // connection replacement can't slip between the two.
            let conn_guard = self.inner.conn.lock().await;
            let Some(conn) = conn_guard.as_ref() else {
                return Err(BridgeError::Disconnected);
            };
            if expected_generation.is_some_and(|expected| expected != conn.generation) {
                return Err(BridgeError::Disconnected);
            }
            let (tx, rx) = oneshot::channel();
            self.inner.waiters.lock().await.push(Waiter {
                generation: Some(conn.generation),
                matcher,
                tx,
            });
            conn.outbound
                .send(Message::Text(text.into()))
                .map_err(|_| BridgeError::Disconnected)?;
            rx
        };

        let message = timeout(Duration::from_secs(timeout_secs), rx)
            .await
            .map_err(|_| BridgeError::Timeout {
                secs: timeout_secs,
                what,
            })?
            .map_err(|_| BridgeError::Disconnected)??;

        if checks_failure && (message.success == Some(false) || message.error.is_some()) {
            if message.error_code() == Some("E_PAUSED") {
                return Err(BridgeError::Paused);
            }
            return Err(BridgeError::ActionFailed(
                message
                    .error_text()
                    .unwrap_or_else(|| "unknown error".to_string()),
            ));
        }
        Ok(message)
    }

    /// Wait for SN-Utils to push a `/token`, optionally restricted to one
    /// normalized origin. The waiter survives helper-tab reconnects (a tab
    /// reload while waiting for `/token` is fine); it fails fast with
    /// `NotConnected` only when no tab connects at all within the connect
    /// budget.
    pub async fn wait_for_session(
        &self,
        timeout_secs: u64,
        target_origin: Option<&str>,
    ) -> Result<SnuInstance, BridgeError> {
        if !self.inner.ws_bound.load(Ordering::Relaxed) {
            return Err(BridgeError::PortConflict(self.inner.config.addr));
        }

        let matcher = Matcher::Session {
            origin: target_origin.map(str::to_string),
        };
        let what = matcher.describe();
        let (tx, rx) = oneshot::channel();
        self.inner.waiters.lock().await.push(Waiter {
            generation: None,
            matcher,
            tx,
        });

        let connect_budget = Duration::from_secs(
            timeout_secs.min(self.inner.config.connect_timeout.as_secs().max(1)),
        );

        let message = tokio::select! {
            result = timeout(Duration::from_secs(timeout_secs), rx) => {
                result
                    .map_err(|_| BridgeError::Timeout { secs: timeout_secs, what })?
                    .map_err(|_| BridgeError::Disconnected)??
            }
            _ = self.never_connected_within(connect_budget) => {
                return Err(BridgeError::NotConnected(
                    connect_budget.as_secs(),
                    self.inner.config.addr,
                ));
            }
        };

        message.instance.ok_or(BridgeError::Disconnected)
    }

    /// Resolves only if no helper tab connects within `budget` (and none was
    /// connected to begin with); pends forever otherwise.
    async fn never_connected_within(&self, budget: Duration) {
        let mut rx = self.inner.connected_tx.subscribe();
        let wait_connected = async {
            loop {
                if *rx.borrow_and_update() {
                    return;
                }
                if rx.changed().await.is_err() {
                    std::future::pending::<()>().await;
                }
            }
        };
        if timeout(budget, wait_connected).await.is_ok() {
            std::future::pending::<()>().await;
        }
    }
}

fn host_hello_message() -> Message {
    Message::Text(
        serde_json::json!({
            "action": "hostHello",
            "hostKind": "standalone",
            "appName": "snow-cli",
            "protocolVersion": 1,
        })
        .to_string()
        .into(),
    )
}

fn banner_message(message: &str) -> Message {
    Message::Text(
        serde_json::json!({
            "action": "bannerMessage",
            "message": message,
            "class": "alert alert-primary",
        })
        .to_string()
        .into(),
    )
}

async fn accept_loop(inner: Arc<Inner>) {
    loop {
        let listener = match TcpListener::bind(&inner.config.addr).await {
            Ok(listener) => listener,
            Err(error) => {
                inner.ws_bound.store(false, Ordering::Relaxed);
                tracing::debug!(
                    %error,
                    addr = %inner.config.addr,
                    "SN-Utils bridge port unavailable; retrying"
                );
                sleep(inner.config.rebind_delay).await;
                continue;
            }
        };
        inner.ws_bound.store(true, Ordering::Relaxed);
        inner.bound_addr_tx.send_replace(listener.local_addr().ok());
        tracing::info!(addr = %inner.config.addr, "SN-Utils bridge listening");

        loop {
            let (stream, peer_addr) = match listener.accept().await {
                Ok(accepted) => accepted,
                Err(error) => {
                    tracing::debug!(%error, "SN-Utils bridge accept failed");
                    sleep(Duration::from_millis(200)).await;
                    continue;
                }
            };
            let socket = match accept_async(stream).await {
                Ok(socket) => socket,
                Err(error) => {
                    tracing::debug!(%error, %peer_addr, "SN-Utils bridge handshake failed");
                    continue;
                }
            };
            attach_connection(&inner, socket, peer_addr).await;
        }
    }
}

/// Install a freshly accepted helper-tab connection, replacing (and tearing
/// down) any previous one. The newest connection always wins: a tab reload
/// reconnects within ~1s and immediately becomes the active bridge.
async fn attach_connection(
    inner: &Arc<Inner>,
    socket: WebSocketStream<TcpStream>,
    peer_addr: SocketAddr,
) {
    let generation = inner.generation.fetch_add(1, Ordering::Relaxed) + 1;
    let (sink, stream) = socket.split();
    let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<Message>();
    let last_seen = Arc::new(std::sync::Mutex::new(Instant::now()));

    let writer = tokio::spawn(writer_loop(sink, outbound_rx));
    let reader = tokio::spawn(reader_loop(
        Arc::clone(inner),
        stream,
        outbound_tx.clone(),
        generation,
        Arc::clone(&last_seen),
    ));

    let previous = {
        let mut conn = inner.conn.lock().await;
        conn.replace(Conn {
            generation,
            outbound: outbound_tx.clone(),
            reader,
            writer,
            last_seen,
        })
    };
    if let Some(previous) = previous {
        previous.reader.abort();
        previous.writer.abort();
        fail_waiters_for_generation(inner, previous.generation).await;
        tracing::debug!(
            replaced = previous.generation,
            by = generation,
            "SN-Utils helper tab connection replaced"
        );
    }

    *inner.helper_status.lock().await = HelperStatus {
        generation,
        ..HelperStatus::default()
    };
    let _ = outbound_tx.send(host_hello_message());
    if let Some(banner) = inner.pending_banner.lock().await.take() {
        let _ = outbound_tx.send(banner_message(&banner));
    }
    // send_replace: a plain watch `send` drops the value when no receiver is
    // currently subscribed, which would leave `connected` stuck at false.
    inner.connected_tx.send_replace(true);
    tracing::debug!(%peer_addr, generation, "SN-Utils helper tab connected");
}

/// Tear down the given connection generation (no-op if it was already
/// replaced) and fail its pending request waiters with `Disconnected`.
async fn detach_connection(inner: &Arc<Inner>, generation: u64) {
    {
        let mut conn = inner.conn.lock().await;
        match conn.as_ref() {
            Some(current) if current.generation == generation => {
                // Safe: we just checked as_ref() above; the Option is guaranteed to be Some
                #[allow(clippy::unwrap_used)]
                let current = conn.take().unwrap();
                current.reader.abort();
                current.writer.abort();
            }
            _ => return,
        }
    }
    inner.connected_tx.send_replace(false);
    fail_waiters_for_generation(inner, generation).await;
    tracing::debug!(generation, "SN-Utils helper tab disconnected");
}

async fn fail_waiters_for_generation(inner: &Inner, generation: u64) {
    let mut waiters = inner.waiters.lock().await;
    let mut remaining = Vec::with_capacity(waiters.len());
    for waiter in waiters.drain(..) {
        if waiter.generation == Some(generation) {
            let _ = waiter.tx.send(Err(BridgeError::Disconnected));
        } else {
            remaining.push(waiter);
        }
    }
    *waiters = remaining;
}

async fn writer_loop(
    mut sink: SplitSink<WebSocketStream<TcpStream>, Message>,
    mut outbound_rx: mpsc::UnboundedReceiver<Message>,
) {
    while let Some(message) = outbound_rx.recv().await {
        if sink.send(message).await.is_err() {
            // Socket is dead; the reader notices and detaches the connection.
            break;
        }
    }
}

async fn reader_loop(
    inner: Arc<Inner>,
    mut stream: SplitStream<WebSocketStream<TcpStream>>,
    outbound: mpsc::UnboundedSender<Message>,
    generation: u64,
    last_seen: Arc<std::sync::Mutex<Instant>>,
) {
    loop {
        let Some(item) = stream.next().await else {
            break;
        };
        let Ok(message) = item else {
            break;
        };
        if let Ok(mut seen) = last_seen.lock() {
            *seen = Instant::now();
        }
        match message {
            Message::Text(text) => match serde_json::from_str::<Value>(&text) {
                Ok(value) if value.is_array() => {
                    tracing::debug!(%text, "ignoring SN-Utils informational array message");
                }
                Ok(value) => match SnuMessage::from_value(value) {
                    Ok(msg) => deliver_message(&inner, msg, generation).await,
                    Err(error) => {
                        tracing::warn!(%error, %text, "unparseable SN-Utils message; ignoring");
                    }
                },
                Err(error) => {
                    tracing::warn!(%error, %text, "invalid JSON from SN-Utils helper tab; ignoring");
                }
            },
            Message::Binary(bytes) => match serde_json::from_slice::<Value>(&bytes)
                .map_err(anyhow::Error::from)
                .and_then(SnuMessage::from_value)
            {
                Ok(msg) => deliver_message(&inner, msg, generation).await,
                Err(error) => {
                    tracing::warn!(%error, "invalid binary JSON from SN-Utils helper tab; ignoring");
                }
            },
            Message::Ping(bytes) => {
                let _ = outbound.send(Message::Pong(bytes));
            }
            Message::Pong(_) | Message::Frame(_) => {}
            Message::Close(frame) => {
                tracing::debug!(?frame, "SN-Utils helper tab closed connection");
                break;
            }
        }
    }
    detach_connection(&inner, generation).await;
}

/// Route a parsed message: capture any embedded session unconditionally, then
/// fulfill every waiter whose matcher accepts it.
async fn deliver_message(inner: &Inner, msg: SnuMessage, generation: u64) {
    capture_helper_metadata(inner, &msg, generation).await;
    if let Some(instance) = &msg.instance
        && instance
            .g_ck
            .as_deref()
            .is_some_and(|token| !token.is_empty())
    {
        let _ = inner.sessions_tx.send(instance.clone());
    }

    let mut waiters = inner.waiters.lock().await;
    let mut remaining = Vec::with_capacity(waiters.len());
    for waiter in waiters.drain(..) {
        if waiter.matcher.matches(&msg) {
            let _ = waiter.tx.send(Ok(msg.clone()));
        } else {
            remaining.push(waiter);
        }
    }
    *waiters = remaining;
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GateUpdate {
    instance_origin: String,
    revision: u64,
    gates: crate::snu::gates::InstanceGates,
}

async fn capture_helper_metadata(inner: &Inner, msg: &SnuMessage, generation: u64) {
    let Some(action) = msg.action.as_deref() else {
        return;
    };
    let value = Value::Object(msg.extra.clone());
    let mut status = inner.helper_status.lock().await;
    if status.generation != generation {
        return;
    }
    match action {
        "helperBuildInfo" => {
            if let Ok(build) = serde_json::from_value::<HelperBuildInfo>(value) {
                if build.capabilities.instance_security_gates.unwrap_or(0) > 0 {
                    status.security_gate_support = HelperSecurityGateSupport::Gated;
                }
                status.build = Some(build);
            }
        }
        "helperLicenseInfo" => {
            if let Ok(license) = serde_json::from_value::<HelperLicenseInfo>(value) {
                status.license = Some(license);
            }
        }
        "helperGatesUpdated" => {
            if let Ok(update) = serde_json::from_value::<GateUpdate>(value)
                && let Some(origin) = normalize_origin(&update.instance_origin)
            {
                status.apply_gate_snapshot(
                    origin,
                    GateSnapshot {
                        revision: update.revision,
                        gates: update.gates,
                    },
                );
            }
        }
        _ => {}
    }
}

/// Periodically ping the helper tab and reap connections whose inbound side
/// has gone silent (half-open sockets a tab reload can leave behind). Browsers
/// answer WebSocket pings automatically at the protocol level, so a healthy
/// tab keeps `last_seen` fresh without any SN-Utils-side changes. Also sweeps
/// abandoned waiters (callers that timed out and dropped their receiver).
async fn heartbeat_loop(inner: Arc<Inner>) {
    let interval = inner.config.heartbeat_interval;
    loop {
        sleep(interval).await;

        inner
            .waiters
            .lock()
            .await
            .retain(|waiter| !waiter.tx.is_closed());

        let current = {
            let conn = inner.conn.lock().await;
            conn.as_ref().map(|conn| {
                (
                    conn.generation,
                    conn.outbound.clone(),
                    Arc::clone(&conn.last_seen),
                )
            })
        };
        let Some((generation, outbound, last_seen)) = current else {
            continue;
        };

        let idle = last_seen
            .lock()
            .map(|seen| seen.elapsed())
            .unwrap_or(Duration::ZERO);
        if idle > interval * 2 {
            tracing::debug!(
                generation,
                idle_ms = idle.as_millis() as u64,
                "SN-Utils helper tab unresponsive; dropping connection"
            );
            detach_connection(&inner, generation).await;
            continue;
        }
        let _ = outbound.send(Message::Ping(Bytes::new()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio_tungstenite::MaybeTlsStream;

    type HelperSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

    fn test_config(addr: SocketAddr, heartbeat: Duration) -> BridgeConfig {
        BridgeConfig {
            addr,
            heartbeat_interval: heartbeat,
            connect_timeout: Duration::from_secs(5),
            rebind_delay: Duration::from_millis(50),
        }
    }

    async fn start_manager(
        heartbeat: Duration,
    ) -> (
        BridgeManager,
        mpsc::UnboundedReceiver<SnuInstance>,
        SocketAddr,
    ) {
        let (manager, sessions) =
            BridgeManager::start(test_config(([127, 0, 0, 1], 0).into(), heartbeat));
        let addr = manager
            .wait_bound(Duration::from_secs(2))
            .await
            .expect("listener bound");
        (manager, sessions, addr)
    }

    async fn connect_helper_raw(addr: SocketAddr) -> HelperSocket {
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
            .await
            .expect("helper connects");
        socket
    }

    async fn connect_helper(addr: SocketAddr) -> HelperSocket {
        let mut socket = connect_helper_raw(addr).await;
        let hello = socket.next().await.unwrap().unwrap();
        let hello: Value = serde_json::from_str(hello.to_text().unwrap()).unwrap();
        assert_eq!(hello["action"], "hostHello");
        socket
    }

    async fn send_json(helper: &mut HelperSocket, value: Value) {
        helper
            .send(Message::Text(value.to_string().into()))
            .await
            .expect("helper send");
    }

    /// Spawn a request and consume its outbound payload from the helper side,
    /// so the helper socket is ready to send replies.
    async fn spawn_request(
        manager: &BridgeManager,
        helper: &mut HelperSocket,
        correlation_id: &str,
    ) -> tokio::task::JoinHandle<Result<SnuMessage, BridgeError>> {
        let requester = manager.clone();
        let payload = json!({ "action": "test", "agentRequestId": correlation_id });
        let matcher = Matcher::Correlation(correlation_id.to_string());
        let handle = tokio::spawn(async move { requester.request(&payload, matcher, 5).await });
        let outbound = helper
            .next()
            .await
            .expect("payload frame")
            .expect("frame ok");
        assert!(
            outbound
                .to_text()
                .expect("text frame")
                .contains(correlation_id)
        );
        handle
    }

    #[tokio::test]
    async fn connection_announces_host_and_scopes_helper_state_to_generation() {
        let (manager, _sessions, addr) = start_manager(Duration::from_secs(60)).await;
        let mut helper = connect_helper_raw(addr).await;

        let hello = helper.next().await.unwrap().unwrap();
        let hello: Value = serde_json::from_str(hello.to_text().unwrap()).unwrap();
        assert_eq!(hello["action"], "hostHello");
        assert_eq!(hello["hostKind"], "standalone");

        send_json(
            &mut helper,
            json!({
                "action": "helperBuildInfo",
                "extensionName": "SN Utils",
                "extensionVersion": "10.1.9.0",
                "debuggerAvailable": true,
                "capabilities": {"protocolVersion": 1, "instanceSecurityGates": 1}
            }),
        )
        .await;
        send_json(
            &mut helper,
            json!({
                "action": "helperGatesUpdated",
                "instanceOrigin": "https://DEV.service-now.com",
                "revision": 7,
                "gates": {
                    "backgroundScripts": "off",
                    "deleteRecords": "approve",
                    "createArtifacts": "auto",
                    "restRequest": "auto",
                    "browserDebugger": "auto"
                }
            }),
        )
        .await;

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let status = manager.helper_status().await;
            if status.build.is_some() && !status.instance_gates.is_empty() {
                assert_eq!(status.generation, 1);
                assert_eq!(
                    status.instance_gates["https://dev.service-now.com:443"].revision,
                    7
                );
                break;
            }
            assert!(Instant::now() < deadline, "helper state was not captured");
            sleep(Duration::from_millis(10)).await;
        }

        let _replacement = connect_helper(addr).await;
        let status = manager.helper_status().await;
        assert_eq!(status.generation, 2);
        assert!(status.build.is_none());
        assert!(status.instance_gates.is_empty());
    }

    #[tokio::test]
    async fn refreshed_gate_omission_does_not_reuse_cached_authorization() {
        let (manager, _sessions, addr) = start_manager(Duration::from_secs(60)).await;
        let mut helper = connect_helper(addr).await;
        send_json(
            &mut helper,
            json!({
                "action": "helperGatesUpdated",
                "instanceOrigin": "https://dev.service-now.com",
                "revision": 1,
                "gates": {
                    "backgroundScripts": "auto",
                    "deleteRecords": "auto",
                    "createArtifacts": "auto",
                    "restRequest": "auto",
                    "browserDebugger": "auto"
                }
            }),
        )
        .await;
        let requester = manager.clone();
        let preflight = tokio::spawn(async move {
            requester
                .preflight(
                    &json!({
                        "action": "agentRunBackgroundScript",
                        "instance": {"url": "https://dev.service-now.com"}
                    }),
                    2,
                )
                .await
        });
        let request = helper.next().await.unwrap().unwrap();
        let request: Value = serde_json::from_str(request.to_text().unwrap()).unwrap();
        send_json(
            &mut helper,
            json!({
                "action": "agentGetCapabilitiesResponse",
                "agentRequestId": request["agentRequestId"],
                "success": true,
                "capabilities": {"protocolVersion": 1, "instanceSecurityGates": 1},
                "instanceGates": {}
            }),
        )
        .await;
        let error = preflight.await.unwrap().unwrap_err();
        assert!(matches!(error, BridgeError::GateUnauthorized(_)));
    }

    #[tokio::test]
    async fn gated_metadata_arriving_during_refresh_prevents_legacy_downgrade() {
        let (manager, _sessions, addr) = start_manager(Duration::from_secs(60)).await;
        let mut helper = connect_helper(addr).await;
        let requester = manager.clone();
        let negotiation = tokio::spawn(async move { requester.negotiate_capabilities(2).await });
        let request = helper.next().await.unwrap().unwrap();
        let request: Value = serde_json::from_str(request.to_text().unwrap()).unwrap();
        send_json(
            &mut helper,
            json!({
                "action": "helperBuildInfo",
                "extensionName": "SN Utils",
                "extensionVersion": "10.1.9.0",
                "debuggerAvailable": true,
                "capabilities": {"protocolVersion": 1, "instanceSecurityGates": 1}
            }),
        )
        .await;
        send_json(
            &mut helper,
            json!({
                "action": "agentGetCapabilitiesResponse",
                "agentRequestId": request["agentRequestId"],
                "success": true,
                "capabilities": {"protocolVersion": 1}
            }),
        )
        .await;
        let error = negotiation.await.unwrap().unwrap_err();
        assert!(matches!(error, BridgeError::GateStateUnavailable(_)));
    }

    #[tokio::test]
    async fn advertised_gated_helper_cannot_downgrade_to_legacy() {
        let (manager, _sessions, addr) = start_manager(Duration::from_secs(60)).await;
        let mut helper = connect_helper(addr).await;
        send_json(
            &mut helper,
            json!({
                "action": "helperBuildInfo",
                "extensionName": "SN Utils",
                "extensionVersion": "10.1.9.0",
                "debuggerAvailable": true,
                "capabilities": {"protocolVersion": 1, "instanceSecurityGates": 1}
            }),
        )
        .await;
        let deadline = Instant::now() + Duration::from_secs(1);
        while !manager.helper_status().await.advertises_instance_gates() {
            assert!(Instant::now() < deadline);
            sleep(Duration::from_millis(10)).await;
        }

        let requester = manager.clone();
        let negotiation = tokio::spawn(async move { requester.negotiate_capabilities(2).await });
        let request = helper.next().await.unwrap().unwrap();
        let request: Value = serde_json::from_str(request.to_text().unwrap()).unwrap();
        send_json(
            &mut helper,
            json!({
                "action": "agentGetCapabilitiesResponse",
                "agentRequestId": request["agentRequestId"],
                "success": true,
                "capabilities": {"protocolVersion": 1}
            }),
        )
        .await;
        let error = negotiation.await.unwrap().unwrap_err();
        assert!(matches!(error, BridgeError::GateStateUnavailable(_)));
    }

    #[tokio::test]
    async fn generation_bound_request_is_not_sent_after_reconnect() {
        let (manager, _sessions, addr) = start_manager(Duration::from_secs(60)).await;
        let _first_helper = connect_helper(addr).await;
        let mut replacement = connect_helper(addr).await;
        let payload = json!({"action": "mutation", "agentRequestId": "bound-1"});
        let error = manager
            .request_on_generation(
                &payload,
                Matcher::Correlation("bound-1".to_string()),
                1,
                Some(1),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, BridgeError::Disconnected));
        assert!(
            timeout(Duration::from_millis(100), replacement.next())
                .await
                .is_err(),
            "stale-generation payload reached replacement helper"
        );
    }

    #[tokio::test]
    async fn capability_negotiation_fails_when_helper_generation_changes() {
        let (manager, _sessions, addr) = start_manager(Duration::from_secs(60)).await;
        let mut first_helper = connect_helper(addr).await;
        let requester = manager.clone();
        let negotiation = tokio::spawn(async move { requester.negotiate_capabilities(2).await });
        let capability_request = first_helper.next().await.unwrap().unwrap();
        assert!(
            capability_request
                .to_text()
                .unwrap()
                .contains("agentGetCapabilities")
        );

        let _replacement = connect_helper(addr).await;
        let error = negotiation.await.unwrap().unwrap_err();
        assert!(matches!(error, BridgeError::Disconnected));
    }

    #[tokio::test]
    async fn correlation_request_ignores_unrelated_messages() {
        let (manager, _sessions, addr) = start_manager(Duration::from_secs(60)).await;
        let mut helper = connect_helper(addr).await;
        manager
            .wait_connected(Duration::from_secs(2))
            .await
            .unwrap();

        let request = spawn_request(&manager, &mut helper, "abc").await;
        send_json(
            &mut helper,
            json!({ "agentRequestId": "other", "success": true }),
        )
        .await;
        send_json(
            &mut helper,
            json!({ "agentRequestId": "abc", "success": true, "data": "hi" }),
        )
        .await;

        let message = request.await.unwrap().unwrap();
        assert_eq!(
            message.extra.get("data").and_then(Value::as_str),
            Some("hi")
        );
    }

    #[tokio::test]
    async fn helper_reconnect_heals_the_bridge() {
        let (manager, _sessions, addr) = start_manager(Duration::from_secs(60)).await;
        let mut helper_a = connect_helper(addr).await;
        manager
            .wait_connected(Duration::from_secs(2))
            .await
            .unwrap();

        // A request in flight when the tab "reloads" fails with a typed
        // disconnect, not a full response timeout.
        let in_flight = spawn_request(&manager, &mut helper_a, "r1").await;
        helper_a.close(None).await.unwrap();
        let error = in_flight.await.unwrap().unwrap_err();
        assert!(matches!(error, BridgeError::Disconnected), "got: {error}");

        // A fresh tab connection is picked up without any broker restart.
        let mut helper_b = connect_helper(addr).await;
        manager
            .wait_connected(Duration::from_secs(2))
            .await
            .unwrap();
        let request = spawn_request(&manager, &mut helper_b, "r2").await;
        send_json(
            &mut helper_b,
            json!({ "agentRequestId": "r2", "success": true }),
        )
        .await;
        assert!(request.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn token_pushed_mid_request_is_captured() {
        let (manager, mut sessions, addr) = start_manager(Duration::from_secs(60)).await;
        let mut helper = connect_helper(addr).await;
        manager
            .wait_connected(Duration::from_secs(2))
            .await
            .unwrap();

        let request = spawn_request(&manager, &mut helper, "q").await;

        // A /token arriving while the request is pending is not lost...
        send_json(
            &mut helper,
            json!({
                "instance": { "name": "dev", "url": "https://dev.service-now.com", "g_ck": "tok" }
            }),
        )
        .await;
        let captured = timeout(Duration::from_secs(2), sessions.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(captured.url, "https://dev.service-now.com");

        // ...and the request still resolves from its own reply.
        send_json(
            &mut helper,
            json!({ "agentRequestId": "q", "success": true }),
        )
        .await;
        assert!(request.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn banner_never_waits_and_is_delivered_on_next_connection() {
        let (manager, _sessions, addr) = start_manager(Duration::from_secs(60)).await;

        // No helper connected: must return immediately, not wait for an accept.
        timeout(Duration::from_millis(200), manager.send_banner("hello"))
            .await
            .expect("send_banner must not block");

        let mut helper = connect_helper(addr).await;
        let first = helper.next().await.unwrap().unwrap();
        let text = first.to_text().unwrap();
        assert!(text.contains("bannerMessage") && text.contains("hello"));
    }

    #[tokio::test]
    async fn heartbeat_reaps_unresponsive_connection() {
        let (manager, _sessions, addr) = start_manager(Duration::from_millis(100)).await;
        // Handshake, then never poll the socket again: pings go unanswered,
        // mimicking the half-open socket a crashed tab leaves behind.
        let helper = connect_helper(addr).await;
        manager
            .wait_connected(Duration::from_secs(2))
            .await
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(3);
        while manager.is_connected() {
            assert!(
                Instant::now() < deadline,
                "heartbeat did not reap the half-open connection"
            );
            sleep(Duration::from_millis(25)).await;
        }
        drop(helper);
    }

    #[tokio::test]
    async fn port_conflict_fails_fast_and_rebinds_when_freed() {
        let squatter = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = squatter.local_addr().unwrap();

        let (manager, _sessions) = BridgeManager::start(test_config(addr, Duration::from_secs(60)));
        // Give the accept loop a moment to fail its first bind attempt.
        sleep(Duration::from_millis(150)).await;

        let error = manager
            .request(
                &json!({ "agentRequestId": "x" }),
                Matcher::Correlation("x".into()),
                1,
            )
            .await
            .unwrap_err();
        assert!(
            matches!(error, BridgeError::PortConflict(_)),
            "got: {error}"
        );
        let error = manager.wait_for_session(1, None).await.unwrap_err();
        assert!(
            matches!(error, BridgeError::PortConflict(_)),
            "got: {error}"
        );

        // Free the port: the retry-bind picks it up without a restart.
        drop(squatter);
        let bound = manager
            .wait_bound(Duration::from_secs(2))
            .await
            .expect("rebinds after the other owner exits");
        assert_eq!(bound.port(), addr.port());

        let mut helper = connect_helper(addr).await;
        manager
            .wait_connected(Duration::from_secs(2))
            .await
            .unwrap();
        let request = spawn_request(&manager, &mut helper, "y").await;
        send_json(
            &mut helper,
            json!({ "agentRequestId": "y", "success": true }),
        )
        .await;
        assert!(request.await.unwrap().is_ok());
    }

    #[test]
    fn socket_address_requires_loopback_ip() {
        assert_eq!(
            loopback_socket_addr("127.0.0.1:1978", "test address").unwrap(),
            SocketAddr::from(([127, 0, 0, 1], 1978))
        );
        assert!(loopback_socket_addr("[::1]:1978", "test address").is_ok());

        let error = loopback_socket_addr("0.0.0.0:1978", "test address").unwrap_err();
        assert!(error.to_string().contains("loopback IP address"));
        let error = loopback_socket_addr("localhost:1978", "test address").unwrap_err();
        assert!(error.to_string().contains("IP socket address"));
    }
}
