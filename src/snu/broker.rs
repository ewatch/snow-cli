use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout};

use crate::models::secret::Secret;
use crate::snu::bridge::{
    BridgeConfig, BridgeError, BridgeManager, Matcher, env_loopback_socket_addr,
};
use crate::snu::gates::{GateDecision, HelperStatus, PreflightResult};
use crate::snu::protocol::{SnuInstance, SnuMessage, normalize_origin};

pub const DEFAULT_SNU_BROKER_ADDR: SocketAddr =
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1979);

const BROKER_IPC_TOKEN_FILE_NAME: &str = "snu-broker-token";
const BROKER_IPC_TOKEN_BYTES: usize = 32;
#[cfg(unix)]
const BROKER_UNIX_DIR_NAME: &str = "run";
#[cfg(unix)]
const BROKER_UNIX_SOCKET_NAME: &str = "snu.sock";
/// Conservative cap below macOS's 104-byte `sockaddr_un.sun_path` limit.
#[cfg(unix)]
const MAX_UNIX_SOCKET_PATH_BYTES: usize = 100;

#[derive(Debug, Clone)]
enum BrokerEndpoint {
    #[cfg(unix)]
    Unix(PathBuf),
    Tcp(SocketAddr),
}

impl BrokerEndpoint {
    fn display_addr(&self) -> String {
        match self {
            #[cfg(unix)]
            Self::Unix(path) => format!("unix://{}", path.display()),
            Self::Tcp(addr) => addr.to_string(),
        }
    }
}

/// Resolves the broker's CLI-facing IPC endpoint.
///
/// On Unix, defaults to a `0o600` Unix domain socket inside a `0o700`
/// directory (peer UID checked via `peer_cred()`). Setting
/// `SNOW_CLI_SNU_BROKER_ADDR` selects the loopback TCP fallback (which is also
/// the default on Windows); TCP IPC always authenticates requests with a
/// per-user random secret token.
fn broker_endpoint() -> anyhow::Result<BrokerEndpoint> {
    if let Ok(value) = std::env::var("SNOW_CLI_SNU_BROKER_ADDR")
        && !value.trim().is_empty()
    {
        return Ok(BrokerEndpoint::Tcp(env_loopback_socket_addr(
            "SNOW_CLI_SNU_BROKER_ADDR",
            DEFAULT_SNU_BROKER_ADDR,
        )?));
    }

    #[cfg(unix)]
    {
        if let Some(socket) = std::env::var_os("SNOW_CLI_SNU_BROKER_SOCKET")
            && !socket.is_empty()
        {
            return Ok(BrokerEndpoint::Unix(PathBuf::from(socket)));
        }
        Ok(BrokerEndpoint::Unix(default_unix_socket_path()))
    }

    #[cfg(not(unix))]
    {
        Ok(BrokerEndpoint::Tcp(env_loopback_socket_addr(
            "SNOW_CLI_SNU_BROKER_ADDR",
            DEFAULT_SNU_BROKER_ADDR,
        )?))
    }
}

fn broker_config_dir() -> PathBuf {
    let config_path = crate::config::profile::AppConfig::config_path();
    config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".servicenow"))
}

#[cfg(unix)]
fn default_unix_socket_path() -> PathBuf {
    let config_dir = broker_config_dir();
    let preferred = config_dir
        .join(BROKER_UNIX_DIR_NAME)
        .join(BROKER_UNIX_SOCKET_NAME);
    if preferred.as_os_str().len() <= MAX_UNIX_SOCKET_PATH_BYTES {
        return preferred;
    }

    // Fall back to a short, per-UID, config-dir-scoped directory when the
    // config directory path exceeds `sockaddr_un.sun_path` limits.
    let mut hasher = Sha256::new();
    hasher.update(config_dir.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    let short_hash: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    let dir_name = format!("snow-cli-{}-{short_hash}", current_euid());
    let temp_candidate = std::env::temp_dir()
        .join(&dir_name)
        .join(BROKER_UNIX_SOCKET_NAME);
    if temp_candidate.as_os_str().len() <= MAX_UNIX_SOCKET_PATH_BYTES {
        return temp_candidate;
    }
    PathBuf::from("/tmp")
        .join(dir_name)
        .join(BROKER_UNIX_SOCKET_NAME)
}

#[cfg(unix)]
#[allow(
    unsafe_code,
    reason = "geteuid is a standard POSIX query with no preconditions"
)]
fn current_euid() -> u32 {
    // SAFETY: `geteuid()` is always safe to call and has no preconditions.
    unsafe { libc::geteuid() }
}

#[cfg(unix)]
fn verify_peer_uid_matches(peer_uid: u32, expected_uid: u32) -> anyhow::Result<()> {
    if peer_uid != expected_uid {
        return Err(anyhow!(
            "unauthorized SN-Utils broker IPC peer (uid {peer_uid} does not match broker uid {expected_uid})"
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn verify_unix_peer_uid(stream: &UnixStream) -> anyhow::Result<()> {
    let cred = stream
        .peer_cred()
        .context("failed to read SN-Utils broker Unix socket peer credentials")?;
    verify_peer_uid_matches(cred.uid(), current_euid())
}

#[cfg(unix)]
fn ensure_private_socket_dir(dir: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    if let Ok(meta) = std::fs::symlink_metadata(dir) {
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(anyhow!(
                "SN-Utils broker socket directory {} is not a real directory",
                dir.display()
            ));
        }
        if meta.uid() != current_euid() {
            return Err(anyhow!(
                "SN-Utils broker socket directory {} is not owned by the current user",
                dir.display()
            ));
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("failed to set 0o700 permissions on {}", dir.display()))?;
        return Ok(());
    }

    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to create socket directory {}", dir.display()));
        }
    }
    let meta = std::fs::symlink_metadata(dir)
        .with_context(|| format!("failed to stat socket directory {}", dir.display()))?;
    if meta.file_type().is_symlink() || !meta.is_dir() || meta.uid() != current_euid() {
        return Err(anyhow!(
            "SN-Utils broker socket directory {} failed ownership/type check",
            dir.display()
        ));
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("failed to set 0o700 permissions on {}", dir.display()))?;
    Ok(())
}

#[cfg(unix)]
fn bind_unix_ipc_socket(path: &Path) -> anyhow::Result<UnixListener> {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};

    let parent = path.parent().ok_or_else(|| {
        anyhow!(
            "SN-Utils broker Unix socket path {} has no parent directory",
            path.display()
        )
    })?;
    ensure_private_socket_dir(parent)?;

    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() || !meta.file_type().is_socket() {
            return Err(anyhow!(
                "refusing to replace non-socket path {} for SN-Utils broker IPC",
                path.display()
            ));
        }
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(anyhow!(
                "SN-Utils broker IPC socket {} is already in use",
                path.display()
            ));
        }
        std::fs::remove_file(path)
            .with_context(|| format!("failed to remove stale socket {}", path.display()))?;
    }

    let listener = UnixListener::bind(path)
        .with_context(|| format!("failed to bind SN-Utils broker IPC on {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to set 0o600 permissions on {}", path.display()))?;
    Ok(listener)
}

fn ipc_token_file_path() -> PathBuf {
    if let Some(path) = std::env::var_os("SNOW_CLI_SNU_BROKER_TOKEN_FILE")
        && !path.is_empty()
    {
        return PathBuf::from(path);
    }
    broker_config_dir().join(BROKER_IPC_TOKEN_FILE_NAME)
}

fn is_valid_ipc_token(token: &str) -> bool {
    token.len() == BROKER_IPC_TOKEN_BYTES * 2
        && token
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn read_ipc_token(file: &Path) -> std::io::Result<Secret> {
    let meta = std::fs::symlink_metadata(file)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "SN-Utils broker IPC token path is not a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))?;
    }
    let token = std::fs::read_to_string(file)?.trim().to_owned();
    if !is_valid_ipc_token(&token) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "SN-Utils broker IPC token file is invalid",
        ));
    }
    Ok(Secret::new(token))
}

fn load_or_create_ipc_token_at(file: &Path) -> anyhow::Result<Secret> {
    match read_ipc_token(file) {
        Ok(token) => return Ok(token),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read IPC token at {}", file.display()));
        }
    }

    let dir = file.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)
        .with_context(|| format!("failed to create directory {}", dir.display()))?;

    let mut token_bytes = [0u8; BROKER_IPC_TOKEN_BYTES];
    SystemRandom::new()
        .fill(&mut token_bytes)
        .map_err(|_| anyhow!("CSPRNG failure while generating SN-Utils broker IPC token"))?;
    let token: String = token_bytes.iter().map(|b| format!("{b:02x}")).collect();

    let mut suffix = [0u8; 8];
    SystemRandom::new().fill(&mut suffix).map_err(|_| {
        anyhow!("CSPRNG failure while creating SN-Utils broker IPC token temp file")
    })?;
    let temp_name = format!(
        ".{BROKER_IPC_TOKEN_FILE_NAME}-{}",
        suffix
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let temporary = dir.join(temp_name);

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let publish_result = (|| -> std::io::Result<()> {
        use std::io::Write as _;
        options
            .open(&temporary)?
            .write_all(format!("{token}\n").as_bytes())?;
        match std::fs::hard_link(&temporary, file) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(error),
        }
    })();
    let _ = std::fs::remove_file(&temporary);
    publish_result.with_context(|| format!("failed to persist IPC token at {}", file.display()))?;

    read_ipc_token(file)
        .with_context(|| format!("failed to reload IPC token at {}", file.display()))
}

fn load_or_create_ipc_token() -> anyhow::Result<Secret> {
    load_or_create_ipc_token_at(&ipc_token_file_path())
}

