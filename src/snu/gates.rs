use std::collections::HashMap;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::snu::protocol::normalize_origin;

/// Per-instance security switches advertised by current SN-Utils helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GateMode {
    Off,
    Approve,
    Auto,
}

/// The security gate selected for an SN-Utils protocol action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GateName {
    BackgroundScripts,
    DeleteRecords,
    CreateArtifacts,
    RestRequest,
    BrowserDebugger,
    /// Changing fields on existing records (SN-Utils 10.2+).
    UpdateRecords,
}

impl std::fmt::Display for GateName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::BackgroundScripts => "backgroundScripts",
            Self::DeleteRecords => "deleteRecords",
            Self::CreateArtifacts => "createArtifacts",
            Self::RestRequest => "restRequest",
            Self::BrowserDebugger => "browserDebugger",
            Self::UpdateRecords => "updateRecords",
        };
        formatter.write_str(name)
    }
}

/// Complete normalized gate state for one authorized instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGates {
    pub background_scripts: GateMode,
    pub delete_records: GateMode,
    pub create_artifacts: GateMode,
    pub rest_request: GateMode,
    pub browser_debugger: GateMode,
    /// Absent on helpers that predate the gate; it then follows
    /// `createArtifacts`, as SN-Utils' own bridge does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_records: Option<GateMode>,
}

impl InstanceGates {
    pub fn mode(&self, gate: GateName) -> GateMode {
        match gate {
            GateName::UpdateRecords => self.update_records.unwrap_or(self.create_artifacts),
            GateName::BackgroundScripts => self.background_scripts,
            GateName::DeleteRecords => self.delete_records,
            GateName::CreateArtifacts => self.create_artifacts,
            GateName::RestRequest => self.rest_request,
            GateName::BrowserDebugger => self.browser_debugger,
        }
    }
}

/// Revisioned gate state. Revisions are comparable only within one helper
/// WebSocket generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateSnapshot {
    pub revision: u64,
    pub gates: InstanceGates,
}

/// Protocol capability versions reported by the helper.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperProtocolCapabilities {
    #[serde(default)]
    pub protocol_version: Option<u64>,
    #[serde(default)]
    pub command_review: Option<u64>,
    #[serde(default)]
    pub rejection_feedback: Option<u64>,
    #[serde(default)]
    pub instance_security_gates: Option<u64>,
}

/// Redactable helper build metadata pushed immediately after connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperBuildInfo {
    pub extension_name: String,
    pub extension_version: String,
    pub debugger_available: bool,
    #[serde(default)]
    pub capabilities: HelperProtocolCapabilities,
}

/// Non-sensitive license/tier metadata pushed by current helper builds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperLicenseInfo {
    pub tier: String,
    pub pro_features: bool,
}

/// Negotiation state for the active helper generation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelperSecurityGateSupport {
    #[default]
    Unknown,
    LegacyUnrestricted,
    Gated,
}

/// Redacted helper state for the active WebSocket generation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperStatus {
    pub generation: u64,
    pub security_gate_support: HelperSecurityGateSupport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<HelperBuildInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<HelperLicenseInfo>,
    #[serde(default)]
    pub instance_gates: HashMap<String, GateSnapshot>,
    /// Capabilities from the latest active `agentGetCapabilities` refresh.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negotiated_capabilities: Option<HelperProtocolCapabilities>,
}

impl HelperStatus {
    /// Whether the helper can hold commands for host-driven review.
    pub fn supports_command_review(&self) -> bool {
        let advertised = |caps: Option<&HelperProtocolCapabilities>| {
            caps.and_then(|caps| caps.command_review).unwrap_or(0) >= 1
        };
        advertised(self.build.as_ref().map(|build| &build.capabilities))
            || advertised(self.negotiated_capabilities.as_ref())
    }

    pub fn advertises_instance_gates(&self) -> bool {
        self.build
            .as_ref()
            .and_then(|build| build.capabilities.instance_security_gates)
            .unwrap_or(0)
            > 0
    }

    pub fn apply_gate_snapshot(&mut self, origin: String, snapshot: GateSnapshot) {
        let should_replace = self
            .instance_gates
            .get(&origin)
            .is_none_or(|current| snapshot.revision > current.revision);
        if should_replace {
            self.instance_gates.insert(origin, snapshot);
        }
    }
}

/// Compatibility result of actively negotiating with one helper generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityNegotiation {
    Legacy { generation: u64 },
    Gated(Box<HelperStatus>),
}

/// Secret-free decision made immediately before dispatching an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateDecision {
    NotGated,
    LegacyUnrestricted,
    Blocked,
    ApprovalRequired,
    Automatic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreflightResult {
    /// Helper WebSocket generation whose negotiated state produced this
    /// decision. Gated and legacy decisions must dispatch on this generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<GateName>,
    pub decision: GateDecision,
}

