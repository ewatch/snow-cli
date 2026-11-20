//! Host-driven command review (SN-Utils 10.3 Review Queue).
//!
//! When a security gate is set to `approve`, the bridge must hold the action
//! until the user approves it in the helper tab. SN-Utils itself holds only
//! background scripts; every other gated action needs the bridge to run the
//! two-phase protocol SN-Utils' own bridge uses (`@snutils/snu` 0.6.6):
//!
//! 1. `reviewRequest` with a fresh review id, nonce, and payload hash;
//! 2. the helper answers `reviewResponse` (approved or rejected);
//! 3. on approval the bridge sends `executeApproved`; the helper either runs
//!    the stored command itself (UI actions) or answers
//!    `approvedNotExecuted`, after which the bridge dispatches the action.
//!
//! A helper that does not advertise `commandReview` cannot hold actions, so
//! approve-mode actions fail closed instead of running unreviewed.

use ring::digest::{SHA256, digest};
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::{Value, json};

use super::bridge::{BridgeError, BridgeManager, Matcher};
use super::gates::{GateDecision, GateName, PreflightResult, gates_for_payload};
use super::protocol::SnuMessage;

/// Outcome of the review step for one action.
#[derive(Debug)]
pub enum ReviewOutcome {
    /// No review needed (not approve mode, or the helper reviews it itself).
    NotRequired,
    /// Approved; the caller dispatches the original action now.
    Dispatch,
    /// Approved and already run by the helper; this is its reply.
    Executed(Box<SnuMessage>),
}

/// Whether this bridge must run a review: approve mode, except for
/// background scripts, which SN-Utils holds in the helper tab itself.
pub fn needs_host_review(payload: &Value, preflight: &PreflightResult) -> bool {
    preflight.decision == GateDecision::ApprovalRequired
        && gates_for_payload(payload) != [GateName::BackgroundScripts]
}

/// Hold an approve-mode action for review in the helper tab.
pub async fn review_if_required(
    manager: &BridgeManager,
    payload: &Value,
    preflight: &PreflightResult,
    timeout_secs: u64,
) -> Result<ReviewOutcome, BridgeError> {
    if !needs_host_review(payload, preflight) {
        return Ok(ReviewOutcome::NotRequired);
    }
    let gate = preflight
        .gate
        .map_or_else(|| "unknown".to_owned(), |gate| gate.to_string());
    let origin = preflight
        .origin
        .clone()
        .unwrap_or_else(|| "unknown instance".to_owned());
    if !manager.helper_status().await.supports_command_review() {
        return Err(BridgeError::ReviewUnsupported { gate, origin });
    }

    let correlation_id = payload
        .get("agentRequestId")
        .and_then(Value::as_str)
        .map_or_else(|| format!("snow-cli-{}", random_hex(8)), str::to_owned);
    let review_id = format!("rev_snow_cli_{}", random_hex(8));
    let nonce = random_hex(16);
    let command = review_command(payload);
    let params = review_params(payload);
    let payload_hash = payload_hash(&origin, &command, &params);
    // The helper runs UI actions from the parameters on the card, so those
    // stay exact; other cards may shorten large values for display.
    let card_params = if command == "run_ui_action" {
        params.clone()
    } else {
        summarize_large_values(&params)
    };
    let review_correlation = format!("{correlation_id}-review");

    let request = json!({
        "action": "reviewRequest",
        "reviewId": review_id,
        "nonce": nonce,
        "payloadHash": payload_hash,
        "command": command,
        "params": card_params,
        "instanceOrigin": origin,
        "instance": review_instance(payload),
        "client": {
            "name": "snow-cli",
            "version": env!("CARGO_PKG_VERSION"),
            "hostKind": "standalone",
            "pid": std::process::id(),
        },
        "expiresIn": timeout_secs,
        "agentRequestId": review_correlation,
    });
    let response = manager
        .request_on_generation(
            &request,
            Matcher::Correlation(review_correlation),
            timeout_secs,
            preflight.generation,
        )
        .await;
    let response = match response {
        Ok(response) => response,
        Err(BridgeError::Timeout { .. }) => {
            cancel(manager, &review_id).await;
            return Err(BridgeError::ApprovalTimeout { gate, origin });
        }
        Err(error) => {
            cancel(manager, &review_id).await;
            return Err(error);
        }
    };

    let field = |name: &str| response.extra.get(name).and_then(Value::as_str);
    if field("reviewId") != Some(review_id.as_str())
        || field("nonce") != Some(nonce.as_str())
        || field("payloadHash") != Some(payload_hash.as_str())
    {
        cancel(manager, &review_id).await;
        return Err(BridgeError::ReviewMismatch);
    }
    if response.extra.get("approved").and_then(Value::as_bool) != Some(true) {
        return Err(BridgeError::ReviewRejected {
            gate,
            origin,
            feedback: field("userFeedback")
                .map(|text| text.chars().take(1000).collect::<String>())
                .filter(|text| !text.trim().is_empty()),
        });
    }

    // The gate may have changed while the card was open: re-check it on the
    // same helper generation before anything runs.
    let recheck = manager.preflight(payload, timeout_secs).await?;
    if recheck.generation != preflight.generation {
        cancel(manager, &review_id).await;
        return Err(BridgeError::Disconnected);
    }
    if recheck.decision == GateDecision::Blocked {
        cancel(manager, &review_id).await;
        return Err(BridgeError::GateBlocked {
            gate: recheck.gate.map_or(gate, |gate| gate.to_string()),
            origin,
        });
    }

    let execute = json!({
        "action": "executeApproved",
        "reviewId": review_id,
        "nonce": nonce,
        "payloadHash": payload_hash,
        "agentRequestId": correlation_id,
    });
    let executed = match manager
        .request_on_generation(
            &execute,
            Matcher::Correlation(correlation_id),
            timeout_secs,
            preflight.generation,
        )
        .await
    {
        Ok(executed) => executed,
        // Approved and handed to the helper: it may have run.
        Err(BridgeError::Timeout { .. }) => {
            return Err(BridgeError::ApprovedWithoutReply { gate, origin });
        }
        Err(error) => return Err(error),
    };
    if executed
        .extra
        .get("approvedNotExecuted")
        .and_then(Value::as_bool)
        == Some(true)
    {
        Ok(ReviewOutcome::Dispatch)
    } else {
        Ok(ReviewOutcome::Executed(Box::new(executed)))
    }
}

