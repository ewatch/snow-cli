use reqwest::Url;

use super::redaction::{
    is_sensitive_field_name, is_sensitive_header_name, is_url_bearing_header_name,
};

const HTTP_DEBUG_ENV_VAR: &str = "SNOW_CLI_DEBUG_HTTP";

const HTTP_DEBUG_INCLUDE_SENSITIVE_ENV_VAR: &str = "SNOW_CLI_DEBUG_HTTP_INCLUDE_SENSITIVE";

/// Maximum UTF-8 byte length kept for HTTP debug request body previews after redaction.
const HTTP_DEBUG_BODY_PREVIEW_LIMIT: usize = 2048;

/// Maximum raw request body size parsed for structured redaction in HTTP debug logs.
/// Bodies above this cap are withheld outright to avoid unbounded parsing work.
const HTTP_DEBUG_MAX_PARSE_BYTES: usize = 1024 * 1024;

pub(super) fn parse_http_debug_env_value(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    !(normalized.is_empty() || matches!(normalized.as_str(), "0" | "false" | "off" | "no"))
}

pub(crate) fn is_http_debug_enabled() -> bool {
    std::env::var(HTTP_DEBUG_ENV_VAR)
        .map(|value| parse_http_debug_env_value(&value))
        .unwrap_or(false)
}

pub(crate) fn is_http_debug_sensitive_enabled() -> bool {
    std::env::var(HTTP_DEBUG_INCLUDE_SENSITIVE_ENV_VAR)
        .map(|value| parse_http_debug_env_value(&value))
        .unwrap_or(false)
}

pub(super) fn format_header_value_for_http_debug(
    name: &str,
    value: &http::HeaderValue,
    include_sensitive: bool,
) -> String {
    if is_sensitive_header_name(name) && !include_sensitive {
        return "<redacted>".to_string();
    }

    match value.to_str() {
        Ok(text) => {
            if is_url_bearing_header_name(name) && !include_sensitive {
                redact_url_str_for_http_debug(text)
            } else {
                text.to_string()
            }
        }
        Err(_) => format!("<{} bytes binary>", value.as_bytes().len()),
    }
}

pub(super) fn format_headers_for_http_debug(
    headers: &http::HeaderMap,
    include_sensitive: bool,
) -> String {
    let mut lines = headers
        .iter()
        .map(|(name, value)| {
            let name = name.as_str().to_ascii_lowercase();
            let value = format_header_value_for_http_debug(&name, value, include_sensitive);
            format!("{name}: {value}")
        })
        .collect::<Vec<_>>();

    lines.sort();

    if lines.is_empty() {
        "<none>".to_string()
    } else {
        lines.join("\n")
    }
}

pub(super) fn redact_url_for_http_debug(url: &Url) -> String {
    let has_userinfo = !url.username().is_empty() || url.password().is_some();
    let has_query = url.query().is_some();
    let has_fragment = url.fragment().is_some();
    if !has_userinfo && !has_query && !has_fragment {
        return url.to_string();
    }

    let mut redacted = url.clone();
    if has_userinfo {
        if !redacted.username().is_empty() {
            let _ = redacted.set_username("<redacted>");
        }
        if redacted.password().is_some() {
            let _ = redacted.set_password(Some("<redacted>"));
        }
    }
    if has_query {
        redacted
            .query_pairs_mut()
            .clear()
            .extend_pairs(url.query_pairs().map(|(key, value)| {
                let value = if is_sensitive_field_name(&key) {
                    std::borrow::Cow::Borrowed("<redacted>")
                } else {
                    value
                };
                (key, value)
            }));
    }
    if has_fragment {
        redacted.set_fragment(Some("<redacted>"));
    }
    redacted.to_string()
}

pub(crate) fn redact_url_str_for_http_debug(raw: &str) -> String {
    if let Ok(parsed) = Url::parse(raw) {
        return redact_url_for_http_debug(&parsed);
    }

    const RELATIVE_ORIGIN: &str = "https://relative.invalid";
    if let Ok(base) = Url::parse(RELATIVE_ORIGIN)
        && let Ok(joined) = base.join(raw)
        && joined.origin() == base.origin()
    {
        let redacted = redact_url_for_http_debug(&joined);
        if let Some(stripped) = redacted.strip_prefix(RELATIVE_ORIGIN) {
            return stripped.to_string();
        }
    }

    super::error::scrub_secrets(raw)
}

