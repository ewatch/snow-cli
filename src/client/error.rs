use serde::Serialize;

/// Structured API error mapped from ServiceNow HTTP responses.
#[derive(Debug, Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub status: u16,
    pub detail: Option<String>,
    pub instance: String,
}

impl ApiError {
    /// Maximum Unicode scalar values kept from a ServiceNow error message or detail.
    const MAX_ENVELOPE_CHARS: usize = 500;

    /// Build an error from an HTTP status and the (optional) response body.
    ///
    /// When the body is the standard ServiceNow error envelope
    /// (`{"error":{"message":..,"detail":..},"status":"failure"}`), its message
    /// and detail are surfaced after secret scrubbing and length bounding, and
    /// an unambiguous message refines the code (e.g. `INVALID_TABLE`). Any
    /// other body is redacted, since it may echo request data.
    pub fn from_status(status: u16, instance: &str, body: Option<String>) -> Self {
        let (code, message) = match status {
            400 => ("BAD_REQUEST", "Invalid request parameters"),
            401 => ("UNAUTHORIZED", "Authentication failed"),
            403 => ("FORBIDDEN", "Insufficient permissions"),
            404 => ("NOT_FOUND", "Resource not found"),
            429 => ("RATE_LIMITED", "Too many requests"),
            _ if status >= 500 => ("SERVER_ERROR", "ServiceNow internal error"),
            _ => ("UNKNOWN_ERROR", "Unexpected error"),
        };

        if let Some(envelope) = body.as_deref().and_then(ServiceNowErrorEnvelope::parse) {
            let message = bound_chars(&scrub_secrets(&envelope.message), Self::MAX_ENVELOPE_CHARS);
            let detail = envelope
                .detail
                .map(|detail| bound_chars(&scrub_secrets(&detail), Self::MAX_ENVELOPE_CHARS));
            let code = refine_code(status, &message, detail.as_deref()).unwrap_or(code);
            return Self {
                code: code.to_string(),
                message,
                status,
                detail,
                instance: instance.to_string(),
            };
        }

        Self {
            code: code.to_string(),
            message: message.to_string(),
            status,
            detail: body.map(|body| safe_error_detail(&body)),
            instance: instance.to_string(),
        }
    }
}

/// The `error` object of a standard ServiceNow REST failure response.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ServiceNowErrorEnvelope {
    pub(crate) message: String,
    pub(crate) detail: Option<String>,
}

impl ServiceNowErrorEnvelope {
    /// Parse `{"error":{"message":"..","detail":".."|null}}`. Returns `None`
    /// for non-JSON bodies, other JSON shapes, or an empty message.
    pub(crate) fn parse(body: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(body).ok()?;
        let error = value.get("error")?;
        let message = error.get("message")?.as_str()?.trim();
        if message.is_empty() {
            return None;
        }
        let detail = error
            .get("detail")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|detail| !detail.is_empty() && *detail != message)
            .map(ToOwned::to_owned);
        Some(Self {
            message: message.to_string(),
            detail,
        })
    }
}

/// Derive a more specific error code when the ServiceNow message is unambiguous.
fn refine_code(status: u16, message: &str, detail: Option<&str>) -> Option<&'static str> {
    let lower = message.to_ascii_lowercase();
    let detail_lower = detail.unwrap_or_default().to_ascii_lowercase();
    if lower.starts_with("invalid table") {
        Some("INVALID_TABLE")
    } else if status == 404 && lower.contains("no record found") {
        Some("RECORD_NOT_FOUND")
    } else if status == 403 && (lower.contains("acl") || detail_lower.contains("acl")) {
        Some("ACL_DENIED")
    } else {
        None
    }
}

/// Key fragments whose `key=value` / `key: value` values are always redacted.
const SENSITIVE_KEY_FRAGMENTS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "cookie",
    "authorization",
    "sysparm_ck",
    "g_ck",
    "jsessionid",
    "api_key",
    "apikey",
    "bearer",
];

