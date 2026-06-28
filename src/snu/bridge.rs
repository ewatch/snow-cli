use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, anyhow};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite::Message};

use crate::snu::protocol::{SnuInstance, SnuMessage, normalize_origin};

/// Loopback WebSocket address used by the SN-Utils ScriptSync helper tab. Keep
/// this stable because the browser-side helper is configured to connect here.
pub const DEFAULT_SNU_WS_ADDR: &str = "127.0.0.1:1978";

/// Maximum time to wait for the SN-Utils ScriptSync helper tab to *connect* to the
/// bridge. This is deliberately short and separate from the per-action/response
/// timeout: an installed, open helper tab reconnects within ~1s, so a long wait
/// here only ever penalizes the "SN-Utils is not running" case. The full
/// `timeout_secs` budget still applies to waiting for `/token` and action replies.
pub const HELPER_CONNECT_TIMEOUT_SECS: u64 = 20;

pub struct SnuBridge {
    socket: WebSocketStream<TcpStream>,
    peer_addr: SocketAddr,
}

impl fmt::Debug for SnuBridge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnuBridge")
            .field("peer_addr", &self.peer_addr)
            .finish_non_exhaustive()
    }
}

impl SnuBridge {
    pub async fn accept(timeout_secs: u64) -> anyhow::Result<Self> {
        let listener = match TcpListener::bind(DEFAULT_SNU_WS_ADDR).await {
            Ok(listener) => listener,
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
                return Err(anyhow!(
                    "SN-Utils bridge port {DEFAULT_SNU_WS_ADDR} is already in use. Only one bridge can own this port at a time, so this usually means another `snow-cli snu` command, the `sn-scriptsync` VS Code extension, or another process is bound to it. Stop the other owner and retry. (Concurrent SN-Utils commands are not yet supported; a multiplexing broker is tracked separately.)"
                ));
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("failed to bind SN-Utils bridge on {DEFAULT_SNU_WS_ADDR}")
                });
            }
        };

        eprintln!(
            "SN-Utils bridge listening on ws://{DEFAULT_SNU_WS_ADDR}. Open the SN-Utils ScriptSync helper tab if it is not already connected."
        );

        let accept_future = async {
            let (stream, peer_addr) = listener.accept().await?;
            let socket = accept_async(stream).await?;
            anyhow::Ok(Self { socket, peer_addr })
        };

        // Fail fast and uniformly when no helper tab is present: the connection
        // itself is near-instant when SN-Utils is running, so cap this wait well
        // below the (possibly large) response timeout.
        let connect_timeout = timeout_secs.min(HELPER_CONNECT_TIMEOUT_SECS);
        timeout(Duration::from_secs(connect_timeout), accept_future)
            .await
            .map_err(|_| {
                anyhow!(
                    "timed out waiting {connect_timeout}s for the SN-Utils ScriptSync helper tab to connect on ws://{DEFAULT_SNU_WS_ADDR}. Is SN-Utils installed and the ScriptSync helper tab open? It auto-connects within ~1s when running."
                )
            })?
    }

    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    pub async fn send_banner(&mut self, message: &str) -> anyhow::Result<()> {
        self.send_json(&serde_json::json!({
            "action": "bannerMessage",
            "message": message,
            "class": "alert alert-primary",
        }))
        .await
    }

    /// Wait for SN-Utils to push a `/token`. When `target_origin` is `Some`, only
    /// a token whose instance URL normalizes to that origin is accepted; tokens
    /// from other tabs in the same SN-Utils portal are discarded so the caller
    /// gets the `g_ck` for the instance it actually asked for.
    pub async fn wait_for_session(
        &mut self,
        timeout_secs: u64,
        target_origin: Option<&str>,
    ) -> anyhow::Result<SnuInstance> {
        let read_loop = async {
            loop {
                let msg = self.read_json_message().await?;
                if let Some(instance) = msg.instance
                    && instance
                        .g_ck
                        .as_deref()
                        .is_some_and(|token| !token.is_empty())
                {
                    if let Some(target) = target_origin
                        && normalize_origin(&instance.url).as_deref() != Some(target)
                    {
                        tracing::debug!(
                            url = %instance.url,
                            %target,
                            "ignoring /token from a non-target instance"
                        );
                        continue;
                    }
                    return Ok(instance);
                }
            }
        };

        timeout(Duration::from_secs(timeout_secs), read_loop)
            .await
            .map_err(|_| match target_origin {
                Some(target) => {
                    anyhow!(
                        "timed out waiting {timeout_secs}s for /token from SN-Utils for {target}"
                    )
                }
                None => anyhow!("timed out waiting {timeout_secs}s for /token from SN-Utils"),
            })?
    }

    pub async fn send_action_and_wait(
        &mut self,
        payload: &Value,
        correlation_id: &str,
        timeout_secs: u64,
    ) -> anyhow::Result<SnuMessage> {
        self.send_json(payload).await?;

        let read_loop = async {
            loop {
                let msg = self.read_json_message().await?;
                if msg.is_response_for(correlation_id) {
                    if msg.success == Some(false) || msg.error.is_some() {
                        return Err(anyhow!(
                            "SN-Utils action failed: {}",
                            msg.error_text()
                                .unwrap_or_else(|| "unknown error".to_string())
                        ));
                    }
                    return Ok(msg);
                }
            }
        };

        timeout(Duration::from_secs(timeout_secs), read_loop)
            .await
            .map_err(|_| {
                anyhow!("timed out waiting {timeout_secs}s for SN-Utils response {correlation_id}")
            })?
    }

    pub async fn send_action_and_wait_for_action(
        &mut self,
        payload: &Value,
        expected_action: &str,
        timeout_secs: u64,
    ) -> anyhow::Result<SnuMessage> {
        self.send_json(payload).await?;

        let expected_action = expected_action.to_string();
        let read_loop = async {
            loop {
                let msg = self.read_json_message().await?;
                if msg.action.as_deref() == Some(expected_action.as_str()) {
                    if msg.success == Some(false) || msg.error.is_some() {
                        return Err(anyhow!(
                            "SN-Utils action failed: {}",
                            msg.error_text()
                                .unwrap_or_else(|| "unknown error".to_string())
                        ));
                    }
                    return Ok(msg);
                }
            }
        };

        timeout(Duration::from_secs(timeout_secs), read_loop)
            .await
            .map_err(|_| {
                anyhow!("timed out waiting {timeout_secs}s for SN-Utils action {expected_action}")
            })?
    }

    pub async fn send_payload_and_wait(
        &mut self,
        payload: &Value,
        timeout_secs: u64,
    ) -> anyhow::Result<SnuMessage> {
        self.send_json(payload).await?;

        let read_loop = async { self.read_json_message().await };

        timeout(Duration::from_secs(timeout_secs), read_loop)
            .await
            .map_err(|_| anyhow!("timed out waiting {timeout_secs}s for SN-Utils response"))?
    }

    async fn send_json(&mut self, value: &Value) -> anyhow::Result<()> {
        self.socket
            .send(Message::Text(serde_json::to_string(value)?))
            .await?;
        Ok(())
    }

    async fn read_json_message(&mut self) -> anyhow::Result<SnuMessage> {
        loop {
            let Some(message) = self.socket.next().await else {
                return Err(anyhow!("SN-Utils helper tab disconnected"));
            };
            let message = message?;
            match message {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(&text).with_context(|| {
                        format!("invalid JSON from SN-Utils helper tab: {text}")
                    })?;
                    if value.is_array() {
                        tracing::debug!(%text, "ignoring SN-Utils informational array message");
                        continue;
                    }
                    return SnuMessage::from_value(value);
                }
                Message::Binary(bytes) => {
                    let value: Value = serde_json::from_slice(&bytes)
                        .context("invalid binary JSON from SN-Utils helper tab")?;
                    return SnuMessage::from_value(value);
                }
                Message::Ping(bytes) => {
                    self.socket.send(Message::Pong(bytes)).await?;
                }
                Message::Pong(_) => {}
                Message::Close(frame) => {
                    return Err(anyhow!("SN-Utils helper tab closed connection: {frame:?}"));
                }
                Message::Frame(_) => {}
            }
        }
    }
}