fn decode_form_key(raw_key: &str) -> String {
    url::form_urlencoded::parse(format!("{raw_key}=").as_bytes())
        .next()
        .map(|(decoded, _)| decoded.into_owned())
        .unwrap_or_else(|| raw_key.to_string())
}

pub(super) fn redact_form_body_for_http_debug(text: &str) -> String {
    text.split('&')
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let decoded_key = decode_form_key(key);
            if is_sensitive_field_name(key) || is_sensitive_field_name(&decoded_key) {
                format!("{key}=<redacted>")
            } else if value.is_empty() && !pair.contains('=') {
                key.to_string()
            } else {
                format!("{key}={value}")
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}

pub(super) fn redact_json_value_for_http_debug(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if is_sensitive_field_name(key) {
                    *child = serde_json::Value::String("<redacted>".to_string());
                } else {
                    redact_json_value_for_http_debug(child);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_json_value_for_http_debug(item);
            }
        }
        _ => {}
    }
}

pub(super) fn redact_body_text_for_http_debug(text: &str, content_type: Option<&str>) -> String {
    let media_type = content_type
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let looks_like_json = {
        let trimmed = text.trim_start();
        trimmed.starts_with('{') || trimmed.starts_with('[')
    };

    if media_type == "application/json"
        || media_type.ends_with("+json")
        || (media_type.is_empty() && looks_like_json)
    {
        return match serde_json::from_str::<serde_json::Value>(text) {
            Ok(mut value) => {
                redact_json_value_for_http_debug(&mut value);
                serde_json::to_string(&value).unwrap_or_else(|_| "<redacted body>".to_string())
            }
            Err(_) => "<redacted body>".to_string(),
        };
    }

    if media_type == "application/x-www-form-urlencoded"
        || (media_type.is_empty() && !looks_like_json && text.contains('='))
    {
        return redact_form_body_for_http_debug(text);
    }

    "<redacted body>".to_string()
}