/// Best effort: remove a review card that will never be answered.
async fn cancel(manager: &BridgeManager, review_id: &str) {
    manager
        .send_untracked(&json!({
            "action": "cancelReview",
            "reviewId": review_id,
            "reason": "COMPLETED_OR_TIMED_OUT",
        }))
        .await;
}

/// The command name SN-Utils' review card and `executeApproved` expect.
fn review_command(payload: &Value) -> String {
    match payload
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "agentRestApi" => "rest_request".to_owned(),
        "agentRunUiAction" => "run_ui_action".to_owned(),
        "createRecord" => "create_record".to_owned(),
        "uploadAttachment" => "upload_attachment".to_owned(),
        "takeScreenshot" => "take_screenshot".to_owned(),
        other => other.to_owned(),
    }
}

/// The action's parameters as shown on the card, without the routing fields
/// and without the session (`instance` carries the `g_ck` token).
fn review_params(payload: &Value) -> Value {
    let mut params = payload.as_object().cloned().unwrap_or_default();
    for key in ["action", "agentRequestId", "instance"] {
        params.remove(key);
    }
    Value::Object(params)
}

/// Longest string value shown in full on a review card.
const CARD_VALUE_LIMIT: usize = 2048;

/// Replace long string values (for example base64 attachment content) with
/// their size and digest, so the card stays readable.
fn summarize_large_values(params: &Value) -> Value {
    match params {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), summarize_large_values(value)))
                .collect(),
        ),
        Value::String(text) if text.len() > CARD_VALUE_LIMIT => Value::String(format!(
            "<{} characters, sha256 {}>",
            text.len(),
            &hex(digest(&SHA256, text.as_bytes()).as_ref())[..16]
        )),
        other => other.clone(),
    }
}

/// Instance name and URL only; never the session token.
fn review_instance(payload: &Value) -> Value {
    let instance = payload.get("instance");
    json!({
        "name": instance.and_then(|i| i.get("name")).cloned().unwrap_or(Value::Null),
        "url": instance.and_then(|i| i.get("url")).cloned().unwrap_or(Value::Null),
    })
}

/// Binds the approval to exactly this origin, command, and parameters.
fn payload_hash(origin: &str, command: &str, params: &Value) -> String {
    let canonical = json!([1, origin, command, params]).to_string();
    hex(digest(&SHA256, canonical.as_bytes()).as_ref())
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    // A failing system RNG leaves zeros; the nonce check still binds the
    // reply to this request, and the id stays unique per correlation.
    let _ = SystemRandom::new().fill(&mut buffer);
    hex(&buffer)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_never_carry_the_session_token() {
        let payload = json!({
            "action": "agentRestApi", "agentRequestId": "r1", "method": "DELETE",
            "endpoint": "/api/now/table/incident/x",
            "instance": {"name": "dev", "url": "https://dev.service-now.com", "g_ck": "secret"}
        });
        let params = review_params(&payload);
        assert_eq!(
            params,
            json!({"method": "DELETE", "endpoint": "/api/now/table/incident/x"})
        );
        let instance = review_instance(&payload);
        assert!(!instance.to_string().contains("secret"));
        assert_eq!(review_command(&payload), "rest_request");
    }

    #[test]
    fn large_values_are_summarized_for_the_card() {
        let params = json!({"file_name": "a.png", "content": "A".repeat(5000), "nested": {"x": "B".repeat(3000)}});
        let card = summarize_large_values(&params);
        assert_eq!(card["file_name"], "a.png");
        assert!(
            card["content"]
                .as_str()
                .unwrap()
                .starts_with("<5000 characters, sha256 ")
        );
        assert!(
            card["nested"]["x"]
                .as_str()
                .unwrap()
                .starts_with("<3000 characters")
        );
    }

    #[test]
    fn payload_hash_binds_origin_command_and_params() {
        let params = json!({"method": "DELETE"});
        let base = payload_hash("https://a:443", "rest_request", &params);
        assert_eq!(base.len(), 64);
        assert_ne!(base, payload_hash("https://b:443", "rest_request", &params));
        assert_ne!(
            base,
            payload_hash("https://a:443", "run_ui_action", &params)
        );
        assert_ne!(
            base,
            payload_hash("https://a:443", "rest_request", &json!({"method": "GET"}))
        );
    }
}
