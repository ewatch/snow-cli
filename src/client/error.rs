use serde::Serialize;

/// Transport classification never retains URLs, payloads, or reqwest Display text.
#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum TransportError {
    #[error("HTTP request timed out; the server may already have processed the request")]
    Timeout,
    #[error("Unable to connect to the server")]
    Connection,
    #[error(
        "HTTP response contains invalid JSON or an unexpected envelope; response data is withheld"
    )]
    InvalidResponse,
    #[error("{0}")]
    InvalidEnvelope(&'static str),
    #[error("HTTP transport failed; request details are withheld")]
    Other,
}

impl TransportError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Timeout => "REQUEST_TIMEOUT",
            Self::Connection => "CONNECTION_FAILED",
            Self::InvalidResponse | Self::InvalidEnvelope(_) => "INVALID_RESPONSE",
            Self::Other => "TRANSPORT_ERROR",
        }
    }

    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Timeout | Self::Connection)
    }
}

/// Keep dependency on the HTTP library inside the client seam, including when
/// classifying errors from response body reads and optional client operations.
pub(crate) fn classify_transport_error(error: &anyhow::Error) -> Option<TransportError> {
    if let Some(error) = error.downcast_ref::<TransportError>() {
        return Some(*error);
    }
    let error = error.downcast_ref::<reqwest::Error>()?;
    Some(if error.is_timeout() {
        TransportError::Timeout
    } else if error.is_connect() {
        TransportError::Connection
    } else if error.is_decode() {
        TransportError::InvalidResponse
    } else {
        TransportError::Other
    })
}

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

        let parsed_body = body
            .as_deref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok());

        // OAuth descriptions may echo credentials or arbitrary request data.
        // Retain only recognized protocol error names, never descriptions or
        // extra fields (even when sensitive HTTP diagnostics are enabled).
        if let Some(oauth_error) = parsed_body.as_ref().and_then(oauth_error_name_from_value) {
            return Self {
                code: code.to_string(),
                message: format!("OAuth2 token request failed: {oauth_error}"),
                status,
                detail: None,
                instance: instance.to_string(),
            };
        }

        if let Some(envelope) = parsed_body
            .as_ref()
            .and_then(ServiceNowErrorEnvelope::from_value)
        {
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
            detail: body.as_deref().and_then(safe_error_detail),
            instance: instance.to_string(),
        }
    }
}

/// OAuth 2.0 error names from RFC 6749 sections 4.1.2.1 and 5.2.
/// An allow-list prevents a server-controlled `error` string echoing secrets.
#[cfg(test)]
fn oauth_error_name(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    oauth_error_name_from_value(&value)
}

fn oauth_error_name_from_value(value: &serde_json::Value) -> Option<String> {
    let name = value.get("error")?.as_str()?;
    match name {
        "invalid_request"
        | "invalid_client"
        | "invalid_grant"
        | "unauthorized_client"
        | "unsupported_grant_type"
        | "invalid_scope"
        | "access_denied"
        | "unsupported_response_type"
        | "server_error"
        | "temporarily_unavailable" => Some(name.to_string()),
        _ => None,
    }
}

/// The `error` object of a standard ServiceNow REST failure response.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ServiceNowErrorEnvelope {
    pub(crate) message: String,
    pub(crate) detail: Option<String>,
}

impl ServiceNowErrorEnvelope {
    /// Extract a ServiceNow error envelope from an already-parsed JSON value.
    pub(crate) fn from_value(value: &serde_json::Value) -> Option<Self> {
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

use super::redaction::SENSITIVE_KEY_FRAGMENTS;

fn scrub_url_userinfo(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut pos = 0;

    while let Some(rel) = text[pos..].find("://") {
        let authority_start = pos + rel + 3;
        let mut end = authority_start;
        while end < bytes.len()
            && !matches!(
                bytes[end],
                b'/' | b'?'
                    | b'#'
                    | b' '
                    | b'\t'
                    | b'"'
                    | b'\''
                    | b'<'
                    | b'>'
                    | b')'
                    | b']'
                    | b'}'
                    | b','
            )
        {
            end += 1;
        }
        if let Some(at_rel) = text[authority_start..end].rfind('@') {
            let at_pos = authority_start + at_rel;
            if at_pos > authority_start {
                out.push_str(&text[copied..authority_start]);
                out.push_str("<redacted>");
                copied = at_pos;
            }
        }
        pos = end;
    }

    out.push_str(&text[copied..]);
    out
}

fn looks_like_basic_credential(candidate: &str) -> bool {
    if candidate.len() < 4
        || !candidate
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'-' | b'_'))
    {
        return false;
    }

    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(candidate)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(candidate));
    if let Ok(bytes) = decoded
        && let Ok(text) = std::str::from_utf8(&bytes)
        && text.contains(':')
        && text.bytes().all(|b| !b.is_ascii_control())
    {
        return true;
    }

    let has_upper = candidate.bytes().any(|b| b.is_ascii_uppercase());
    let has_lower = candidate.bytes().any(|b| b.is_ascii_lowercase());
    let has_digit = candidate.bytes().any(|b| b.is_ascii_digit());
    let has_b64_punct = candidate.bytes().any(|b| matches!(b, b'+' | b'/' | b'='));
    candidate.len() >= 8 && (has_b64_punct || (has_upper && has_lower && has_digit))
}