fn truncate_utf8_at_byte_limit(text: &str, max_bytes: usize) -> (&str, bool) {
    if text.len() <= max_bytes {
        return (text, false);
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

pub(super) fn format_body_bytes_for_http_debug(
    bytes: &[u8],
    content_type: Option<&str>,
    include_sensitive: bool,
) -> String {
    if bytes.is_empty() {
        return "<empty>".to_string();
    }

    if !include_sensitive {
        return format!("<redacted body, {} bytes>", bytes.len());
    }

    if bytes.len() > HTTP_DEBUG_MAX_PARSE_BYTES {
        return format!("<redacted body too large, {} bytes>", bytes.len());
    }

    match std::str::from_utf8(bytes) {
        Ok(text) => {
            let redacted = redact_body_text_for_http_debug(text, content_type);
            let (preview, truncated) =
                truncate_utf8_at_byte_limit(&redacted, HTTP_DEBUG_BODY_PREVIEW_LIMIT);
            if truncated {
                format!("{preview}... <truncated, {} bytes total>", bytes.len())
            } else {
                preview.to_string()
            }
        }
        Err(_) => format!("<{} bytes binary>", bytes.len()),
    }
}

pub(super) fn format_request_body_for_http_debug(request: &reqwest::Request) -> String {
    let body = match request.body() {
        Some(body) => body,
        None => return "<none>".to_string(),
    };

    let bytes = match body.as_bytes() {
        Some(bytes) => bytes,
        None => return "<streaming body>".to_string(),
    };

    let content_type = request
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    format_body_bytes_for_http_debug(
        bytes,
        content_type,
        super::is_http_debug_sensitive_enabled(),
    )
}

pub(crate) fn log_raw_http_request(request: &reqwest::Request) {
    if !is_http_debug_enabled() {
        return;
    }

    let headers =
        format_headers_for_http_debug(request.headers(), super::is_http_debug_sensitive_enabled());
    let body = format_request_body_for_http_debug(request);

    tracing::debug!(
        target: "snow_cli::http",
        method = %request.method(),
        url = %redact_url_for_http_debug(request.url()),
        headers = %headers,
        body = %body,
        "Raw HTTP request"
    );
}

pub(crate) fn log_raw_http_response(
    url: &str,
    status: reqwest::StatusCode,
    headers: &http::HeaderMap,
) {
    if !is_http_debug_enabled() {
        return;
    }

    let headers = format_headers_for_http_debug(headers, super::is_http_debug_sensitive_enabled());
    let redacted_url = redact_url_str_for_http_debug(url);

    tracing::debug!(
        target: "snow_cli::http",
        url = %redacted_url,
        status = status.as_u16(),
        headers = %headers,
        "Raw HTTP response"
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::Url;

    #[test]
    fn test_parse_http_debug_env_value() {
        assert!(parse_http_debug_env_value("1"));
        assert!(parse_http_debug_env_value("true"));
        assert!(parse_http_debug_env_value("yes"));
        assert!(!parse_http_debug_env_value("0"));
        assert!(!parse_http_debug_env_value("false"));
        assert!(!parse_http_debug_env_value("off"));
        assert!(!parse_http_debug_env_value(""));
    }

    #[test]
    fn test_format_headers_for_http_debug_redacts_sensitive_headers() {
        let request = reqwest::Client::new()
            .post("https://example.com/api")
            .header("Authorization", "Bearer very-secret")
            .header("Cookie", "JSESSIONID=abc123")
            .header("X-Trace-Id", "trace-42")
            .build()
            .unwrap();

        let headers = format_headers_for_http_debug(request.headers(), false);
        assert!(headers.contains("authorization: <redacted>"));
        assert!(headers.contains("cookie: <redacted>"));
        assert!(headers.contains("x-trace-id: trace-42"));
    }

    #[test]
    fn test_format_headers_for_http_debug_includes_sensitive_headers_when_enabled() {
        let request = reqwest::Client::new()
            .post("https://example.com/api")
            .header("Authorization", "Bearer very-secret")
            .header("Cookie", "JSESSIONID=abc123")
            .header("X-Trace-Id", "trace-42")
            .build()
            .unwrap();

        let headers = format_headers_for_http_debug(request.headers(), true);
        assert!(headers.contains("authorization: Bearer very-secret"));
        assert!(headers.contains("cookie: JSESSIONID=abc123"));
        assert!(headers.contains("x-trace-id: trace-42"));
    }

    #[test]
    fn test_format_request_body_for_http_debug_redacts_body_by_default() {
        let no_body_request = reqwest::Client::new()
            .get("https://example.com/api")
            .build()
            .unwrap();
        assert_eq!(
            format_request_body_for_http_debug(&no_body_request),
            "<none>"
        );

        let request = reqwest::Client::new()
            .post("https://example.com/api")
            .body("client_secret=secret&grant_type=client_credentials".to_string())
            .build()
            .unwrap();

        let body_preview = format_request_body_for_http_debug(&request);
        assert!(body_preview.contains("<redacted body"));
        assert!(!body_preview.contains("secret"));
    }

    #[test]
    fn test_redact_body_text_for_http_debug_redacts_form_and_json_secrets() {
        let form = redact_body_text_for_http_debug(
            "client_secret=secret&grant_type=client_credentials&password=pw",
            Some("application/x-www-form-urlencoded"),
        );
        assert!(form.contains("client_secret=<redacted>"));
        assert!(form.contains("password=<redacted>"));
        assert!(form.contains("grant_type=client_credentials"));
        assert!(!form.contains("client_secret=secret"));
        assert!(!form.contains("password=pw"));

        let json = redact_body_text_for_http_debug(
            r#"{"token":"abc","nested":{"refresh_token":"def"},"scope":"useraccount"}"#,
            Some("application/json"),
        );
        assert!(json.contains(r#""token":"<redacted>""#));
        assert!(json.contains(r#""refresh_token":"<redacted>""#));
        assert!(json.contains(r#""scope":"useraccount""#));
        assert!(!json.contains("abc"));
        assert!(!json.contains("def"));
    }

    #[test]
    fn test_redact_url_for_http_debug_redacts_sensitive_query_params() {
        let url = Url::parse(
            "https://example.com/oauth?code=abc&state=xyz&client_secret=secret&sysparm_limit=1",
        )
        .unwrap();
        let redacted = redact_url_for_http_debug(&url);
        assert!(redacted.contains("code=%3Credacted%3E"));
        assert!(redacted.contains("client_secret=%3Credacted%3E"));
        assert!(redacted.contains("state=xyz"));
        assert!(redacted.contains("sysparm_limit=1"));
        assert!(!redacted.contains("code=abc"));
        assert!(!redacted.contains("client_secret=secret"));
    }

    #[test]
    fn test_redact_url_for_http_debug_redacts_userinfo_and_fragment() {
        let url = Url::parse(
            "https://alice:super-secret@example.com/oauth?state=ok#access_token=frag-secret",
        )
        .unwrap();
        let redacted = redact_url_for_http_debug(&url);
        assert!(!redacted.contains("alice"));
        assert!(!redacted.contains("super-secret"));
        assert!(!redacted.contains("frag-secret"));
        assert!(redacted.contains("state=ok"));
        assert!(redacted.contains("#%3Credacted%3E"), "{redacted}");
    }

    #[test]
    fn test_format_headers_redacts_api_keys_csrf_and_url_headers() {
        let request = reqwest::Client::new()
            .get("https://example.com/api")
            .header("x-sn-apikey", "sn-key-123")
            .header("x-api-key", "api-key-456")
            .header("api-key", "raw-key-789")
            .header("x-csrf-token", "csrf-111")
            .header("x-xsrf-token", "xsrf-222")
            .header(
                "Location",
                "https://user:pass@example.com/cb?code=secret-code&state=ok#token=frag",
            )
            .header("Referer", "/oauth/callback?access_token=ref-secret#frag2")
            .build()
            .unwrap();

        let headers = format_headers_for_http_debug(request.headers(), false);
        for leaked in [
            "sn-key-123",
            "api-key-456",
            "raw-key-789",
            "csrf-111",
            "xsrf-222",
            "user:pass",
            "secret-code",
            "token=frag",
            "ref-secret",
            "frag2",
        ] {
            assert!(!headers.contains(leaked), "leaked {leaked} in:\n{headers}");
        }
        assert!(headers.contains("x-sn-apikey: <redacted>"));
        assert!(headers.contains("x-api-key: <redacted>"));
        assert!(headers.contains("api-key: <redacted>"));
        assert!(headers.contains("x-csrf-token: <redacted>"));
        assert!(headers.contains("x-xsrf-token: <redacted>"));
        assert!(headers.contains("state=ok"));
    }

    #[test]
    fn test_redact_form_body_percent_decodes_keys() {
        let form = redact_form_body_for_http_debug(
            "client%5Fsecret=topsecret&user%5fpassword=hunter2&grant_type=refresh_token",
        );
        assert!(!form.contains("topsecret"), "{form}");
        assert!(!form.contains("hunter2"), "{form}");
        assert!(form.contains("client%5Fsecret=<redacted>"), "{form}");
        assert!(form.contains("user%5fpassword=<redacted>"), "{form}");
        assert!(form.contains("grant_type=refresh_token"), "{form}");
    }

    #[test]
    fn test_large_json_body_over_2048_bytes_with_equals_redacts_before_truncating() {
        let padding = "a=b&".repeat(600); // > 2048 bytes and contains '='
        let payload = serde_json::json!({
            "password": "early-secret-value",
            "query": padding,
            "client_secret": "late-secret-value",
            "nested": { "access_token": "nested-secret-value" }
        })
        .to_string();
        assert!(payload.len() > HTTP_DEBUG_BODY_PREVIEW_LIMIT);

        let preview =
            format_body_bytes_for_http_debug(payload.as_bytes(), Some("application/json"), true);
        assert!(preview.contains("<truncated"), "{preview}");
        assert!(!preview.contains("early-secret-value"), "{preview}");
        assert!(!preview.contains("late-secret-value"), "{preview}");
        assert!(!preview.contains("nested-secret-value"), "{preview}");
        assert!(preview.contains("<redacted>"), "{preview}");
    }

    #[test]
    fn test_malformed_json_never_falls_through_to_form_parsing() {
        let broken = r#"{"password":"secret-value","query":"a=b""#;
        let redacted = redact_body_text_for_http_debug(broken, Some("application/json"));
        assert_eq!(redacted, "<redacted body>");
        assert!(!redacted.contains("secret-value"));

        let broken_no_ct = r#"{"token":"secret-value","eq":"a=b""#;
        let redacted_no_ct = redact_body_text_for_http_debug(broken_no_ct, None);
        assert_eq!(redacted_no_ct, "<redacted body>");
    }

    #[test]
    fn test_multibyte_utf8_truncation_preserves_valid_utf8() {
        // Each '€' is 3 bytes; construct a JSON payload exceeding 2048 bytes where byte 2048
        // falls in the middle of a multi-byte UTF-8 character.
        let euros = "€".repeat(800);
        let payload = serde_json::json!({ "note": euros, "password": "secret" }).to_string();
        let preview =
            format_body_bytes_for_http_debug(payload.as_bytes(), Some("application/json"), true);
        assert!(preview.contains("<truncated"), "{preview}");
        assert!(!preview.contains("secret"), "{preview}");
        assert!(!preview.contains("bytes binary"), "{preview}");
    }
}