/// Redact credential-like values from free text taken from a server response.
///
/// Values following a sensitive key (`token=..`, `"password": ".."`,
/// `Authorization: Bearer ..`) are replaced with `<redacted>`, and control
/// characters (including ANSI escapes) are replaced with spaces.
pub(crate) fn scrub_secrets(text: &str) -> String {
    let text: String = text
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    // Match on bytes: every fragment and delimiter is ASCII, so matches can
    // never start or end inside a multi-byte character.
    let lower = text.to_ascii_lowercase().into_bytes();
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut index = 0;

    while index < bytes.len() {
        let Some(fragment) = SENSITIVE_KEY_FRAGMENTS
            .iter()
            .find(|fragment| lower[index..].starts_with(fragment.as_bytes()))
        else {
            index += 1;
            continue;
        };

        let mut cursor = index + fragment.len();
        // Finish the key word (e.g. `token` inside `access_token_value`).
        while cursor < bytes.len()
            && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'_')
        {
            cursor += 1;
        }
        let key_end = cursor;
        while cursor < bytes.len() && matches!(bytes[cursor], b'"' | b'\'' | b' ') {
            cursor += 1;
        }
        let has_separator = cursor < bytes.len() && matches!(bytes[cursor], b'=' | b':');
        if has_separator {
            cursor += 1;
        } else if *fragment != "bearer" || key_end == cursor {
            index = key_end;
            continue;
        }
        while cursor < bytes.len() && matches!(bytes[cursor], b'"' | b'\'' | b' ') {
            cursor += 1;
        }
        // `Authorization: Bearer <token>` / `Basic <token>`: skip the scheme word.
        if let Some(scheme) = [&b"bearer "[..], &b"basic "[..]]
            .into_iter()
            .find(|scheme| lower[cursor..].starts_with(scheme))
        {
            cursor += scheme.len();
        }
        let value_start = cursor;
        while cursor < bytes.len()
            && !matches!(
                bytes[cursor],
                b' ' | b'&' | b',' | b';' | b'"' | b'\'' | b'}' | b')' | b']'
            )
        {
            cursor += 1;
        }
        if cursor > value_start {
            out.push_str(&text[copied..value_start]);
            out.push_str("<redacted>");
            copied = cursor;
        }
        index = cursor.max(index + 1);
    }

    out.push_str(&text[copied..]);
    out
}

fn bound_chars(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let mut bounded = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        bounded.push_str("... <truncated>");
    }
    bounded
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {} (HTTP {})", self.code, self.message, self.status)
    }
}

impl std::error::Error for ApiError {}

/// Sanitized error reported by a successful HTTP GraphQL response.
///
/// Only bounded `errors[*].message` strings are retained. Query text, partial
/// data, paths, locations, extensions, and the raw response body are discarded.
#[derive(Debug, Serialize)]
pub struct GraphqlError {
    pub detail: Option<String>,
}

impl GraphqlError {
    /// Maximum GraphQL errors retained in structured stderr output.
    const MAX_MESSAGES: usize = 8;
    /// Maximum Unicode scalar values retained from each GraphQL error message.
    const MAX_MESSAGE_CHARS: usize = 256;

    pub fn from_errors(errors: &[serde_json::Value]) -> Self {
        let mut messages = errors
            .iter()
            .filter_map(|error| error.get("message").and_then(serde_json::Value::as_str))
            .filter_map(|message| {
                let message = message.trim();
                if message.is_empty() {
                    None
                } else {
                    Some(truncate_graphql_message(message))
                }
            })
            .take(Self::MAX_MESSAGES)
            .collect::<Vec<_>>();

        if errors.len() > Self::MAX_MESSAGES {
            messages.push(format!(
                "<{} additional GraphQL errors omitted>",
                errors.len() - Self::MAX_MESSAGES
            ));
        }

        Self {
            detail: if messages.is_empty() {
                None
            } else {
                Some(messages.join("; "))
            },
        }
    }

