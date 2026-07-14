use reqwest::Url;

const HTTP_DEBUG_ENV_VAR: &str = "SNOW_CLI_DEBUG_HTTP";

const HTTP_DEBUG_INCLUDE_SENSITIVE_ENV_VAR: &str = "SNOW_CLI_DEBUG_HTTP_INCLUDE_SENSITIVE";

const HTTP_DEBUG_BODY_PREVIEW_LIMIT: usize = 2048;

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

pub(super) fn is_sensitive_header_name(name: &str) -> bool {
    matches!(
        name,
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "x-user-token"
            | "x-usertoken"
            | "x-auth-token"
    )
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
        Ok(text) => text.to_string(),
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

pub(super) fn is_sensitive_field_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "password"
            | "user_password"
            | "client_secret"
            | "token"
            | "access_token"
            | "refresh_token"
            | "api_token"
            | "session_cookie"
            | "code"
            | "code_verifier"
            | "sysparm_ck"
    ) || normalized.contains("secret")
        || normalized.contains("password")
        || normalized.contains("token")
        || normalized.contains("cookie")
}

pub(super) fn redact_url_for_http_debug(url: &Url) -> String {
    if url.query().is_none() {
        return url.to_string();
    }

    let mut redacted = url.clone();
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
    redacted.to_string()
}

pub(super) fn redact_form_body_for_http_debug(text: &str) -> String {
    text.split('&')
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            if is_sensitive_field_name(key) {
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
    let content_type = content_type.unwrap_or_default().to_ascii_lowercase();
    if content_type.contains("application/json")
        && let Ok(mut value) = serde_json::from_str::<serde_json::Value>(text)
    {
        redact_json_value_for_http_debug(&mut value);
        return serde_json::to_string(&value).unwrap_or_else(|_| "<redacted body>".to_string());
    }

    if content_type.contains("application/x-www-form-urlencoded") || text.contains('=') {
        return redact_form_body_for_http_debug(text);
    }

    "<redacted body>".to_string()
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

    if bytes.is_empty() {
        return "<empty>".to_string();
    }

    if !super::is_http_debug_sensitive_enabled() {
        return format!("<redacted body, {} bytes>", bytes.len());
    }

    let preview_len = bytes.len().min(HTTP_DEBUG_BODY_PREVIEW_LIMIT);
    let preview_bytes = &bytes[..preview_len];

    match std::str::from_utf8(preview_bytes) {
        Ok(text) => {
            let content_type = request
                .headers()
                .get(http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok());
            let redacted = redact_body_text_for_http_debug(text, content_type);
            if preview_len < bytes.len() {
                format!("{redacted}... <truncated, {} bytes total>", bytes.len())
            } else {
                redacted
            }
        }
        Err(_) => format!("<{} bytes binary>", bytes.len()),
    }
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

    let redacted_url = Url::parse(url)
        .map(|parsed| redact_url_for_http_debug(&parsed))
        .unwrap_or_else(|_| url.to_string());

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
}
