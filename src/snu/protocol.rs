use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Browser-session metadata sent by SN-Utils when the user runs `/token`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnuInstance {
    pub name: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub g_ck: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// Loosely-typed SN-Utils WebSocket message. SN-Utils actions are not a stable
/// public schema, so keep unknown fields rather than throwing them away.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnuMessage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,

    #[serde(
        rename = "agentRequestId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub agent_request_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<SnuInstance>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl SnuMessage {
    pub fn from_value(value: Value) -> anyhow::Result<Self> {
        Ok(serde_json::from_value(value)?)
    }

    pub fn is_response_for(&self, correlation_id: &str) -> bool {
        self.agent_request_id.as_deref() == Some(correlation_id)
    }

    pub fn error_text(&self) -> Option<String> {
        match &self.error {
            Some(Value::String(text)) => Some(text.clone()),
            Some(Value::Object(map)) => map
                .get("message")
                .or_else(|| map.get("detail"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| Some(Value::Object(map.clone()).to_string())),
            Some(other) => Some(other.to_string()),
            None => self
                .extra
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string),
        }
    }
}

/// Normalize an instance URL to a stable origin key (`scheme://host:port`) so
/// the broker can map a `g_ck` to the instance it belongs to regardless of path
/// or trailing slash. Returns `None` for inputs that are not a parseable URL
/// with a host.
pub fn normalize_origin(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let port = parsed.port_or_known_default()?;
    Some(format!("{}://{}:{}", parsed.scheme(), host, port))
}

/// Resolve a user-supplied `--instance` value (a full URL or a bare host) to a
/// normalized origin. Bare hosts are assumed to be `https://`.
pub fn resolve_origin(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(origin) = normalize_origin(trimmed) {
        return Some(origin);
    }
    normalize_origin(&format!("https://{trimmed}"))
}

pub fn redact_session_for_output(instance: &SnuInstance) -> Value {
    serde_json::json!({
        "name": instance.name,
        "url": instance.url,
        "has_g_ck": instance.g_ck.as_deref().is_some_and(|token| !token.is_empty()),
        "scope": instance.scope,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_token_message() {
        let msg = SnuMessage::from_value(serde_json::json!({
            "instance": { "name": "dev", "url": "https://dev.service-now.com", "g_ck": "secret" }
        }))
        .unwrap();
        assert_eq!(msg.instance.unwrap().name, "dev");
    }

    #[test]
    fn redacts_g_ck_from_output() {
        let out = redact_session_for_output(&SnuInstance {
            name: "dev".into(),
            url: "https://dev.service-now.com".into(),
            g_ck: Some("secret".into()),
            scope: None,
        });
        assert_eq!(out["has_g_ck"], true);
        assert!(out.to_string().contains("has_g_ck"));
        assert!(!out.to_string().contains("secret"));
    }

    #[test]
    fn normalize_origin_ignores_path_and_trailing_slash() {
        let a = normalize_origin("https://dev123.service-now.com/nav_to.do?uri=x").unwrap();
        let b = normalize_origin("https://dev123.service-now.com/").unwrap();
        assert_eq!(a, b);
        assert_eq!(a, "https://dev123.service-now.com:443");
    }

    #[test]
    fn resolve_origin_accepts_url_and_bare_host() {
        let from_url = resolve_origin("https://dev123.service-now.com/incident.do").unwrap();
        let from_host = resolve_origin("dev123.service-now.com").unwrap();
        assert_eq!(from_url, from_host);
    }
}
