use super::*;

pub(super) async fn connect_bridge(
    timeout_secs: u64,
    banner: Option<&str>,
) -> anyhow::Result<BrokerBridge> {
    let bridge = BrokerBridge::connect_or_spawn().await?;
    tracing::debug!("SN-Utils broker connected");

    if let Some(message) = banner {
        let _ = bridge.send_banner(message, timeout_secs).await;
    }

    Ok(bridge)
}

pub(super) async fn connect_and_wait_for_session(
    timeout_secs: u64,
    target_origin: Option<String>,
) -> anyhow::Result<(BrokerBridge, SnuInstance)> {
    let bridge = connect_bridge(
        timeout_secs,
        Some("snow-cli SN-Utils bridge connected. Run /token in a ServiceNow tab if the helper has not sent the browser session yet."),
    )
    .await?;
    let instance = bridge
        .wait_for_session(timeout_secs, false, target_origin)
        .await?;
    Ok((bridge, instance))
}

pub(super) async fn connect_and_wait_for_fresh_session(
    timeout_secs: u64,
    target_origin: Option<String>,
) -> anyhow::Result<(BrokerBridge, SnuInstance)> {
    let bridge = connect_bridge(
        timeout_secs,
        Some("snow-cli SN-Utils bridge connected. Run /token in a ServiceNow tab to refresh the browser session metadata."),
    )
    .await?;
    let instance = bridge
        .wait_for_session(timeout_secs, true, target_origin)
        .await?;
    Ok((bridge, instance))
}

/// Report bridge/browser connectivity without hanging on the legacy
/// `{"command":"check_connection"}` payload, which the current SN-Utils
/// ScriptSync helper never answers. Instead we (1) ensure the broker is running,
/// (2) probe the helper tab with a live banner round-trip over the WebSocket —
/// which requires no `/token` and proves the tab is responsive — and (3) fold in
/// the broker's own session bookkeeping.
pub(super) async fn handle_check_connection(
    timeout_secs: u64,
    verify: bool,
    target_origin: Option<String>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let bridge = BrokerBridge::connect_or_spawn().await?;
    // A successful banner send means the helper tab is connected and the socket
    // accepts writes. Failure (e.g. no helper tab within the connect timeout) is
    // reported as `connected: false` rather than propagated, so `check-connection`
    // always returns a useful snapshot instead of erroring out.
    let browser_responsive = bridge
        .send_banner(
            "snow-cli check-connection: SN-Utils bridge is responsive.",
            timeout_secs,
        )
        .await
        .is_ok();
    // `--verify` proves (or disproves) the cached g_ck against ServiceNow with a
    // cheap probe query. Verification failure is reported in the snapshot, not
    // propagated, so the connectivity half of the output always arrives.
    let verification = if verify {
        Some(bridge.verify_session(timeout_secs, target_origin).await)
    } else {
        None
    };
    let status = crate::snu::broker::broker_status().await?;
    print_output(
        &build_check_connection_result(&status, browser_responsive, verification),
        output_format,
    )
}

/// Report instance metadata from the broker's session state (URL, origin,
/// captured `g_ck` presence, and scope) instead of the legacy
/// `{"command":"get_instance_info"}` payload the current helper never answers.
/// `connect_or_spawn` restarts the broker (reloading any persisted sessions) if
/// it is not already running.
pub(super) async fn handle_get_instance_info(
    target_origin: Option<String>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let _bridge = BrokerBridge::connect_or_spawn().await?;
    let status = crate::snu::broker::broker_status().await?;
    let value = build_instance_info_result(&status, target_origin.as_deref())?;
    print_output(&value, output_format)
}

pub(super) fn build_check_connection_result(
    status: &BrokerStatus,
    browser_responsive: bool,
    verification: Option<anyhow::Result<bool>>,
) -> Value {
    let mut result = json!({
        "connected": browser_responsive,
        "broker_running": true,
        "broker_version": status.version,
        "ipc_addr": status.ipc_addr,
        "browser_connected": status.browser_connected,
        "session_count": status.session_count,
        "latest_instance_url": status.latest_instance_url,
        "instances": status.instances,
    });
    if let (Some(verification), Some(object)) = (verification, result.as_object_mut()) {
        match verification {
            Ok(valid) => {
                object.insert("token_valid".to_string(), Value::Bool(valid));
                if !valid {
                    object.insert(
                        "hint".to_string(),
                        Value::String(
                            "ServiceNow rejected the cached session token. Run /token in a ServiceNow tab to refresh it.".to_string(),
                        ),
                    );
                }
            }
            Err(error) => {
                object.insert("token_valid".to_string(), Value::Null);
                object.insert("verify_error".to_string(), Value::String(error.to_string()));
            }
        }
    }
    result
}

pub(super) fn build_instance_info_result(
    status: &BrokerStatus,
    target_origin: Option<&str>,
) -> anyhow::Result<Value> {
    let instance = match target_origin {
        Some(origin) => status
            .instances
            .iter()
            .find(|instance| instance.origin == origin)
            .ok_or_else(|| {
                anyhow!(
                    "no SN-Utils browser session for {origin}. Run /token in a ServiceNow tab for that instance first."
                )
            })?,
        None => status
            .instances
            .iter()
            .find(|instance| instance.is_latest)
            .or_else(|| status.instances.first())
            .ok_or_else(|| {
                anyhow!("no SN-Utils browser session yet. Run /token in a ServiceNow tab first.")
            })?,
    };
    Ok(json!({
        "url": instance.url,
        "origin": instance.origin,
        "has_g_ck": instance.has_g_ck,
        "scope": instance.scope,
        "is_latest": instance.is_latest,
        "browser_connected": status.browser_connected,
        "captured_at": instance.captured_at,
        "last_verified_at": instance.last_verified_at,
    }))
}