/// Active capability response, with instance origins canonicalized to the same
/// explicit-port representation used by broker sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperCapabilitiesReport {
    pub protocol: HelperProtocolCapabilities,
    pub instance_gates: HashMap<String, GateSnapshot>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCapabilitiesReport {
    #[serde(default)]
    capabilities: HelperProtocolCapabilities,
    #[serde(default)]
    instance_gates: HashMap<String, GateSnapshot>,
}

/// Classify an outbound helper action before it reaches the browser socket,
/// following SN-Utils' own bridge (`@snutils/snu` 0.6.6 `getCommandPolicy`).
/// Every returned gate must allow the action; an empty list means ungated.
pub fn gates_for_payload(payload: &Value) -> Vec<GateName> {
    let Some(action) = payload.get("action").and_then(Value::as_str) else {
        return Vec::new();
    };
    match action {
        "executeBackgroundScript" | "agentRunBackgroundScript" => vec![GateName::BackgroundScripts],
        "createRecord" | "uploadAttachment" => vec![GateName::CreateArtifacts],
        // Running a UI action commits the open form, so it updates an existing
        // record; a name mentioning delete/destroy only ever escalates.
        "agentRunUiAction" => {
            // The helper reads `params` when present, else the payload, and
            // takes `uiAction` or `actionName`. Any delete/destroy name in
            // either place only ever escalates.
            let deletes = [
                "/uiAction",
                "/actionName",
                "/params/uiAction",
                "/params/actionName",
            ]
            .iter()
            .filter_map(|pointer| payload.pointer(pointer).and_then(Value::as_str))
            .map(str::to_ascii_lowercase)
            .any(|name| name.contains("delete") || name.contains("destroy"));
            if deletes {
                vec![GateName::DeleteRecords]
            } else {
                vec![GateName::UpdateRecords]
            }
        }
        // A screenshot is a read; only debugger-based capture needs the gate.
        "takeScreenshot" => {
            let uses_debugger = ["cdp", "allowDebugger"]
                .iter()
                .any(|key| payload.get(*key).and_then(Value::as_bool) == Some(true));
            if uses_debugger {
                vec![GateName::BrowserDebugger]
            } else {
                Vec::new()
            }
        }
        "agentRestApi" => rest_gates(payload),
        // Winding a debugger session down stays possible after the gate is
        // switched off mid-capture, as in SN-Utils' own bridge.
        "agentCdpStopConsoleCapture"
        | "agentCdpStopNetworkCapture"
        | "agentCdpClearDialogHandler"
        | "agentCdpDetach" => Vec::new(),
        action if action.starts_with("agentCdp") => vec![GateName::BrowserDebugger],
        _ => Vec::new(),
    }
}

fn rest_gates(payload: &Value) -> Vec<GateName> {
    let method = payload
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("GET")
        .to_ascii_uppercase();
    match method.as_str() {
        "POST" | "PUT" | "PATCH" => match batch_sub_methods(payload) {
            // Not a Batch API call: a plain write.
            BatchMethods::NotBatch => vec![GateName::RestRequest],
            BatchMethods::Methods(methods)
                if methods
                    .iter()
                    .all(|m| matches!(m.as_str(), "GET" | "POST" | "PUT" | "PATCH")) =>
            {
                vec![GateName::RestRequest]
            }
            // A DELETE, an unreadable method, or an unreadable body.
            BatchMethods::Methods(_) | BatchMethods::Unreadable => {
                vec![GateName::DeleteRecords, GateName::RestRequest]
            }
        },
        "DELETE" => vec![GateName::DeleteRecords],
        "GET" | "HEAD" | "OPTIONS" => Vec::new(),
        _ => vec![GateName::RestRequest],
    }
}

enum BatchMethods {
    NotBatch,
    Methods(Vec<String>),
    Unreadable,
}