fn scan_unquoted_value_end(bytes: &[u8], start: usize) -> usize {
    let mut cursor = start;
    while cursor < bytes.len()
        && !matches!(
            bytes[cursor],
            b' ' | b'&' | b',' | b';' | b'"' | b'\'' | b'}' | b')' | b']'
        )
    {
        cursor += 1;
    }
    cursor
}

/// Redact credential-like values from free text taken from a server response.
///
/// Values following a sensitive key (`token=..`, `"password": ".."`,
/// `Authorization: Bearer ..`), standalone `Bearer`/`Basic` credentials,
/// multi-cookie `Cookie:` headers, and URL userinfo are replaced with
/// `<redacted>`, and control characters (including ANSI escapes) are replaced
/// with spaces.
pub(crate) fn scrub_secrets(text: &str) -> String {
    let sanitized: String = text
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let text = scrub_url_userinfo(&sanitized);
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

        if matches!(*fragment, "bearer" | "basic")
            && index > 0
            && (bytes[index - 1].is_ascii_alphanumeric() || bytes[index - 1] == b'_')
        {
            index += 1;
            continue;
        }

        let mut cursor = index + fragment.len();
        // Finish the key word (e.g. `token` inside `access_token_value` or `x-csrf-token`).
        while cursor < bytes.len()
            && (bytes[cursor].is_ascii_alphanumeric() || matches!(bytes[cursor], b'_' | b'-'))
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
        } else if !matches!(*fragment, "bearer" | "basic") || key_end == cursor {
            index = key_end;
            continue;
        }

        while cursor < bytes.len() && bytes[cursor] == b' ' {
            cursor += 1;
        }
        let opening_quote = if cursor < bytes.len() && matches!(bytes[cursor], b'"' | b'\'') {
            let quote = bytes[cursor];
            cursor += 1;
            Some(quote)
        } else {
            None
        };

        // `Authorization: Bearer <token>` / `Basic <token>`: skip the scheme word.
        if has_separator
            && let Some(scheme) = [&b"bearer "[..], &b"basic "[..]]
                .into_iter()
                .find(|scheme| lower[cursor..].starts_with(scheme))
        {
            cursor += scheme.len();
            while cursor < bytes.len() && bytes[cursor] == b' ' {
                cursor += 1;
            }
        }

        let value_start = cursor;
        if let Some(quote) = opening_quote {
            while cursor < bytes.len() {
                if bytes[cursor] == b'\\' && cursor + 1 < bytes.len() {
                    cursor += 2;
                    continue;
                }
                if bytes[cursor] == quote {
                    break;
                }
                cursor += 1;
            }
        } else {
            cursor = scan_unquoted_value_end(bytes, cursor);
            // Multi-cookie header (`Cookie: a=1; glide_user_activity=2; glide_session_store=3`).
            if has_separator && *fragment == "cookie" {
                while cursor < bytes.len() && bytes[cursor] == b';' {
                    let mut next = cursor + 1;
                    while next < bytes.len() && bytes[next] == b' ' {
                        next += 1;
                    }
                    let pair_end = scan_unquoted_value_end(bytes, next);
                    if bytes[next..pair_end].contains(&b'=') {
                        cursor = pair_end;
                    } else {
                        break;
                    }
                }
            }
        }

        if !has_separator
            && *fragment == "basic"
            && !looks_like_basic_credential(&text[value_start..cursor])
        {
            index = cursor.max(index + 1);
            continue;
        }

        if cursor > value_start {
            out.push_str(&text[copied..value_start]);
            out.push_str("<redacted>");
            copied = cursor;
        }
        index = if opening_quote.is_some() && cursor < bytes.len() {
            cursor + 1
        } else {
            cursor.max(index + 1)
        };
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
    /// Total detail character budget aligned with `crate::error::MAX_DIAGNOSTIC_CHARS`
    /// so `safe_message` never chops off later errors or the omission notice.
    const MAX_DETAIL_CHARS: usize = crate::error::MAX_DIAGNOSTIC_CHARS;

    pub fn from_errors(errors: &[serde_json::Value]) -> Self {
        let sanitized: Vec<String> = errors
            .iter()
            .filter_map(|error| error.get("message").and_then(serde_json::Value::as_str))
            .filter_map(|message| {
                let message = scrub_secrets(message.trim());
                let message = message.trim();
                if message.is_empty() {
                    None
                } else {
                    Some(truncate_graphql_message(message))
                }
            })
            .collect();

        let total_sanitized = sanitized.len();
        let mut kept: Vec<String> = Vec::new();
        let mut used_chars = 0usize;

        for (index, message) in sanitized.into_iter().take(Self::MAX_MESSAGES).enumerate() {
            let msg_chars = message.chars().count();
            let sep_chars = if kept.is_empty() { 0 } else { 2 }; // "; "
            let remaining_after = total_sanitized.saturating_sub(index + 1);
            let notice_chars = if remaining_after > 0 {
                2 + format!("<{remaining_after} additional GraphQL errors omitted>")
                    .chars()
                    .count()
            } else {
                0
            };

            if !kept.is_empty()
                && used_chars + sep_chars + msg_chars + notice_chars > Self::MAX_DETAIL_CHARS
            {
                break;
            }

            used_chars += sep_chars + msg_chars;
            kept.push(message);
        }

        let omitted = if kept.len() < total_sanitized {
            total_sanitized - kept.len()
        } else {
            errors.len().saturating_sub(Self::MAX_MESSAGES)
        };
        if omitted > 0 {
            kept.push(format!("<{omitted} additional GraphQL errors omitted>"));
        }

        Self {
            detail: if kept.is_empty() {
                None
            } else {
                Some(kept.join("; "))
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

/// Unknown response text is always withheld from runtime errors. Sensitive
/// HTTP diagnostics are a separate channel and cannot weaken this boundary.
pub(crate) fn safe_error_detail(body: &str) -> Option<String> {
    if body.trim().is_empty() {
        None
    } else {
        Some(format!("<response body redacted, {} bytes>", body.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_response_detail_is_always_redacted() {
        let body = "private credential ".repeat(100);
        assert_eq!(
            safe_error_detail(&body),
            Some(format!("<response body redacted, {} bytes>", body.len()))
        );
        assert_eq!(safe_error_detail("  "), None);
        assert_eq!(safe_error_detail(""), None);
    }

    #[test]
    fn oauth_error_names_are_allow_listed_and_ignore_descriptions() {
        assert_eq!(
            oauth_error_name(r#"{"error":"invalid_client","error_description":"secret"}"#)
                .as_deref(),
            Some("invalid_client")
        );
        for body in [
            r#"{"error":"unlabelled-secret"}"#,
            r#"{"error":42}"#,
            "<html>secret</html>",
            "invalid JSON",
        ] {
            assert_eq!(oauth_error_name(body), None);
        }
    }

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
            ("Cookie: JSESSIONID=ABC; glide_user=x", "Cookie: <redacted>"),
            (
                "Cookie: JSESSIONID=ABC; glide_user_activity=secret1; glide_session_store=secret2 failed",
                "Cookie: <redacted> failed",
            ),
            (
                r#"{"password": "my secret, with spaces", "ok": true}"#,
                r#"{"password": "<redacted>", "ok": true}"#,
            ),
            (
                r#"{"password": "escaped \"quote\" secret"}"#,
                r#"{"password": "<redacted>"}"#,
            ),
            ("Basic dXNlcjpwYXNz rejected", "Basic <redacted> rejected"),
            (
                "Basic authentication required",
                "Basic authentication required",
            ),
            (
                "Failed to connect to https://admin:hunter2@dev123.service-now.com/api",
                "Failed to connect to https://<redacted>@dev123.service-now.com/api",
            ),
            ("X-SN-ApiKey: sn-secret-key", "X-SN-ApiKey: <redacted>"),
            (
                "glide_user_activity=secret1; glide_session_store=secret2",
                "glide_user_activity=<redacted>; glide_session_store=<redacted>",
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

    #[test]
    fn graphql_error_scrubs_secrets_in_messages() {
        let errors = serde_json::json!([
            {
                "message": "Auth failed for Authorization: Bearer top-secret-tok and https://admin:pw@dev.service-now.com"
            }
        ]);
        let error = GraphqlError::from_errors(errors.as_array().unwrap());
        let detail = error.detail.as_deref().unwrap();
        assert!(!detail.contains("top-secret-tok"), "{detail}");
        assert!(!detail.contains("admin:pw"), "{detail}");
        assert!(detail.contains("<redacted>"), "{detail}");
    }
}