    pub const fn code(&self) -> &'static str {
        "GRAPHQL_ERROR"
    }

    pub const fn message(&self) -> &'static str {
        "GraphQL request returned errors"
    }
}

impl std::fmt::Display for GraphqlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code(), self.message())
    }
}

impl std::error::Error for GraphqlError {}

fn truncate_graphql_message(message: &str) -> String {
    let mut chars = message.chars();
    let mut truncated = chars
        .by_ref()
        .take(GraphqlError::MAX_MESSAGE_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        truncated.push_str("... <truncated>");
    }
    truncated
}

fn safe_error_detail(body: &str) -> String {
    const MAX_DETAIL_LEN: usize = 1024;

    if body.trim().is_empty() {
        return String::new();
    }

    let include_sensitive = std::env::var("SNOW_CLI_DEBUG_HTTP_INCLUDE_SENSITIVE")
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            !(normalized.is_empty() || matches!(normalized.as_str(), "0" | "false" | "off" | "no"))
        })
        .unwrap_or(false);

    if !include_sensitive {
        return format!("<response body redacted, {} bytes>", body.len());
    }

    let mut detail = body.chars().take(MAX_DETAIL_LEN).collect::<String>();
    if body.chars().count() > MAX_DETAIL_LEN {
        detail.push_str("... <truncated>");
    }
    detail
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_status_maps_common_codes() {
        let err = ApiError::from_status(404, "https://test.service-now.com", None);
        assert_eq!(err.code, "NOT_FOUND");
        assert_eq!(err.status, 404);

        let err = ApiError::from_status(401, "https://test.service-now.com", None);
        assert_eq!(err.code, "UNAUTHORIZED");

        let err = ApiError::from_status(500, "https://test.service-now.com", None);
        assert_eq!(err.code, "SERVER_ERROR");

        let err = ApiError::from_status(429, "https://test.service-now.com", None);
        assert_eq!(err.code, "RATE_LIMITED");
    }

    #[test]
    fn test_from_status_redacts_detail_by_default() {
        let err = ApiError::from_status(
            400,
            "https://test.service-now.com",
            Some("Invalid encoded query containing token=secret".to_string()),
        );
        assert_eq!(
            err.detail,
            Some("<response body redacted, 45 bytes>".to_string())
        );
    }

    #[test]
    fn test_from_status_surfaces_servicenow_envelope() {
        let body = r#"{"error":{"message":"Invalid table u_does_not_exist","detail":null},"status":"failure"}"#;
        let err = ApiError::from_status(400, "https://test.service-now.com", Some(body.into()));
        assert_eq!(err.code, "INVALID_TABLE");
        assert_eq!(err.message, "Invalid table u_does_not_exist");
        assert_eq!(err.detail, None);
        assert_eq!(err.status, 400);
    }

    #[test]
    fn test_from_status_keeps_http_code_for_generic_envelope() {
        let body = r#"{"error":{"message":"User is not authenticated","detail":"Required to provide Auth information"},"status":"failure"}"#;
        let err = ApiError::from_status(401, "https://test.service-now.com", Some(body.into()));
        assert_eq!(err.code, "UNAUTHORIZED");
        assert_eq!(err.message, "User is not authenticated");
        assert_eq!(
            err.detail.as_deref(),
            Some("Required to provide Auth information")
        );
    }

    #[test]
    fn test_from_status_refines_record_and_acl_codes() {
        let not_found = r#"{"error":{"message":"No Record found","detail":"Record doesn't exist or ACL restricts the record retrieval"},"status":"failure"}"#;
        let err = ApiError::from_status(404, "https://x", Some(not_found.into()));
        assert_eq!(err.code, "RECORD_NOT_FOUND");

        let acl = r#"{"error":{"message":"Operation Failed","detail":"ACL Exception Update Failed due to security constraints"},"status":"failure"}"#;
        let err = ApiError::from_status(403, "https://x", Some(acl.into()));
        assert_eq!(err.code, "ACL_DENIED");

        let forbidden = r#"{"error":{"message":"Forbidden","detail":null}}"#;
        let err = ApiError::from_status(403, "https://x", Some(forbidden.into()));
        assert_eq!(err.code, "FORBIDDEN");
    }

    #[test]
    fn test_from_status_scrubs_and_bounds_envelope_text() {
        let long = "x".repeat(ApiError::MAX_ENVELOPE_CHARS + 10);
        let body = serde_json::json!({
            "error": {
                "message": "Bad query token=abc123secret&x=1",
                "detail": long,
            }
        })
        .to_string();
        let err = ApiError::from_status(400, "https://x", Some(body));
        assert_eq!(err.message, "Bad query token=<redacted>&x=1");
        assert!(err.detail.unwrap().ends_with("... <truncated>"));
    }

    #[test]
    fn test_from_status_redacts_non_envelope_bodies() {
        for body in [
            "<html><body>Error token=abc</body></html>",
            r#"{"result":{"password":"hunter2"}}"#,
            r#"{"error":{"message":""}}"#,
        ] {
            let err = ApiError::from_status(500, "https://x", Some(body.into()));
            assert_eq!(err.message, "ServiceNow internal error");
            assert!(
                err.detail
                    .as_deref()
                    .unwrap()
                    .starts_with("<response body redacted"),
                "{body}"
            );
        }
    }

    #[test]
    fn scrub_secrets_redacts_common_credential_shapes() {
        let cases = [
            ("password=hunter2 next", "password=<redacted> next"),
            (
                r#"{"access_token": "abc.def"}"#,
                r#"{"access_token": "<redacted>"}"#,
            ),
            (
                "Authorization: Bearer abc.def",
                "Authorization: Bearer <redacted>",
            ),
            (
                "Authorization: Basic dXNlcjpwYXNz",
                "Authorization: Basic <redacted>",
            ),
            ("Bearer abc.def rejected", "Bearer <redacted> rejected"),
            (
                "Cookie: JSESSIONID=ABC; glide_user=x",
                "Cookie: <redacted>; glide_user=x",
            ),
            ("g_ck=abc", "g_ck=<redacted>"),
            (
                "Ungültiges Passwort: password=ä€x ok",
                "Ungültiges Passwort: password=<redacted> ok",
            ),
            ("Invalid table u_foo", "Invalid table u_foo"),
            ("token expired", "token expired"),
            ("line1\nline2\u{1b}[31m", "line1 line2 [31m"),
        ];
        for (input, expected) in cases {
            assert_eq!(scrub_secrets(input), expected, "input: {input:?}");
        }
    }

    #[test]
    fn test_display() {
        let err = ApiError::from_status(404, "https://test.service-now.com", None);
        let display = format!("{err}");
        assert!(display.contains("NOT_FOUND"));
        assert!(display.contains("404"));
    }

    #[test]
    fn graphql_error_retains_only_bounded_messages() {
        let secret_extension = "must-not-be-retained";
        let long_message = "x".repeat(GraphqlError::MAX_MESSAGE_CHARS + 1);
        let errors = serde_json::json!([
            {
                "message": long_message,
                "path": ["incident", "caller"],
                "extensions": {"debug": secret_extension}
            },
            {"message": 42},
            {"locations": [{"line": 1}]}
        ]);
        let error = GraphqlError::from_errors(errors.as_array().unwrap());
        let detail = error.detail.as_deref().unwrap();

        assert!(detail.contains("<truncated>"));
        assert!(!detail.contains(secret_extension));
        assert!(!detail.contains("incident"));
        assert_eq!(error.code(), "GRAPHQL_ERROR");
    }
}