fn verify_ipc_token(provided: Option<&str>, expected: &Secret) -> bool {
    let Some(provided) = provided else {
        return false;
    };
    let lhs = Sha256::digest(provided.as_bytes());
    let rhs = Sha256::digest(expected.expose_secret().as_bytes());
    let mut diff = 0u8;
    for (a, b) in lhs.iter().zip(rhs.iter()) {
        diff |= a ^ b;
    }
    diff == 0 && provided.len() == expected.expose_secret().len()
}
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
struct BrokerEnvelope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    auth_token: Option<String>,
    #[serde(flatten)]
    request: BrokerRequest,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BrokerRequest {
    Ping,
    Status,
    RefreshCapabilities {
        timeout_secs: u64,
    },
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
    SendActionWithLegacyFallback {
        current_payload: Value,
        correlation_id: String,
        legacy_payload: Value,
        legacy_expected_action: String,
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
    VerifySession {
        timeout_secs: u64,
        #[serde(default)]
        origin: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct BrokerResponse {
    ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error_code: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    token_valid: Option<bool>,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct BrokerClientError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerStatus {
    pub version: String,
    pub ipc_addr: String,
    pub browser_connected: bool,
    pub helper: HelperStatus,
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
    /// Epoch seconds the current `g_ck` arrived over the helper socket. `None`
    /// for sessions restored from the disk cache (age unknown until the next
    /// `/token` or successful action).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<u64>,
    /// Epoch seconds of the last action ServiceNow accepted with this session.
    /// `None` means the token has not been proven valid in this broker's
    /// lifetime — a good moment for `check-connection --verify`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_verified_at: Option<u64>,
}

/// In-memory freshness bookkeeping for one origin's session. Deliberately not
/// persisted: after a broker restart the session is "cold" (age unknown,
/// unverified), which is exactly when a validity probe is worth running.
#[derive(Debug, Clone, Copy, Default)]
struct SessionMeta {
    captured_at: Option<u64>,
    last_verified_at: Option<u64>,
}

/// Shared broker handle. The helper-tab WebSocket is owned by the
/// [`BridgeManager`], which runs its own accept/read/heartbeat tasks for the
/// broker's whole lifetime, so dispatch never serializes on a bridge lock and
/// control requests (`status`/`stop`/`ping`) only ever take the short `state`
/// lock.
struct Broker {
    state: Mutex<BrokerState>,
    manager: BridgeManager,
    /// Legacy background-script responses have no usable correlation id, so
    /// only one may be in flight or concurrent callers could consume each
    /// other's action-matched response.
    legacy_action_lock: Mutex<()>,
}

impl Broker {
    fn new(idle_timeout: Duration, manager: BridgeManager, ipc_addr: String) -> Self {
        Self {
            state: Mutex::new(BrokerState::new(idle_timeout, ipc_addr)),
            manager,
            legacy_action_lock: Mutex::new(()),
        }
    }
}

struct BrokerState {
    sessions_by_origin: HashMap<String, SnuInstance>,
    meta_by_origin: HashMap<String, SessionMeta>,
    latest_origin: Option<String>,
    last_activity: Instant,
    active_clients: usize,
    in_flight: usize,
    shutdown: bool,
    idle_timeout: Duration,
    ipc_addr: String,
}

impl BrokerState {
    fn new(idle_timeout: Duration, ipc_addr: String) -> Self {
        Self {
            sessions_by_origin: HashMap::new(),
            meta_by_origin: HashMap::new(),
            latest_origin: None,
            last_activity: Instant::now(),
            active_clients: 0,
            in_flight: 0,
            shutdown: false,
            idle_timeout,
            ipc_addr,
        }
    }

    fn status(&self, browser_connected: bool, helper: HelperStatus) -> BrokerStatus {
        let mut instances: Vec<InstanceSummary> = self
            .sessions_by_origin
            .iter()
            .map(|(origin, instance)| {
                let meta = self.meta_by_origin.get(origin).copied().unwrap_or_default();
                InstanceSummary {
                    url: instance.url.clone(),
                    origin: origin.clone(),
                    has_g_ck: instance
                        .g_ck
                        .as_ref()
                        .is_some_and(|token| !token.expose_secret().is_empty()),
                    scope: instance.scope.clone(),
                    is_latest: self.latest_origin.as_deref() == Some(origin.as_str()),
                    captured_at: meta.captured_at,
                    last_verified_at: meta.last_verified_at,
                }
            })
            .collect();
        instances.sort_by(|a, b| a.origin.cmp(&b.origin));

        BrokerStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            ipc_addr: self.ipc_addr.to_string(),
            browser_connected,
            helper,
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
            // Only a *changed* token resets the freshness clock: WaitSession
            // re-remembers cached sessions on every command, and that must not
            // make an old token look newly captured (or wipe its verification).
            let token_changed = self
                .sessions_by_origin
                .get(&origin)
                .is_none_or(|existing| existing.g_ck != instance.g_ck);
            if token_changed {
                self.meta_by_origin.insert(
                    origin.clone(),
                    SessionMeta {
                        captured_at: Some(now_epoch_secs()),
                        last_verified_at: None,
                    },
                );
            }
            self.sessions_by_origin
                .insert(origin.clone(), instance.clone());
            self.latest_origin = Some(origin);
        }
    }

    /// Record that ServiceNow just accepted an action with this origin's
    /// session, so freshness reporting (and future probe decisions) can tell a
    /// proven-live token from one that has merely not failed yet.
    fn mark_verified(&mut self, origin: &str) {
        if self.sessions_by_origin.contains_key(origin) {
            self.meta_by_origin
                .entry(origin.to_string())
                .or_default()
                .last_verified_at = Some(now_epoch_secs());
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
        self.meta_by_origin.remove(origin);
        if self.latest_origin.as_deref() == Some(origin) {
            self.latest_origin = self.sessions_by_origin.keys().next().cloned();
        }
        removed
    }

    /// Drop every cached session, returning the origins that were cleared.
    fn clear_all(&mut self) -> Vec<String> {
        let cleared = self.sessions_by_origin.keys().cloned().collect();
        self.sessions_by_origin.clear();
        self.meta_by_origin.clear();
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
    endpoint: BrokerEndpoint,
    auth_token: Option<Secret>,
}

impl BrokerBridge {
    pub async fn connect_or_spawn() -> anyhow::Result<Self> {
        let endpoint = broker_endpoint()?;
        let auth_token = match &endpoint {
            BrokerEndpoint::Tcp(_) => Some(load_or_create_ipc_token()?),
            #[cfg(unix)]
            BrokerEndpoint::Unix(_) => None,
        };
        if connect_once(&endpoint, auth_token.as_ref()).await.is_err() {
            BridgeConfig::from_env()?;
            spawn_broker()?;
            wait_until_ready(&endpoint, auth_token.as_ref()).await?;
        }

        Ok(Self {
            endpoint,
            auth_token,
        })
    }

    pub async fn connect_existing() -> anyhow::Result<Self> {
        let endpoint = broker_endpoint()?;
        let auth_token = match &endpoint {
            BrokerEndpoint::Tcp(_) => Some(load_or_create_ipc_token()?),
            #[cfg(unix)]
            BrokerEndpoint::Unix(_) => None,
        };
        connect_once(&endpoint, auth_token.as_ref()).await?;
        Ok(Self {
            endpoint,
            auth_token,
        })
    }

    pub async fn refresh_helper_capabilities(
        &self,
        timeout_secs: u64,
    ) -> anyhow::Result<BrokerStatus> {
        self.request(BrokerRequest::RefreshCapabilities { timeout_secs })
            .await?
            .status
            .ok_or_else(|| anyhow!("SN-Utils broker did not return helper capability status"))
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

    /// Dispatch a correlated current-helper action, but select a proven legacy
    /// protocol action when capability negotiation identifies an older helper.
    /// Selection happens in the broker immediately before dispatch so a
    /// mutation can never be sent once through each protocol.
    pub async fn send_action_and_wait_with_legacy_fallback(
        &self,
        current_payload: &Value,
        correlation_id: &str,
        legacy_payload: &Value,
        legacy_expected_action: &str,
        timeout_secs: u64,
    ) -> anyhow::Result<SnuMessage> {
        self.request(BrokerRequest::SendActionWithLegacyFallback {
            current_payload: current_payload.clone(),
            correlation_id: correlation_id.to_string(),
            legacy_payload: legacy_payload.clone(),
            legacy_expected_action: legacy_expected_action.to_string(),
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

    /// Actively check whether the cached session's `g_ck` is still accepted by
    /// ServiceNow, via a cheap probe query through the helper tab. `Ok(true)`
    /// means the token is proven live; `Ok(false)` means ServiceNow rejected it
    /// and only then is a `/token` re-run warranted.
    pub async fn verify_session(
        &self,
        timeout_secs: u64,
        origin: Option<String>,
    ) -> anyhow::Result<bool> {
        self.request(BrokerRequest::VerifySession {
            timeout_secs,
            origin,
        })
        .await?
        .token_valid
        .ok_or_else(|| anyhow!("SN-Utils broker did not return a verification result"))
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
        let envelope = BrokerEnvelope {
            auth_token: self
                .auth_token
                .as_ref()
                .map(|secret| secret.expose_secret().to_string()),
            request,
        };
        let response = send_ipc_envelope(&self.endpoint, &envelope).await?;
        if response.ok {
            Ok(response)
        } else {
            Err(BrokerClientError {
                code: response
                    .error_code
                    .unwrap_or_else(|| "SNU_BROKER_ERROR".to_string()),
                message: response
                    .error
                    .unwrap_or_else(|| "SN-Utils broker request failed".to_string()),
            }
            .into())
        }
    }
}

async fn send_ipc_envelope(
    endpoint: &BrokerEndpoint,
    envelope: &BrokerEnvelope,
) -> anyhow::Result<BrokerResponse> {
    match endpoint {
        #[cfg(unix)]
        BrokerEndpoint::Unix(path) => {
            let stream = UnixStream::connect(path).await.with_context(|| {
                format!(
                    "failed to connect to SN-Utils broker at unix://{}",
                    path.display()
                )
            })?;
            send_ipc_envelope_on_stream(stream, envelope).await
        }
        BrokerEndpoint::Tcp(addr) => {
            let stream = TcpStream::connect(addr)
                .await
                .with_context(|| format!("failed to connect to SN-Utils broker at {addr}"))?;
            send_ipc_envelope_on_stream(stream, envelope).await
        }
    }
}

async fn send_ipc_envelope_on_stream<S>(
    stream: S,
    envelope: &BrokerEnvelope,
) -> anyhow::Result<BrokerResponse>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);

    let raw = serde_json::to_string(envelope)?;
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

    serde_json::from_str(&line).context("invalid JSON response from SN-Utils broker")
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
    broker_endpoint()?;
    match BrokerBridge::connect_existing().await {
        Ok(bridge) => bridge.clear_sessions(origin).await,
        Err(_) => Ok(Vec::new()),
    }
}

pub async fn run_broker_server() -> anyhow::Result<()> {
    let idle_timeout = idle_timeout();
    let endpoint = broker_endpoint()?;
    let bridge_config = BridgeConfig::from_env()?;

    enum BoundListener {
        #[cfg(unix)]
        Unix {
            listener: UnixListener,
            socket_path: PathBuf,
        },
        Tcp {
            listener: TcpListener,
            token: Arc<Secret>,
        },
    }

    let bound = match &endpoint {
        #[cfg(unix)]
        BrokerEndpoint::Unix(path) => BoundListener::Unix {
            listener: bind_unix_ipc_socket(path)?,
            socket_path: path.clone(),
        },
        BrokerEndpoint::Tcp(addr) => {
            let token = Arc::new(load_or_create_ipc_token()?);
            let listener = TcpListener::bind(addr)
                .await
                .with_context(|| format!("failed to bind SN-Utils broker IPC on {addr}"))?;
            BoundListener::Tcp { listener, token }
        }
    };

    let (manager, mut sessions_rx) = BridgeManager::start(bridge_config);
    let broker = Arc::new(Broker::new(idle_timeout, manager, endpoint.display_addr()));

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

    match bound {
        #[cfg(unix)]
        BoundListener::Unix {
            listener,
            socket_path,
        } => {
            loop {
                if broker.state.lock().await.shutdown {
                    break;
                }

                let accept = timeout(Duration::from_secs(1), listener.accept()).await;
                let Ok(Ok((stream, _peer_addr))) = accept else {
                    continue;
                };
                if let Err(error) = verify_unix_peer_uid(&stream) {
                    tracing::warn!(%error, "Rejected unauthorized SN-Utils broker IPC peer");
                    continue;
                }
                let client_broker = Arc::clone(&broker);
                tokio::spawn(async move {
                    if let Err(error) = handle_client(stream, client_broker, None).await {
                        tracing::debug!(%error, "SN-Utils broker client failed");
                    }
                });
            }
            let _ = std::fs::remove_file(&socket_path);
        }
        BoundListener::Tcp { listener, token } => loop {
            if broker.state.lock().await.shutdown {
                break;
            }

            let accept = timeout(Duration::from_secs(1), listener.accept()).await;
            let Ok(Ok((stream, _peer_addr))) = accept else {
                continue;
            };
            let client_broker = Arc::clone(&broker);
            let client_token = Arc::clone(&token);
            tokio::spawn(async move {
                if let Err(error) = handle_client(stream, client_broker, Some(client_token)).await {
                    tracing::debug!(%error, "SN-Utils broker client failed");
                }
            });
        },
    }

    Ok(())
}

async fn handle_client<S>(
    stream: S,
    broker: Arc<Broker>,
    expected_token: Option<Arc<Secret>>,
) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    {
        let mut state = broker.state.lock().await;
        state.active_clients += 1;
        state.touch();
    }

    let result = handle_client_inner(stream, Arc::clone(&broker), expected_token.as_deref()).await;

    {
        let mut state = broker.state.lock().await;
        state.active_clients = state.active_clients.saturating_sub(1);
        state.touch();
    }

    result
}

async fn handle_client_inner<S>(
    stream: S,
    broker: Arc<Broker>,
    expected_token: Option<&Secret>,
) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    if line.trim().is_empty() {
        return Ok(());
    }

    let envelope: BrokerEnvelope =
        serde_json::from_str(&line).context("invalid SN-Utils broker request")?;
    let response = if let Some(expected) = expected_token
        && !verify_ipc_token(envelope.auth_token.as_deref(), expected)
    {
        BrokerResponse {
            ok: false,
            error_code: Some("SNU_BROKER_UNAUTHORIZED".to_string()),
            error: Some(
                "SN-Utils broker IPC request rejected: invalid or missing authentication token"
                    .to_string(),
            ),
            message: None,
            instance: None,
            status: None,
            cleared: None,
            token_valid: None,
        }
    } else {
        match process_request(envelope.request, &broker).await {
            Ok(response) => response,
            Err(error) => BrokerResponse {
                ok: false,
                error_code: Some(
                    error
                        .downcast_ref::<BridgeError>()
                        .map_or("SNU_BROKER_ERROR", BridgeError::code)
                        .to_string(),
                ),
                error: Some(error.to_string()),
                message: None,
                instance: None,
                status: None,
                cleared: None,
                token_valid: None,
            },
        }
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
            let helper = broker.manager.helper_status().await;
            let state = broker.state.lock().await;
            Ok(BrokerResponse {
                ok: true,
                error_code: None,
                error: None,
                message: None,
                instance: None,
                status: Some(state.status(browser_connected, helper)),
                cleared: None,
                token_valid: None,
            })
        }
        BrokerRequest::RefreshCapabilities { timeout_secs } => {
            broker.manager.negotiate_capabilities(timeout_secs).await?;
            let browser_connected = broker.manager.is_connected();
            let helper = broker.manager.helper_status().await;
            let state = broker.state.lock().await;
            Ok(BrokerResponse {
                ok: true,
                error_code: None,
                error: None,
                message: None,
                instance: None,
                status: Some(state.status(browser_connected, helper)),
                cleared: None,
                token_valid: None,
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
                error_code: None,
                error: None,
                message: None,
                instance: Some(instance),
                status: None,
                cleared: None,
                token_valid: None,
            })
        }
        BrokerRequest::SendAction {
            mut payload,
            correlation_id,
            timeout_secs,
        } => {
            let preflight = preflight_for_dispatch(broker, &payload, timeout_secs).await?;
            let host_reviewed =
                match review_before_dispatch(broker, &payload, &preflight, timeout_secs).await? {
                    ReviewStep::Done(message) => return Ok(message_response(*message)),
                    ReviewStep::Proceed { host_reviewed } => host_reviewed,
                };
            let result = send_action_with_refresh(
                broker,
                &mut payload,
                &correlation_id,
                timeout_secs,
                preflight.generation,
            )
            .await;
            let message = explain_approval_timeout(result, &preflight, host_reviewed)?;
            Ok(message_response(message))
        }
        BrokerRequest::SendActionForAction {
            payload,
            expected_action,
            timeout_secs,
        } => {
            let preflight = preflight_for_dispatch(broker, &payload, timeout_secs).await?;
            let host_reviewed =
                match review_before_dispatch(broker, &payload, &preflight, timeout_secs).await? {
                    ReviewStep::Done(message) => return Ok(message_response(*message)),
                    ReviewStep::Proceed { host_reviewed } => host_reviewed,
                };
            let result = broker
                .manager
                .request_on_generation(
                    &payload,
                    Matcher::Action(expected_action),
                    timeout_secs,
                    preflight.generation,
                )
                .await
                .map_err(anyhow::Error::from);
            let message = explain_approval_timeout(result, &preflight, host_reviewed)?;
            Ok(message_response(message))
        }
        BrokerRequest::SendActionWithLegacyFallback {
            mut current_payload,
            correlation_id,
            mut legacy_payload,
            legacy_expected_action,
            timeout_secs,
        } => {
            let preflight = preflight_for_dispatch(broker, &current_payload, timeout_secs).await?;
            let host_reviewed =
                match review_before_dispatch(broker, &current_payload, &preflight, timeout_secs)
                    .await?
                {
                    ReviewStep::Done(message) => return Ok(message_response(*message)),
                    ReviewStep::Proceed { host_reviewed } => host_reviewed,
                };
            let result = if preflight.decision == GateDecision::LegacyUnrestricted {
                let _legacy_guard = broker.legacy_action_lock.lock().await;
                refresh_payload_instance_from_cache(broker, &mut legacy_payload).await?;
                send_legacy_action_with_refresh(
                    broker,
                    &mut legacy_payload,
                    &legacy_expected_action,
                    timeout_secs,
                    preflight.generation,
                )
                .await
            } else {
                send_action_with_refresh(
                    broker,
                    &mut current_payload,
                    &correlation_id,
                    timeout_secs,
                    preflight.generation,
                )
                .await
            };
            let message = explain_approval_timeout(result, &preflight, host_reviewed)?;
            Ok(message_response(message))
        }
        BrokerRequest::RefreshSession {
            timeout_secs,
            origin,
        } => {
            let instance = refresh_session(broker, timeout_secs, origin.as_deref()).await?;
            Ok(BrokerResponse {
                ok: true,
                error_code: None,
                error: None,
                message: None,
                instance: Some(instance),
                status: None,
                cleared: None,
                token_valid: None,
            })
        }
        BrokerRequest::VerifySession {
            timeout_secs,
            origin,
        } => {
            let session = {
                let state = broker.state.lock().await;
                match origin.as_deref() {
                    Some(origin) => state.session_for_origin(origin),
                    None => state.latest_session(),
                }
            };
            let Some(instance) = session else {
                return Err(anyhow!(
                    "no SN-Utils browser session to verify. Run /token in a ServiceNow tab first."
                ));
            };
            let valid = probe_session(broker, &instance, timeout_secs).await?;
            if valid && let Some(origin) = normalize_origin(&instance.url) {
                broker.state.lock().await.mark_verified(&origin);
            }
            Ok(BrokerResponse {
                ok: true,
                error_code: None,
                error: None,
                message: None,
                instance: None,
                status: None,
                cleared: None,
                token_valid: Some(valid),
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
                error_code: None,
                error: None,
                message: None,
                instance: None,
                status: None,
                cleared: Some(cleared),
                token_valid: None,
            })
        }
    }
}

async fn preflight_for_dispatch(
    broker: &Broker,
    payload: &Value,
    timeout_secs: u64,
) -> anyhow::Result<PreflightResult> {
    let preflight = broker.manager.preflight(payload, timeout_secs).await?;
    if preflight.decision == GateDecision::Blocked {
        return Err(BridgeError::GateBlocked {
            gate: preflight
                .gate
                .map(|gate| gate.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            origin: preflight
                .origin
                .clone()
                .unwrap_or_else(|| "unknown instance".to_string()),
        }
        .into());
    }
    Ok(preflight)
}

/// Result of the review step before a dispatch.
enum ReviewStep {
    /// Dispatch now. `host_reviewed` is true when this bridge ran a review
    /// and the user approved it.
    Proceed { host_reviewed: bool },
    /// Approved and already run by the helper; this is its reply.
    Done(Box<SnuMessage>),
}

/// Hold an approve-mode action for review in the helper tab.
async fn review_before_dispatch(
    broker: &Broker,
    payload: &Value,
    preflight: &PreflightResult,
    timeout_secs: u64,
) -> anyhow::Result<ReviewStep> {
    use crate::snu::review::{ReviewOutcome, review_if_required};
    Ok(
        match review_if_required(&broker.manager, payload, preflight, timeout_secs).await? {
            ReviewOutcome::NotRequired => ReviewStep::Proceed {
                host_reviewed: false,
            },
            ReviewOutcome::Dispatch => ReviewStep::Proceed {
                host_reviewed: true,
            },
            ReviewOutcome::Executed(message) => ReviewStep::Done(message),
        },
    )
}

/// Preflight a retry. A retry that would need a fresh host-run review is not
/// retried automatically: the original failure is returned so the caller
/// re-runs the command and the user approves it again.
async fn retry_preflight(
    broker: &Broker,
    payload: &Value,
    timeout_secs: u64,
    failure: &str,
) -> anyhow::Result<PreflightResult> {
    let preflight = preflight_for_dispatch(broker, payload, timeout_secs).await?;
    if crate::snu::review::needs_host_review(payload, &preflight) {
        return Err(BridgeError::ActionFailed(failure.to_owned()).into());
    }
    Ok(preflight)
}

fn explain_approval_timeout(
    result: anyhow::Result<SnuMessage>,
    preflight: &PreflightResult,
    host_reviewed: bool,
) -> anyhow::Result<SnuMessage> {
    let gate = || {
        preflight
            .gate
            .map(|gate| gate.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    };
    let origin = || {
        preflight
            .origin
            .clone()
            .unwrap_or_else(|| "unknown instance".to_string())
    };
    match result {
        // Approved here and sent: the action may have run, so it must not be
        // reported as an approval timeout that invites a retry.
        Err(error)
            if host_reviewed
                && error
                    .downcast_ref::<BridgeError>()
                    .is_some_and(|error| matches!(error, BridgeError::Timeout { .. })) =>
        {
            Err(BridgeError::ApprovedWithoutReply {
                gate: gate(),
                origin: origin(),
            }
            .into())
        }
        // The helper itself holds this action (background scripts) while the
        // user decides, so a timeout here is an approval timeout.
        Err(error)
            if preflight.decision == GateDecision::ApprovalRequired
                && error
                    .downcast_ref::<BridgeError>()
                    .is_some_and(|error| matches!(error, BridgeError::Timeout { .. })) =>
        {
            Err(BridgeError::ApprovalTimeout {
                gate: preflight
                    .gate
                    .map(|gate| gate.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                origin: preflight
                    .origin
                    .clone()
                    .unwrap_or_else(|| "unknown instance".to_string()),
            }
            .into())
        }
        result => result,
    }
}

/// Pause before retrying after `/token` or a transient helper error: the tab
/// re-authenticates asynchronously, and an immediate retry races it (observed
/// live as `Unexpected end of JSON input` from the helper's own fetch in the
/// same second the fresh token arrived).
const HELPER_SETTLE_DELAY: Duration = Duration::from_millis(1200);

/// Cap on how long a validity probe may take: it is a side-quest of the real
/// request and must not double a long action timeout.
const PROBE_TIMEOUT_SECS_MAX: u64 = 15;

/// Send a bridge action with resilience, without prompting for `/token` unless
/// a probe has proven the token dead:
/// - transient helper-tab errors (empty/unparseable fetch responses) get one
///   retry after a short pause — never an eviction or a `/token` prompt;
/// - auth-flavored failures are double-checked with [`probe_session`]: if the
///   session still works, the original error was a permissions/ACL denial and
///   is reported as such, keeping the valid session cached;
/// - only a probe-confirmed dead token evicts the session, prompts the user to
///   re-run `/token`, splices the fresh session into the payload, waits for the
///   tab to settle, and retries exactly once.
async fn send_action_with_refresh(
    broker: &Broker,
    payload: &mut Value,
    correlation_id: &str,
    timeout_secs: u64,
    expected_generation: Option<u64>,
) -> anyhow::Result<SnuMessage> {
    let first = broker
        .manager
        .request_on_generation(
            payload,
            Matcher::Correlation(correlation_id.to_string()),
            timeout_secs,
            expected_generation,
        )
        .await;

    let text = match first {
        Ok(message) => {
            mark_payload_verified(broker, payload).await;
            return Ok(message);
        }
        Err(BridgeError::ActionFailed(text)) => text,
        Err(error) => return Err(error.into()),
    };

    if is_transient_helper_text(&text) {
        sleep(HELPER_SETTLE_DELAY).await;
        let retry_preflight = retry_preflight(broker, payload, timeout_secs, &text).await?;
        let message = broker
            .manager
            .request_on_generation(
                payload,
                Matcher::Correlation(correlation_id.to_string()),
                timeout_secs,
                retry_preflight.generation,
            )
            .await?;
        mark_payload_verified(broker, payload).await;
        return Ok(message);
    }

    let instance = payload
        .get("instance")
        .and_then(|value| serde_json::from_value::<SnuInstance>(value.clone()).ok());
    let (Some(instance), true) = (instance, is_stale_token_text(&text)) else {
        return Err(BridgeError::ActionFailed(text).into());
    };

    // Auth-flavored failure: probe with the same session before prompting. A
    // working probe means the token is fine and the failure was an ACL denial
    // on the target table — re-running /token would not help.
    match probe_session(broker, &instance, timeout_secs).await {
        Ok(true) => {
            let detail = crate::client::error::safe_error_detail(&text);
            return Err(anyhow!(
                "ServiceNow denied this action, but the browser session is still valid — this is a permissions (ACL) problem on the target table, not an expired token, so re-running /token will not help: {detail}"
            ));
        }
        Ok(false) => {}
        Err(error) => {
            return Err(error.context(
                "could not verify the browser session after an authentication-like action failure; the cached session was preserved",
            ));
        }
    }

    // Refresh only the instance this action targeted, so a `/token` from a
    // different tab in the same SN-Utils portal can't silently redirect the
    // retry to the wrong instance.
    let target_origin = normalize_origin(&instance.url);
    let fresh = refresh_session(broker, timeout_secs, target_origin.as_deref()).await?;
    if let Some(object) = payload.as_object_mut() {
        object.insert("instance".to_string(), serde_json::to_value(&fresh)?);
    }
    sleep(HELPER_SETTLE_DELAY).await;
    let retry_preflight = retry_preflight(broker, payload, timeout_secs, &text).await?;
    let message = broker
        .manager
        .request_on_generation(
            payload,
            Matcher::Correlation(correlation_id.to_string()),
            timeout_secs,
            retry_preflight.generation,
        )
        .await?;
    mark_payload_verified(broker, payload).await;
    Ok(message)
}

/// Replace a queued action's session with the broker's newest session for the
/// same origin. This is required after waiting on the legacy serialization lock:
/// an earlier caller may have refreshed the token while this caller was queued.
async fn refresh_payload_instance_from_cache(
    broker: &Broker,
    payload: &mut Value,
) -> anyhow::Result<()> {
    let origin = payload
        .pointer("/instance/url")
        .and_then(Value::as_str)
        .and_then(normalize_origin);
    let Some(origin) = origin else {
        return Ok(());
    };
    let current = broker.state.lock().await.session_for_origin(&origin);
    if let Some(current) = current
        && let Some(object) = payload.as_object_mut()
    {
        object.insert("instance".to_string(), serde_json::to_value(current)?);
    }
    Ok(())
}

/// Legacy helpers identify background-script replies only by action name. The
/// caller serializes these requests with `legacy_action_lock`; this function
/// retains the same probe-before-refresh behavior as correlated current actions.
async fn send_legacy_action_with_refresh(
    broker: &Broker,
    payload: &mut Value,
    expected_action: &str,
    timeout_secs: u64,
    expected_generation: Option<u64>,
) -> anyhow::Result<SnuMessage> {
    let first = broker
        .manager
        .request_on_generation_with_support(
            payload,
            Matcher::Action(expected_action.to_string()),
            timeout_secs,
            expected_generation,
            crate::snu::gates::HelperSecurityGateSupport::LegacyUnrestricted,
        )
        .await;
    let text = match first {
        Ok(message) => {
            mark_payload_verified(broker, payload).await;
            return Ok(message);
        }
        Err(BridgeError::ActionFailed(text)) => text,
        Err(error) => return Err(error.into()),
    };

    if is_transient_helper_text(&text) {
        sleep(HELPER_SETTLE_DELAY).await;
        let retry_preflight = preflight_for_dispatch(broker, payload, timeout_secs).await?;
        if retry_preflight.decision != GateDecision::LegacyUnrestricted {
            anyhow::bail!(
                "SN-Utils helper changed protocol generation while retrying a legacy action; retry the command"
            );
        }
        let message = broker
            .manager
            .request_on_generation_with_support(
                payload,
                Matcher::Action(expected_action.to_string()),
                timeout_secs,
                retry_preflight.generation,
                crate::snu::gates::HelperSecurityGateSupport::LegacyUnrestricted,
            )
            .await?;
        mark_payload_verified(broker, payload).await;
        return Ok(message);
    }

    let instance = payload
        .get("instance")
        .and_then(|value| serde_json::from_value::<SnuInstance>(value.clone()).ok());
    let (Some(instance), true) = (instance, is_stale_token_text(&text)) else {
        return Err(BridgeError::ActionFailed(text).into());
    };

    match probe_session(broker, &instance, timeout_secs).await {
        Ok(true) => {
            let detail = crate::client::error::safe_error_detail(&text);
            return Err(anyhow!(
                "ServiceNow denied this action, but the browser session is still valid — this is a permissions (ACL) problem, not an expired token, so re-running /token will not help: {detail}"
            ));
        }
        Ok(false) => {}
        Err(error) => {
            return Err(error.context(
                "could not verify the browser session after an authentication-like legacy action failure; the cached session was preserved",
            ));
        }
    }

    let target_origin = normalize_origin(&instance.url);
    let fresh = refresh_session(broker, timeout_secs, target_origin.as_deref()).await?;
    if let Some(object) = payload.as_object_mut() {
        object.insert("instance".to_string(), serde_json::to_value(&fresh)?);
    }
    sleep(HELPER_SETTLE_DELAY).await;
    let retry_preflight = preflight_for_dispatch(broker, payload, timeout_secs).await?;
    if retry_preflight.decision != GateDecision::LegacyUnrestricted {
        anyhow::bail!(
            "SN-Utils helper changed protocol generation while refreshing a legacy action; retry the command"
        );
    }
    let message = broker
        .manager
        .request_on_generation_with_support(
            payload,
            Matcher::Action(expected_action.to_string()),
            timeout_secs,
            retry_preflight.generation,
            crate::snu::gates::HelperSecurityGateSupport::LegacyUnrestricted,
        )
        .await?;
    mark_payload_verified(broker, payload).await;
    Ok(message)
}

/// Record a successful round-trip for the session origin the payload used.
async fn mark_payload_verified(broker: &Broker, payload: &Value) {
    let origin = payload
        .get("instance")
        .and_then(|instance| instance.get("url"))
        .and_then(Value::as_str)
        .and_then(normalize_origin);
    if let Some(origin) = origin {
        broker.state.lock().await.mark_verified(&origin);
    }
}

/// Cheapest known-good round-trip that exercises the session's `g_ck`: a
/// 1-record, sys_id-only query against `sys_user` (readable by any
/// authenticated user). `Ok(true)` = ServiceNow accepted the token,
/// `Ok(false)` = it rejected the session as unauthenticated, `Err` = the probe
/// could not run at all (e.g. helper tab gone), which callers must treat as
/// "not proven valid".
async fn probe_session(
    broker: &Broker,
    instance: &SnuInstance,
    timeout_secs: u64,
) -> anyhow::Result<bool> {
    let correlation_id = format!("snow_probe_{}", uuid::Uuid::new_v4().simple());
    let payload = serde_json::json!({
        "action": "agentQueryRecords",
        "agentRequestId": correlation_id,
        "tableName": "sys_user",
        "queryString": "sysparm_fields=sys_id&sysparm_limit=1",
        "instance": instance,
        "appName": "snow-cli",
    });
    let probe_timeout = timeout_secs.clamp(1, PROBE_TIMEOUT_SECS_MAX);
    match broker
        .manager
        .request(
            &payload,
            Matcher::Correlation(correlation_id),
            probe_timeout,
        )
        .await
    {
        Ok(_) => Ok(true),
        Err(BridgeError::ActionFailed(text)) if is_probe_stale_token_text(&text) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// `true` for helper-tab errors that are transient — its own fetch returned an
/// empty or unparseable response, typically while the tab re-authenticates or
/// the instance hiccups. Safe to retry once; never a reason to prompt for
/// `/token` or evict the session.
fn is_transient_helper_text(text: &str) -> bool {
    let text = text.to_lowercase();
    text.contains("unexpected end of json input")
        || text.contains("failed to execute 'json'")
        || text.contains("failed to fetch")
        || text.contains("networkerror")
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
    let instance = broker
        .manager
        .wait_for_session(timeout_secs, origin)
        .await?;
    broker.state.lock().await.remember_session(&instance);
    persist_broker_sessions(broker).await;
    Ok(instance)
}

/// `true` when the helper tab's error text says ServiceNow rejected the request
/// because the `g_ck` token is expired/invalid, meaning we should refresh it
/// from SN-Utils and retry. This classifies application-level error text, not
/// socket state — socket death is a typed [`BridgeError`] from the manager.
fn is_probe_stale_token_text(text: &str) -> bool {
    let text = text.to_lowercase();
    text.contains("http 401") || text.contains("not authenticated") || text.contains("unauthorized")
}

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
        error_code: None,
        error: None,
        message: None,
        instance: None,
        status: None,
        cleared: None,
        token_valid: None,
    }
}

fn message_response(message: SnuMessage) -> BrokerResponse {
    BrokerResponse {
        ok: true,
        error_code: None,
        error: None,
        message: Some(message),
        instance: None,
        status: None,
        cleared: None,
        token_valid: None,
    }
}

/// Probe the broker IPC with a full Ping round-trip. Merely being able to
/// connect is not enough: any process could own the port, and treating a
/// foreign listener as a live broker would make every subsequent request fail
/// confusingly. Requiring a parseable `ok` response also flushes out a stale
/// broker that accepts but no longer answers.
async fn connect_once(
    endpoint: &BrokerEndpoint,
    auth_token: Option<&Secret>,
) -> anyhow::Result<()> {
    let envelope = BrokerEnvelope {
        auth_token: auth_token.map(|secret| secret.expose_secret().to_string()),
        request: BrokerRequest::Ping,
    };
    let response = timeout(
        Duration::from_secs(2),
        send_ipc_envelope(endpoint, &envelope),
    )
    .await
    .map_err(|_| anyhow!("SN-Utils broker did not answer ping"))??;
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
#[allow(
    unsafe_code,
    reason = "pre_exec requires unsafe; the closure only calls async-signal-safe setsid"
)]
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

async fn wait_until_ready(
    endpoint: &BrokerEndpoint,
    auth_token: Option<&Secret>,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(BROKER_READY_TIMEOUT_SECS);
    loop {
        if connect_once(endpoint, auth_token).await.is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow!(
                "timed out waiting for SN-Utils broker on {}",
                endpoint.display_addr()
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
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    async fn gated_test_bridge() -> (
        Arc<Broker>,
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) {
        let config = BridgeConfig {
            addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            heartbeat_interval: Duration::from_secs(60),
            connect_timeout: Duration::from_secs(2),
            rebind_delay: Duration::from_millis(50),
            bridge_id: String::new(),
            allowed_origins: crate::snu::bridge::default_allowed_origins(),
            allow_native_peers: false,
        };
        let (manager, _sessions) = BridgeManager::start(config);
        let ws_addr = manager
            .wait_bound(Duration::from_secs(2))
            .await
            .expect("bridge bound");
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut request = format!("ws://{ws_addr}").into_client_request().unwrap();
        request.headers_mut().insert(
            "origin",
            crate::snu::bridge::DEFAULT_ALLOWED_EXTENSION_ORIGINS[0]
                .parse()
                .unwrap(),
        );
        let (mut helper, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let hello = helper.next().await.unwrap().unwrap();
        assert!(hello.to_text().unwrap().contains("hostHello"));
        (
            Arc::new(Broker::new(
                Duration::from_secs(300),
                manager,
                "127.0.0.1:1979".to_string(),
            )),
            helper,
        )
    }

    async fn answer_capabilities(
        helper: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        mode: &str,
    ) {
        let capability_request = helper.next().await.unwrap().unwrap();
        let capability_request: Value =
            serde_json::from_str(capability_request.to_text().unwrap()).unwrap();
        assert_eq!(capability_request["action"], "agentGetCapabilities");
        helper
            .send(Message::Text(
                serde_json::json!({
                    "action": "agentGetCapabilitiesResponse",
                    "agentRequestId": capability_request["agentRequestId"],
                    "success": true,
                    "capabilities": {"protocolVersion": 1, "instanceSecurityGates": 1},
                    "instanceGates": {
                        "https://dev.service-now.com": {
                            "revision": 1,
                            "gates": {
                                "backgroundScripts": mode,
                                "deleteRecords": "approve",
                                "createArtifacts": "auto",
                                "restRequest": "auto",
                                "browserDebugger": "auto"
                            }
                        }
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
    }

    /// Answer the capability check with explicit per-gate modes.
    async fn answer_capabilities_with(
        helper: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        gates: Value,
    ) {
        let capability_request = helper.next().await.unwrap().unwrap();
        let capability_request: Value =
            serde_json::from_str(capability_request.to_text().unwrap()).unwrap();
        assert_eq!(capability_request["action"], "agentGetCapabilities");
        helper
            .send(Message::Text(
                serde_json::json!({
                    "action": "agentGetCapabilitiesResponse",
                    "agentRequestId": capability_request["agentRequestId"],
                    "success": true,
                    "capabilities": {"protocolVersion": 1, "instanceSecurityGates": 1},
                    "instanceGates": {
                        "https://dev.service-now.com": {"revision": 1, "gates": gates}
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
    }

    type HelperWs = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    async fn answer_capabilities_review(helper: &mut HelperWs, gates: Value, command_review: bool) {
        let capability_request = helper.next().await.unwrap().unwrap();
        let capability_request: Value =
            serde_json::from_str(capability_request.to_text().unwrap()).unwrap();
        assert_eq!(capability_request["action"], "agentGetCapabilities");
        let mut capabilities =
            serde_json::json!({"protocolVersion": 1, "instanceSecurityGates": 1});
        if command_review {
            capabilities["commandReview"] = Value::from(1);
        }
        helper
            .send(Message::Text(
                serde_json::json!({
                    "action": "agentGetCapabilitiesResponse",
                    "agentRequestId": capability_request["agentRequestId"],
                    "success": true,
                    "capabilities": capabilities,
                    "instanceGates": {
                        "https://dev.service-now.com": {"revision": 1, "gates": gates}
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
    }

    async fn next_json(helper: &mut HelperWs) -> Value {
        let message = tokio::time::timeout(Duration::from_secs(2), helper.next())
            .await
            .expect("helper receives a message")
            .unwrap()
            .unwrap();
        serde_json::from_str(message.to_text().unwrap()).unwrap()
    }

    async fn send_json(helper: &mut HelperWs, value: Value) {
        helper
            .send(Message::Text(value.to_string().into()))
            .await
            .unwrap();
    }

    fn approve_gates() -> Value {
        serde_json::json!({"backgroundScripts": "auto", "deleteRecords": "approve",
            "createArtifacts": "auto", "updateRecords": "approve",
            "restRequest": "auto", "browserDebugger": "auto"})
    }

    fn spawn_dispatch(
        broker: &Arc<Broker>,
        payload: Value,
    ) -> tokio::task::JoinHandle<anyhow::Result<BrokerResponse>> {
        let broker = Arc::clone(broker);
        let correlation_id = payload["agentRequestId"].as_str().unwrap().to_string();
        tokio::spawn(async move {
            dispatch(
                BrokerRequest::SendAction {
                    payload,
                    correlation_id,
                    timeout_secs: 3,
                },
                &broker,
            )
            .await
        })
    }

    fn rest_delete() -> Value {
        serde_json::json!({
            "action": "agentRestApi", "agentRequestId": "del-1", "method": "DELETE",
            "endpoint": "/api/now/table/incident/abc",
            "instance": {"name": "dev", "url": "https://dev.service-now.com", "g_ck": "secret-token"}
        })
    }

    /// Answer the reviewRequest, returning it for inspection.
    async fn answer_review(helper: &mut HelperWs, approved: bool, tamper_nonce: bool) -> Value {
        let review = next_json(helper).await;
        assert_eq!(review["action"], "reviewRequest");
        send_json(
            helper,
            serde_json::json!({
                "action": "reviewResponse",
                "agentRequestId": review["agentRequestId"],
                "reviewId": review["reviewId"],
                "nonce": if tamper_nonce { Value::from("forged") } else { review["nonce"].clone() },
                "payloadHash": review["payloadHash"],
                "approved": approved,
                "userFeedback": if approved { Value::Null } else { Value::from("not today") },
            }),
        )
        .await;
        review
    }

    async fn assert_nothing_sent(helper: &mut HelperWs) {
        if let Ok(Some(Ok(Message::Text(text)))) =
            tokio::time::timeout(Duration::from_millis(200), helper.next()).await
        {
            let value: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(
                value["action"], "cancelReview",
                "unexpected message: {text}"
            );
        }
    }

    #[tokio::test]
    async fn gated_page_actions_need_one_exact_instance_url() {
        let auto = serde_json::json!({"backgroundScripts": "auto", "deleteRecords": "auto",
            "createArtifacts": "auto", "updateRecords": "auto",
            "restRequest": "auto", "browserDebugger": "auto"});
        let mut blocked = auto.clone();
        blocked["updateRecords"] = Value::from("off");
        let cases = [
            // The URL names the permissive instance, but the helper acts on the tab.
            serde_json::json!({"action": "agentRunUiAction", "agentRequestId": "t-1",
                "uiAction": "sysverb_update", "tabId": 42,
                "url": "https://dev.service-now.com/*"}),
            serde_json::json!({"action": "agentRunUiAction", "agentRequestId": "t-2",
                "uiAction": "sysverb_update", "tabId": 42}),
            // A pattern covering both instances.
            serde_json::json!({"action": "agentRunUiAction", "agentRequestId": "t-3",
                "uiAction": "sysverb_update", "url": "https://*.service-now.com/*"}),
        ];
        for payload in cases {
            let (broker, mut helper) = gated_test_bridge().await;
            let request = spawn_dispatch(&broker, payload.clone());
            let capability_request = next_json(&mut helper).await;
            send_json(
                &mut helper,
                serde_json::json!({
                    "action": "agentGetCapabilitiesResponse",
                    "agentRequestId": capability_request["agentRequestId"],
                    "success": true,
                    "capabilities": {"protocolVersion": 1, "instanceSecurityGates": 1},
                    "instanceGates": {
                        "https://dev.service-now.com": {"revision": 1, "gates": auto},
                        "https://prod.service-now.com": {"revision": 1, "gates": blocked},
                    }
                }),
            )
            .await;
            let error = request.await.unwrap().unwrap_err();
            assert_eq!(
                error.downcast_ref::<BridgeError>().unwrap().code(),
                "SNU_GATE_STATE_UNAVAILABLE",
                "payload: {payload}"
            );
            assert!(
                format!("{error:#}").contains("exact instance URL"),
                "{error:#}"
            );
            assert_nothing_sent(&mut helper).await;
        }
    }

    #[tokio::test]
    async fn approve_mode_rest_delete_is_reviewed_then_dispatched() {
        let (broker, mut helper) = gated_test_bridge().await;
        let request = spawn_dispatch(&broker, rest_delete());
        answer_capabilities_review(&mut helper, approve_gates(), true).await;
        let review = answer_review(&mut helper, true, false).await;
        answer_capabilities_review(&mut helper, approve_gates(), true).await;
        assert_eq!(review["command"], "rest_request");
        assert_eq!(review["params"]["method"], "DELETE");
        assert!(
            !review.to_string().contains("secret-token"),
            "no session token on the card"
        );

        let execute = next_json(&mut helper).await;
        assert_eq!(execute["action"], "executeApproved");
        assert_eq!(execute["agentRequestId"], "del-1");
        assert_eq!(execute["nonce"], review["nonce"]);
        send_json(&mut helper, serde_json::json!({"agentRequestId": "del-1", "success": true, "approvedNotExecuted": true})).await;
        // The gate is checked once more right before dispatch.
        answer_capabilities_review(&mut helper, approve_gates(), true).await;

        let action = next_json(&mut helper).await;
        assert_eq!(
            action["action"], "agentRestApi",
            "dispatched only after approval"
        );
        send_json(&mut helper, serde_json::json!({"action": "agentRestApiResponse", "agentRequestId": "del-1", "success": true})).await;
        assert!(request.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn approve_mode_ui_action_is_run_by_the_helper_after_approval() {
        let (broker, mut helper) = gated_test_bridge().await;
        let payload = serde_json::json!({
            "action": "agentRunUiAction", "agentRequestId": "ui-1", "uiAction": "sysverb_update",
            "url": "https://dev.service-now.com/*",
        });
        let request = spawn_dispatch(&broker, payload);
        answer_capabilities_review(&mut helper, approve_gates(), true).await;
        let review = answer_review(&mut helper, true, false).await;
        answer_capabilities_review(&mut helper, approve_gates(), true).await;
        assert_eq!(review["command"], "run_ui_action");
        assert_eq!(review["params"]["uiAction"], "sysverb_update");
        let execute = next_json(&mut helper).await;
        assert_eq!(execute["action"], "executeApproved");
        send_json(&mut helper, serde_json::json!({"action": "agentRunUiActionResponse", "agentRequestId": "ui-1", "success": true})).await;
        assert!(request.await.unwrap().is_ok());
        assert_nothing_sent(&mut helper).await;
    }

    #[tokio::test]
    async fn rejected_or_forged_reviews_run_nothing() {
        for (approved, tamper, code) in [
            (false, false, "SNU_REVIEW_REJECTED"),
            (true, true, "SNU_REVIEW_MISMATCH"),
        ] {
            let (broker, mut helper) = gated_test_bridge().await;
            let request = spawn_dispatch(&broker, rest_delete());
            answer_capabilities_review(&mut helper, approve_gates(), true).await;
            answer_review(&mut helper, approved, tamper).await;
            let error = request.await.unwrap().unwrap_err();
            let bridge = error.downcast_ref::<BridgeError>().expect("bridge error");
            assert_eq!(bridge.code(), code);
            if !approved {
                assert!(error.to_string().contains("not today"), "{error}");
            }
            assert_nothing_sent(&mut helper).await;
        }
    }

    #[tokio::test]
    async fn a_gate_switched_off_during_review_runs_nothing() {
        let (broker, mut helper) = gated_test_bridge().await;
        let request = spawn_dispatch(&broker, rest_delete());
        answer_capabilities_review(&mut helper, approve_gates(), true).await;
        answer_review(&mut helper, true, false).await;
        let mut off = approve_gates();
        off["deleteRecords"] = Value::from("off");
        answer_capabilities_review(&mut helper, off, true).await;
        let error = request.await.unwrap().unwrap_err();
        assert_eq!(
            error.downcast_ref::<BridgeError>().unwrap().code(),
            "SNU_GATE_BLOCKED"
        );
        assert_nothing_sent(&mut helper).await;
    }

    #[tokio::test]
    async fn an_unanswered_review_is_cancelled_and_a_late_approval_runs_nothing() {
        let (broker, mut helper) = gated_test_bridge().await;
        // A Batch API delete: classified by its sub-requests, so it is held.
        let mut payload = serde_json::json!({
            "action": "agentRestApi", "method": "POST", "endpoint": "/api/now/v1/batch",
            "body": {"rest_requests": [{"method": "DELETE", "url": "/api/now/table/incident/abc"}]},
            "instance": {"name": "dev", "url": "https://dev.service-now.com", "g_ck": "secret-token"}
        });
        payload["agentRequestId"] = Value::from("late-1");
        let request = {
            let broker = Arc::clone(&broker);
            tokio::spawn(async move {
                dispatch(
                    BrokerRequest::SendAction {
                        payload,
                        correlation_id: "late-1".to_string(),
                        timeout_secs: 1,
                    },
                    &broker,
                )
                .await
            })
        };
        answer_capabilities_review(&mut helper, approve_gates(), true).await;
        let review = next_json(&mut helper).await;
        assert_eq!(review["action"], "reviewRequest");
        let error = request.await.unwrap().unwrap_err();
        assert_eq!(
            error.downcast_ref::<BridgeError>().unwrap().code(),
            "SNU_APPROVAL_TIMEOUT"
        );
        let cancel = next_json(&mut helper).await;
        assert_eq!(cancel["action"], "cancelReview");
        assert_eq!(cancel["reviewId"], review["reviewId"]);
        // A late approval finds no waiter: nothing is executed or dispatched.
        send_json(
            &mut helper,
            serde_json::json!({"action": "reviewResponse", "agentRequestId": review["agentRequestId"],
                "reviewId": review["reviewId"], "nonce": review["nonce"],
                "payloadHash": review["payloadHash"], "approved": true}),
        )
        .await;
        assert!(
            tokio::time::timeout(Duration::from_millis(300), helper.next())
                .await
                .is_err(),
            "nothing may follow a late approval"
        );
    }

    #[tokio::test]
    async fn a_lost_reply_after_approval_is_not_called_an_approval_timeout() {
        let (broker, mut helper) = gated_test_bridge().await;
        let request = spawn_dispatch(&broker, rest_delete());
        answer_capabilities_review(&mut helper, approve_gates(), true).await;
        answer_review(&mut helper, true, false).await;
        answer_capabilities_review(&mut helper, approve_gates(), true).await;
        let execute = next_json(&mut helper).await;
        assert_eq!(execute["action"], "executeApproved");
        send_json(&mut helper, serde_json::json!({"agentRequestId": "del-1", "success": true, "approvedNotExecuted": true})).await;
        // The gate is checked once more right before dispatch.
        answer_capabilities_review(&mut helper, approve_gates(), true).await;
        let action = next_json(&mut helper).await;
        assert_eq!(action["action"], "agentRestApi");
        // The helper never answers the dispatched delete.
        let error = request.await.unwrap().unwrap_err();
        assert_eq!(
            error.downcast_ref::<BridgeError>().unwrap().code(),
            "SNU_APPROVED_NO_REPLY"
        );
    }

    #[tokio::test]
    async fn approve_mode_fails_closed_without_review_support() {
        let (broker, mut helper) = gated_test_bridge().await;
        let request = spawn_dispatch(&broker, rest_delete());
        answer_capabilities_review(&mut helper, approve_gates(), false).await;
        let error = request.await.unwrap().unwrap_err();
        assert_eq!(
            error.downcast_ref::<BridgeError>().unwrap().code(),
            "SNU_REVIEW_UNSUPPORTED"
        );
        assert_nothing_sent(&mut helper).await;
    }

    #[tokio::test]
    async fn user_gate_settings_block_form_commits_and_batch_deletes() {
        let instance = serde_json::json!({
            "name": "dev", "url": "https://dev.service-now.com", "g_ck": "token"
        });
        let cases = [
            (
                serde_json::json!({"action": "agentRunUiAction", "uiAction": "sysverb_update",
                    "url": "https://dev.service-now.com/incident.do*"}),
                serde_json::json!({"backgroundScripts": "auto", "deleteRecords": "auto",
                    "createArtifacts": "auto", "updateRecords": "off",
                    "restRequest": "auto", "browserDebugger": "auto"}),
                "updateRecords",
            ),
            (
                // An older helper without updateRecords: it follows createArtifacts.
                serde_json::json!({"action": "agentRunUiAction", "uiAction": "sysverb_update",
                    "url": "https://dev.service-now.com/incident.do*"}),
                serde_json::json!({"backgroundScripts": "auto", "deleteRecords": "auto",
                    "createArtifacts": "off", "restRequest": "auto", "browserDebugger": "auto"}),
                "updateRecords",
            ),
            (
                serde_json::json!({"action": "agentRestApi", "method": "POST",
                    "endpoint": "/api/now/v1/batch",
                    "body": {"rest_requests": [{"method": "DELETE", "url": "/api/now/table/incident/x"}]},
                    "instance": instance}),
                serde_json::json!({"backgroundScripts": "auto", "deleteRecords": "off",
                    "createArtifacts": "auto", "restRequest": "auto", "browserDebugger": "auto"}),
                "deleteRecords",
            ),
        ];
        for (mut payload, gates, blocked_gate) in cases {
            let (broker, mut helper) = gated_test_bridge().await;
            payload["agentRequestId"] = Value::String("blocked-1".to_string());
            let request_broker = Arc::clone(&broker);
            let request = tokio::spawn(async move {
                dispatch(
                    BrokerRequest::SendAction {
                        payload,
                        correlation_id: "blocked-1".to_string(),
                        timeout_secs: 2,
                    },
                    &request_broker,
                )
                .await
            });
            answer_capabilities_with(&mut helper, gates).await;
            let error = request.await.unwrap().unwrap_err();
            let message = format!("{error:#}");
            assert_eq!(
                error.downcast_ref::<BridgeError>().unwrap().code(),
                "SNU_GATE_BLOCKED",
                "{message}"
            );
            assert!(message.contains(blocked_gate), "{message}");
            // Nothing beyond the capability check reached the browser.
            assert!(
                tokio::time::timeout(Duration::from_millis(200), helper.next())
                    .await
                    .is_err(),
                "a blocked action must not be sent"
            );
        }
    }

    #[tokio::test]
    async fn approve_and_auto_actions_are_dispatched() {
        for mode in ["approve", "auto"] {
            let (broker, mut helper) = gated_test_bridge().await;
            let request_broker = Arc::clone(&broker);
            let current_payload = serde_json::json!({
                "action": "agentRunBackgroundScript",
                "agentRequestId": "action-1",
                "script": "gs.info('test')",
                "instance": {
                    "name": "dev",
                    "url": "https://dev.service-now.com",
                    "g_ck": "token"
                }
            });
            let legacy_payload = serde_json::json!({
                "action": "executeBackgroundScript",
                "content": "gs.info('test')",
                "instance": {
                    "name": "dev",
                    "url": "https://dev.service-now.com",
                    "g_ck": "token"
                }
            });
            let request = tokio::spawn(async move {
                dispatch(
                    BrokerRequest::SendActionWithLegacyFallback {
                        current_payload,
                        correlation_id: "action-1".into(),
                        legacy_payload,
                        legacy_expected_action: "responseFromBackgroundScript".into(),
                        timeout_secs: 2,
                    },
                    &request_broker,
                )
                .await
            });

            answer_capabilities(&mut helper, mode).await;
            let action = helper.next().await.unwrap().unwrap();
            let action: Value = serde_json::from_str(action.to_text().unwrap()).unwrap();
            assert_eq!(action["action"], "agentRunBackgroundScript");
            helper
                .send(Message::Text(
                    serde_json::json!({
                        "action": "responseFromBackgroundScript",
                        "agentRequestId": "action-1",
                        "success": true
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            assert!(request.await.unwrap().is_ok(), "gate mode: {mode}");
        }
    }

    #[tokio::test]
    async fn community_agent_actions_are_dispatched_and_correlated() {
        for action_name in [
            "agentGetContext",
            "switchContext",
            "agentGetFormState",
            "agentSetField",
            "agentRunUiAction",
            "agentClickElement",
            "agentNavigate",
            "agentGetParentOptions",
            "agentRestApi",
        ] {
            let (broker, mut helper) = gated_test_bridge().await;
            let request_broker = Arc::clone(&broker);
            let correlation_id = format!("{action_name}-1");
            let mut payload = serde_json::json!({
                "action": action_name,
                "agentRequestId": correlation_id,
                "instance": {
                    "name": "dev",
                    "url": "https://dev.service-now.com",
                    "g_ck": "token"
                }
            });
            if action_name == "agentRestApi" {
                payload["method"] = Value::String("GET".to_string());
            }
            if action_name == "agentRunUiAction" {
                // A gated page action names its instance by URL.
                payload["url"] = Value::String("https://dev.service-now.com/*".to_string());
            }
            let request_correlation_id = correlation_id.clone();
            let request = tokio::spawn(async move {
                dispatch(
                    BrokerRequest::SendAction {
                        payload,
                        correlation_id: request_correlation_id,
                        timeout_secs: 2,
                    },
                    &request_broker,
                )
                .await
            });

            if action_name == "agentRunUiAction" {
                // Committing a form is gated on updateRecords.
                answer_capabilities(&mut helper, "auto").await;
            }
            let action = helper.next().await.unwrap().unwrap();
            let action: Value = serde_json::from_str(action.to_text().unwrap()).unwrap();
            assert_eq!(action["action"], action_name);
            helper
                .send(Message::Text(
                    serde_json::json!({
                        "action": format!("{action_name}Response"),
                        "agentRequestId": correlation_id,
                        "success": true
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            assert!(request.await.unwrap().is_ok(), "action: {action_name}");
        }
    }

    #[tokio::test]
    async fn screenshot_dispatch_uses_selected_instance_for_gate_preflight() {
        let (broker, mut helper) = gated_test_bridge().await;
        let request_broker = Arc::clone(&broker);
        let payload = serde_json::json!({
            "action": "takeScreenshot",
            "agentRequestId": "screenshot-1",
            // Only debugger-based capture is gated, and a gated page action
            // names its instance by URL.
            "cdp": true,
            "url": "https://dev.service-now.com/*",
            "instance": {
                "name": "dev",
                "url": "https://dev.service-now.com",
                "g_ck": "token"
            }
        });
        let request = tokio::spawn(async move {
            dispatch(
                BrokerRequest::SendAction {
                    payload,
                    correlation_id: "screenshot-1".into(),
                    timeout_secs: 2,
                },
                &request_broker,
            )
            .await
        });

        answer_capabilities(&mut helper, "auto").await;
        let action = helper.next().await.unwrap().unwrap();
        let action: Value = serde_json::from_str(action.to_text().unwrap()).unwrap();
        assert_eq!(action["action"], "takeScreenshot");
        assert_eq!(action["instance"]["url"], "https://dev.service-now.com");
        helper
            .send(Message::Text(
                serde_json::json!({
                    "action": "takeScreenshotResponse",
                    "agentRequestId": "screenshot-1",
                    "success": true
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        assert!(request.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn rest_write_actions_negotiate_their_gate_and_dispatch() {
        for method in ["POST", "PUT", "PATCH", "DELETE"] {
            let (broker, mut helper) = gated_test_bridge().await;
            let request_broker = Arc::clone(&broker);
            let correlation_id = format!("rest-{method}");
            let payload = serde_json::json!({
                "action": "agentRestApi",
                "agentRequestId": correlation_id,
                "method": method,
                "endpoint": "/api/now/table/incident",
                "instance": {
                    "name": "dev",
                    "url": "https://dev.service-now.com",
                    "g_ck": "token"
                }
            });
            let request_correlation_id = correlation_id.clone();
            let request = tokio::spawn(async move {
                dispatch(
                    BrokerRequest::SendAction {
                        payload,
                        correlation_id: request_correlation_id,
                        timeout_secs: 2,
                    },
                    &request_broker,
                )
                .await
            });

            // Every gate on auto: approve mode is covered by the review tests.
            answer_capabilities_with(
                &mut helper,
                serde_json::json!({"backgroundScripts": "auto", "deleteRecords": "auto",
                    "createArtifacts": "auto", "restRequest": "auto", "browserDebugger": "auto"}),
            )
            .await;
            let action = helper.next().await.unwrap().unwrap();
            let action: Value = serde_json::from_str(action.to_text().unwrap()).unwrap();
            assert_eq!(action["action"], "agentRestApi");
            assert_eq!(action["method"], method);
            helper
                .send(Message::Text(
                    serde_json::json!({
                        "action": "agentRestApiResponse",
                        "agentRequestId": correlation_id,
                        "success": true
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            assert!(request.await.unwrap().is_ok(), "REST method: {method}");
        }
    }

    #[tokio::test]
    async fn legacy_helper_is_labeled_and_action_is_dispatched() {
        let (broker, mut helper) = gated_test_bridge().await;
        let request_broker = Arc::clone(&broker);
        let current_payload = serde_json::json!({
            "action": "agentRunBackgroundScript",
            "agentRequestId": "legacy-1",
            "script": "gs.info('legacy')",
            "instance": {
                "name": "dev",
                "url": "https://dev.service-now.com",
                "g_ck": "token"
            }
        });
        let legacy_payload = serde_json::json!({
            "action": "executeBackgroundScript",
            "content": "gs.info('legacy')",
            "instance": {
                "name": "dev",
                "url": "https://dev.service-now.com",
                "g_ck": "token"
            }
        });
        let request = tokio::spawn(async move {
            dispatch(
                BrokerRequest::SendActionWithLegacyFallback {
                    current_payload,
                    correlation_id: "legacy-1".into(),
                    legacy_payload,
                    legacy_expected_action: "responseFromBackgroundScript".into(),
                    timeout_secs: 2,
                },
                &request_broker,
            )
            .await
        });

        let capability_request = helper.next().await.unwrap().unwrap();
        let capability_request: Value =
            serde_json::from_str(capability_request.to_text().unwrap()).unwrap();
        helper
            .send(Message::Text(
                serde_json::json!({
                    "action": "agentGetCapabilitiesResponse",
                    "agentRequestId": capability_request["agentRequestId"],
                    "success": true,
                    "tier": "community"
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        let action = helper.next().await.unwrap().unwrap();
        assert!(
            action
                .to_text()
                .unwrap()
                .contains("executeBackgroundScript")
        );
        helper
            .send(Message::Text(
                serde_json::json!({
                    "action": "responseFromBackgroundScript",
                    "success": true
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();

        assert!(request.await.unwrap().is_ok());
        assert_eq!(
            broker.manager.helper_status().await.security_gate_support,
            crate::snu::gates::HelperSecurityGateSupport::LegacyUnrestricted
        );
    }

    #[tokio::test]
    async fn blocked_action_never_reaches_helper_socket() {
        let (broker, mut helper) = gated_test_bridge().await;
        let request_broker = Arc::clone(&broker);
        let payload = serde_json::json!({
            "action": "executeBackgroundScript",
            "agentRequestId": "action-1",
            "content": "TOP_SECRET_SCRIPT",
            "instance": {
                "name": "dev",
                "url": "https://dev.service-now.com",
                "g_ck": "TOP_SECRET_TOKEN"
            }
        });
        let request = tokio::spawn(async move {
            dispatch(
                BrokerRequest::SendAction {
                    payload,
                    correlation_id: "action-1".into(),
                    timeout_secs: 2,
                },
                &request_broker,
            )
            .await
        });

        answer_capabilities(&mut helper, "off").await;

        let error = request.await.unwrap().unwrap_err();
        let text = error.to_string();
        assert!(text.contains("backgroundScripts") && text.contains("blocks actions"));
        assert!(!text.contains("TOP_SECRET"));
        assert!(
            timeout(Duration::from_millis(150), helper.next())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn queued_legacy_payload_uses_newest_cached_session() {
        let (broker, _helper) = gated_test_bridge().await;
        broker.state.lock().await.remember_session(&SnuInstance {
            name: "dev".to_string(),
            url: "https://dev.service-now.com".to_string(),
            g_ck: Some(crate::models::secret::Secret::new(
                "fresh-token".to_string(),
            )),
            scope: None,
        });
        let mut payload = serde_json::json!({
            "action": "executeBackgroundScript",
            "instance": {
                "name": "dev",
                "url": "https://dev.service-now.com",
                "g_ck": "stale-token"
            }
        });
        refresh_payload_instance_from_cache(&broker, &mut payload)
            .await
            .unwrap();
        assert_eq!(payload["instance"]["g_ck"], "fresh-token");
    }

    fn instance(url: &str) -> SnuInstance {
        SnuInstance {
            name: url.to_string(),
            url: url.to_string(),
            g_ck: Some(crate::models::secret::Secret::new("token".to_string())),
            scope: None,
        }
    }

    fn state() -> BrokerState {
        BrokerState::new(Duration::from_secs(300), "127.0.0.1:1979".to_string())
    }

    fn instance_with_token(url: &str, token: &str) -> SnuInstance {
        SnuInstance {
            name: url.to_string(),
            url: url.to_string(),
            g_ck: Some(crate::models::secret::Secret::new(token.to_string())),
            scope: None,
        }
    }

    #[test]
    fn remember_session_resets_freshness_only_when_token_changes() {
        let mut state = state();
        let origin = "https://a.service-now.com:443";
        state.remember_session(&instance_with_token("https://a.service-now.com", "t1"));
        let captured = state.meta_by_origin[origin].captured_at;
        assert!(captured.is_some());
        state.mark_verified(origin);
        assert!(state.meta_by_origin[origin].last_verified_at.is_some());

        // Re-remembering the same token (WaitSession cache hit) keeps the clock.
        state.remember_session(&instance_with_token("https://a.service-now.com", "t1"));
        assert_eq!(state.meta_by_origin[origin].captured_at, captured);
        assert!(state.meta_by_origin[origin].last_verified_at.is_some());

        // A new token resets capture time and clears the verification.
        state.remember_session(&instance_with_token("https://a.service-now.com", "t2"));
        assert!(state.meta_by_origin[origin].captured_at.is_some());
        assert!(state.meta_by_origin[origin].last_verified_at.is_none());
    }

    #[test]
    fn mark_verified_ignores_unknown_origin_and_clear_drops_meta() {
        let mut state = state();
        state.mark_verified("https://ghost.service-now.com:443");
        assert!(state.meta_by_origin.is_empty());

        let origin = "https://a.service-now.com:443";
        state.remember_session(&instance("https://a.service-now.com"));
        state.mark_verified(origin);
        assert!(state.meta_by_origin[origin].last_verified_at.is_some());

        state.clear_origin(origin);
        assert!(state.meta_by_origin.is_empty());

        state.remember_session(&instance("https://a.service-now.com"));
        state.clear_all();
        assert!(state.meta_by_origin.is_empty());
    }

    #[test]
    fn status_reports_session_freshness() {
        let mut state = state();
        let origin = "https://a.service-now.com:443";
        state.remember_session(&instance("https://a.service-now.com"));
        let before_verify = state.status(true, HelperStatus::default());
        assert!(before_verify.instances[0].captured_at.is_some());
        assert!(before_verify.instances[0].last_verified_at.is_none());

        state.mark_verified(origin);
        let after_verify = state.status(true, HelperStatus::default());
        assert!(after_verify.instances[0].last_verified_at.is_some());
    }

    #[test]
    fn transient_helper_text_is_retryable_not_stale() {
        for text in [
            "Failed to execute 'json' on 'Response': Unexpected end of JSON input",
            "TypeError: Failed to fetch",
            "NetworkError when attempting to fetch resource",
        ] {
            assert!(
                is_transient_helper_text(text),
                "should be transient: {text}"
            );
            assert!(!is_stale_token_text(text), "must not read as stale: {text}");
        }
        assert!(!is_transient_helper_text("User is not authenticated"));
        assert!(is_stale_token_text("User is not authenticated"));
        assert!(is_probe_stale_token_text("HTTP 401 Unauthorized"));
        assert!(!is_probe_stale_token_text("HTTP 403 Forbidden"));
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
        assert_eq!(
            state.status(false, HelperStatus::default()).session_count,
            0
        );
    }

    #[test]
    fn status_lists_all_instances_with_latest_flag() {
        let mut state = state();
        state.remember_session(&instance("https://a.service-now.com"));
        state.remember_session(&instance("https://b.service-now.com"));

        let status = state.status(true, HelperStatus::default());
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

    #[tokio::test]
    async fn tcp_ipc_rejects_missing_or_wrong_auth_token_and_accepts_valid_token() {
        let (broker, _helper) = gated_test_bridge().await;
        let expected_token = Arc::new(Secret::new("a".repeat(64)));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server_broker = Arc::clone(&broker);
        let server_token = Arc::clone(&expected_token);
        let server = tokio::spawn(async move {
            for _ in 0..3 {
                let (stream, _) = listener.accept().await.unwrap();
                handle_client(
                    stream,
                    Arc::clone(&server_broker),
                    Some(Arc::clone(&server_token)),
                )
                .await
                .unwrap();
            }
        });

        let endpoint = BrokerEndpoint::Tcp(addr);

        let missing = send_ipc_envelope(
            &endpoint,
            &BrokerEnvelope {
                auth_token: None,
                request: BrokerRequest::Status,
            },
        )
        .await
        .unwrap();
        assert!(!missing.ok);
        assert_eq!(
            missing.error_code.as_deref(),
            Some("SNU_BROKER_UNAUTHORIZED")
        );

        let wrong = send_ipc_envelope(
            &endpoint,
            &BrokerEnvelope {
                auth_token: Some("b".repeat(64)),
                request: BrokerRequest::Status,
            },
        )
        .await
        .unwrap();
        assert!(!wrong.ok);
        assert_eq!(wrong.error_code.as_deref(), Some("SNU_BROKER_UNAUTHORIZED"));

        let valid = send_ipc_envelope(
            &endpoint,
            &BrokerEnvelope {
                auth_token: Some(expected_token.expose_secret().to_string()),
                request: BrokerRequest::Status,
            },
        )
        .await
        .unwrap();
        assert!(valid.ok);
        assert!(valid.status.is_some());

        server.await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unix_ipc_uses_private_dir_and_socket_and_checks_peer_uid() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let sock_dir = dir.path().join("run");
        let sock_path = sock_dir.join("snu.sock");
        let listener = bind_unix_ipc_socket(&sock_path).unwrap();

        let dir_mode = std::fs::metadata(&sock_dir).unwrap().permissions().mode() & 0o777;
        let sock_mode = std::fs::metadata(&sock_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(sock_mode, 0o600);

        let uid = current_euid();
        assert!(verify_peer_uid_matches(uid, uid).is_ok());
        assert!(verify_peer_uid_matches(uid.wrapping_add(1), uid).is_err());

        let (broker, _helper) = gated_test_bridge().await;
        let server_broker = Arc::clone(&broker);
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            verify_unix_peer_uid(&stream).unwrap();
            handle_client(stream, server_broker, None).await.unwrap();
        });

        let response = send_ipc_envelope(
            &BrokerEndpoint::Unix(sock_path.clone()),
            &BrokerEnvelope {
                auth_token: None,
                request: BrokerRequest::Ping,
            },
        )
        .await
        .unwrap();
        assert!(response.ok);

        server.await.unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn ipc_token_file_is_owner_only_and_refuses_symlinks() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join(BROKER_IPC_TOKEN_FILE_NAME);
        let first = load_or_create_ipc_token_at(&token_path).unwrap();
        let second = load_or_create_ipc_token_at(&token_path).unwrap();
        assert_eq!(first.expose_secret(), second.expose_secret());
        assert!(is_valid_ipc_token(first.expose_secret()));

        let mode = std::fs::metadata(&token_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        let target = dir.path().join("other");
        std::fs::write(&target, format!("{}\n", "c".repeat(64))).unwrap();
        let symlink_path = dir.path().join("symlink-token");
        std::os::unix::fs::symlink(&target, &symlink_path).unwrap();
        assert!(load_or_create_ipc_token_at(&symlink_path).is_err());
    }
}