/// The sub-request methods of a Batch API call (`/api/now/v1/batch`). The
/// endpoint is normalized like a browser URL first, so dot segments or
/// encoded dots cannot hide it.
fn batch_sub_methods(payload: &Value) -> BatchMethods {
    let endpoint = payload
        .get("endpoint")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Ok(base) = url::Url::parse("https://instance.invalid") else {
        return BatchMethods::Unreadable;
    };
    let Ok(url) = base.join(endpoint) else {
        return BatchMethods::Unreadable;
    };
    let mut path = url.path().to_ascii_lowercase();
    while path.contains("//") {
        path = path.replace("//", "/");
    }
    let path = path.strip_suffix('/').unwrap_or(&path);
    let is_batch = path
        .strip_prefix("/api/now/v")
        .and_then(|rest| rest.split_once("/batch"))
        .is_some_and(|(version, tail)| {
            !version.is_empty() && version.chars().all(|c| c.is_ascii_digit()) && tail.is_empty()
        });
    if !is_batch {
        return BatchMethods::NotBatch;
    }
    let body = match payload.get("body") {
        Some(Value::String(text)) => match serde_json::from_str::<Value>(text) {
            Ok(body) => body,
            Err(_) => return BatchMethods::Unreadable,
        },
        Some(body) => body.clone(),
        None => return BatchMethods::Unreadable,
    };
    match body.get("rest_requests").and_then(Value::as_array) {
        Some(entries) => BatchMethods::Methods(
            entries
                .iter()
                .map(|entry| {
                    entry
                        .get("method")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_ascii_uppercase()
                })
                .collect(),
        ),
        None => BatchMethods::Unreadable,
    }
}

/// Resolve the target origin without retaining any session or action payload.
pub fn payload_origin(payload: &Value) -> Option<String> {
    let page_action = matches!(
        payload.get("action").and_then(Value::as_str),
        Some(
            "agentRunUiAction"
                | "agentSetField"
                | "agentClickElement"
                | "agentGetFormState"
                | "agentNavigate"
                | "takeScreenshot"
        )
    );
    // Page actions act on the tab their URL selects, never on `instance`;
    // like the helper, `params.url` wins over the top-level `url`.
    let url = if page_action {
        payload
            .pointer("/params/url")
            .or_else(|| payload.get("url"))
    } else {
        payload
            .pointer("/instance/url")
            .or_else(|| payload.get("url"))
    };
    url.and_then(Value::as_str).and_then(normalize_origin)
}

impl HelperCapabilitiesReport {
    pub fn from_value(value: Value) -> anyhow::Result<Self> {
        let raw: RawCapabilitiesReport =
            serde_json::from_value(value).context("invalid SN-Utils helper capability response")?;
        let instance_gates = raw
            .instance_gates
            .into_iter()
            .filter_map(|(origin, snapshot)| {
                normalize_origin(&origin).map(|canonical| (canonical, snapshot))
            })
            .collect();
        Ok(Self {
            protocol: raw.capabilities,
            instance_gates,
        })
    }

