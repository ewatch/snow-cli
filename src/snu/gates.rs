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
}

impl std::fmt::Display for GateName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::BackgroundScripts => "backgroundScripts",
            Self::DeleteRecords => "deleteRecords",
            Self::CreateArtifacts => "createArtifacts",
            Self::RestRequest => "restRequest",
            Self::BrowserDebugger => "browserDebugger",
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
}

impl InstanceGates {
    pub fn mode(&self, gate: GateName) -> GateMode {
        match gate {
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
}

impl HelperStatus {
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
    Legacy,
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

/// Classify an outbound helper action before it reaches the browser socket.
/// Actions not covered by an extension security gate return `None`.
pub fn gate_for_payload(payload: &Value) -> Option<GateName> {
    let action = payload.get("action").and_then(Value::as_str)?;
    match action {
        "executeBackgroundScript" | "agentRunBackgroundScript" => Some(GateName::BackgroundScripts),
        "createRecord" | "uploadAttachment" => Some(GateName::CreateArtifacts),
        "takeScreenshot" => Some(GateName::BrowserDebugger),
        "agentRestApi" => match payload
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET")
            .to_ascii_uppercase()
            .as_str()
        {
            "GET" | "HEAD" | "OPTIONS" => None,
            "DELETE" => Some(GateName::DeleteRecords),
            _ => Some(GateName::RestRequest),
        },
        action if action.starts_with("agentCdp") => Some(GateName::BrowserDebugger),
        _ => None,
    }
}

/// Resolve the target origin without retaining any session or action payload.
pub fn payload_origin(payload: &Value) -> Option<String> {
    payload
        .pointer("/instance/url")
        .or_else(|| payload.get("url"))
        .and_then(Value::as_str)
        .and_then(normalize_origin)
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
        let cases = [
            (serde_json::json!({"action": "agentQueryRecords"}), None),
            (serde_json::json!({"action": "requestTableStructure"}), None),
            (serde_json::json!({"action": "requestAppMeta"}), None),
            (serde_json::json!({"action": "runSlashCommand"}), None),
            (serde_json::json!({"action": "activateTab"}), None),
            (serde_json::json!({"action": "switchContext"}), None),
            (serde_json::json!({"action": "agentGetContext"}), None),
            (serde_json::json!({"action": "agentGetParentOptions"}), None),
            (serde_json::json!({"action": "agentGetFormState"}), None),
            (serde_json::json!({"action": "agentSetField"}), None),
            (serde_json::json!({"action": "agentRunUiAction"}), None),
            (serde_json::json!({"action": "agentClickElement"}), None),
            (serde_json::json!({"action": "agentNavigate"}), None),
            (
                serde_json::json!({"action": "executeBackgroundScript"}),
                Some(GateName::BackgroundScripts),
            ),
            (
                serde_json::json!({"action": "agentRunBackgroundScript"}),
                Some(GateName::BackgroundScripts),
            ),
            (
                serde_json::json!({"action": "createRecord"}),
                Some(GateName::CreateArtifacts),
            ),
            (
                serde_json::json!({"action": "uploadAttachment"}),
                Some(GateName::CreateArtifacts),
            ),
            (
                serde_json::json!({"action": "takeScreenshot"}),
                Some(GateName::BrowserDebugger),
            ),
            (
                serde_json::json!({"action": "agentCdpCaptureScreenshot"}),
                Some(GateName::BrowserDebugger),
            ),
            (
                serde_json::json!({"action": "agentRestApi", "method": "GET"}),
                None,
            ),
            (
                serde_json::json!({"action": "agentRestApi", "method": "PATCH"}),
                Some(GateName::RestRequest),
            ),
            (
                serde_json::json!({"action": "agentRestApi", "method": "DELETE"}),
                Some(GateName::DeleteRecords),
            ),
        ];

        for (payload, expected) in cases {
            assert_eq!(gate_for_payload(&payload), expected, "payload: {payload}");
        }
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