    pub fn supports_instance_gates(&self) -> bool {
        self.protocol.instance_security_gates.unwrap_or(0) > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_protocol_actions_at_the_dispatch_seam() {
        use GateName::*;
        let batch = |requests: serde_json::Value| {
            serde_json::json!({"action": "agentRestApi", "method": "POST",
                "endpoint": "/api/now/v1/batch", "body": {"rest_requests": requests}})
        };
        let cases = [
            (serde_json::json!({"action": "agentQueryRecords"}), vec![]),
            (
                serde_json::json!({"action": "requestTableStructure"}),
                vec![],
            ),
            (serde_json::json!({"action": "requestAppMeta"}), vec![]),
            (serde_json::json!({"action": "runSlashCommand"}), vec![]),
            (serde_json::json!({"action": "activateTab"}), vec![]),
            (serde_json::json!({"action": "switchContext"}), vec![]),
            (serde_json::json!({"action": "agentGetContext"}), vec![]),
            (
                serde_json::json!({"action": "agentGetParentOptions"}),
                vec![],
            ),
            (serde_json::json!({"action": "agentGetFormState"}), vec![]),
            (serde_json::json!({"action": "agentSetField"}), vec![]),
            (serde_json::json!({"action": "agentClickElement"}), vec![]),
            (serde_json::json!({"action": "agentNavigate"}), vec![]),
            (
                serde_json::json!({"action": "agentRunUiAction", "uiAction": "sysverb_update"}),
                vec![UpdateRecords],
            ),
            (
                serde_json::json!({"action": "agentRunUiAction", "uiAction": "sysverb_delete"}),
                vec![DeleteRecords],
            ),
            (
                serde_json::json!({"action": "agentRunUiAction", "params": {"actionName": "delete_all"}}),
                vec![DeleteRecords],
            ),
            (
                serde_json::json!({"action": "agentRunUiAction", "uiAction": "save",
                    "params": {"uiAction": "sysverb_destroy"}}),
                vec![DeleteRecords],
            ),
            (
                serde_json::json!({"action": "agentCdpStopNetworkCapture"}),
                vec![],
            ),
            (serde_json::json!({"action": "agentCdpDetach"}), vec![]),
            (
                serde_json::json!({"action": "agentCdpStartNetworkCapture"}),
                vec![BrowserDebugger],
            ),
            (
                serde_json::json!({"action": "agentCdp"}),
                vec![BrowserDebugger],
            ),
            (
                serde_json::json!({"action": "executeBackgroundScript"}),
                vec![BackgroundScripts],
            ),
            (
                serde_json::json!({"action": "agentRunBackgroundScript"}),
                vec![BackgroundScripts],
            ),
            (
                serde_json::json!({"action": "createRecord"}),
                vec![CreateArtifacts],
            ),
            (
                serde_json::json!({"action": "uploadAttachment"}),
                vec![CreateArtifacts],
            ),
            (serde_json::json!({"action": "takeScreenshot"}), vec![]),
            (
                serde_json::json!({"action": "takeScreenshot", "cdp": true}),
                vec![BrowserDebugger],
            ),
            (
                serde_json::json!({"action": "agentCdpCaptureScreenshot"}),
                vec![BrowserDebugger],
            ),
            (
                serde_json::json!({"action": "agentRestApi", "method": "GET"}),
                vec![],
            ),
            (
                serde_json::json!({"action": "agentRestApi", "method": "PATCH"}),
                vec![RestRequest],
            ),
            (
                serde_json::json!({"action": "agentRestApi", "method": "DELETE"}),
                vec![DeleteRecords],
            ),
            (
                batch(serde_json::json!([{"method": "GET"}, {"method": "patch"}])),
                vec![RestRequest],
            ),
            (
                batch(serde_json::json!([{"method": "GET"}, {"method": "DELETE"}])),
                vec![DeleteRecords, RestRequest],
            ),
            (
                batch(serde_json::json!([{}])),
                vec![DeleteRecords, RestRequest],
            ),
            (
                serde_json::json!({"action": "agentRestApi", "method": "POST",
                    "endpoint": "/api/now/v1/batch", "body": "not json"}),
                vec![DeleteRecords, RestRequest],
            ),
            (
                // Dot segments and doubled slashes cannot hide the endpoint.
                serde_json::json!({"action": "agentRestApi", "method": "POST",
                    "endpoint": "/api/now/table/../v2//Batch/",
                    "body": {"rest_requests": [{"method": "DELETE"}]}}),
                vec![DeleteRecords, RestRequest],
            ),
        ];

        for (payload, expected) in cases {
            assert_eq!(gates_for_payload(&payload), expected, "payload: {payload}");
        }
    }

    #[test]
    fn update_records_follows_create_artifacts_until_published() {
        let mut gates: InstanceGates = serde_json::from_value(serde_json::json!({
            "backgroundScripts": "approve", "deleteRecords": "approve",
            "createArtifacts": "off", "restRequest": "auto", "browserDebugger": "auto"
        }))
        .unwrap();
        assert_eq!(gates.mode(GateName::UpdateRecords), GateMode::Off);
        gates.update_records = Some(GateMode::Approve);
        assert_eq!(gates.mode(GateName::UpdateRecords), GateMode::Approve);
    }

    #[test]
    fn stale_gate_revisions_are_ignored() {
        let mut status = HelperStatus::default();
        let gates = InstanceGates {
            background_scripts: GateMode::Auto,
            delete_records: GateMode::Approve,
            create_artifacts: GateMode::Auto,
            rest_request: GateMode::Auto,
            browser_debugger: GateMode::Auto,
            update_records: None,
        };
        status.apply_gate_snapshot(
            "https://dev.service-now.com:443".into(),
            GateSnapshot {
                revision: 9,
                gates: gates.clone(),
            },
        );
        let mut stale = gates;
        stale.background_scripts = GateMode::Off;
        status.apply_gate_snapshot(
            "https://dev.service-now.com:443".into(),
            GateSnapshot {
                revision: 8,
                gates: stale,
            },
        );

        assert_eq!(
            status.instance_gates["https://dev.service-now.com:443"]
                .gates
                .background_scripts,
            GateMode::Auto
        );
    }

    #[test]
    fn capability_response_canonicalizes_javascript_origins() {
        let report = HelperCapabilitiesReport::from_value(serde_json::json!({
            "success": true,
            "capabilities": { "protocolVersion": 1, "instanceSecurityGates": 1 },
            "instanceGates": {
                "https://DEV.service-now.com": {
                    "revision": 42,
                    "gates": {
                        "backgroundScripts": "off",
                        "deleteRecords": "approve",
                        "createArtifacts": "auto",
                        "restRequest": "auto",
                        "browserDebugger": "off"
                    }
                }
            }
        }))
        .unwrap();

        assert_eq!(report.protocol.instance_security_gates, Some(1));
        let snapshot = report
            .instance_gates
            .get("https://dev.service-now.com:443")
            .unwrap();
        assert_eq!(snapshot.revision, 42);
        assert_eq!(snapshot.gates.background_scripts, GateMode::Off);
        assert_eq!(snapshot.gates.delete_records, GateMode::Approve);
        assert_eq!(snapshot.gates.create_artifacts, GateMode::Auto);
    }
}
