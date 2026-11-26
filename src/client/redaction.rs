//! Shared definitions of sensitive HTTP headers, fields, and cookies used across
//! debug logging, error scrubbing, and script response redaction.

/// Lowercase key fragments whose values are redacted in free-text error scrubbing,
/// URL query parameters, form bodies, and JSON debug payloads.
pub(super) const SENSITIVE_KEY_FRAGMENTS: &[&str] = &[
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
    "api-key",
    "x-sn-apikey",
    "x-api-key",
    "x-csrf-token",
    "x-xsrf-token",
    "glide_user",
    "glide_session",
    "glide_sso",
    "bearer",
    "basic",
];

/// Returns `true` when an HTTP header name carries credentials or session tokens.
pub(super) fn is_sensitive_header_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "x-user-token"
            | "x-usertoken"
            | "x-auth-token"
            | "x-sn-apikey"
            | "x-api-key"
            | "api-key"
            | "x-csrf-token"
            | "x-xsrf-token"
    )
}

/// Returns `true` when an HTTP header carries a URL whose query, userinfo, or
/// fragment may contain tokens and must be redacted in debug logs.
pub(super) fn is_url_bearing_header_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    matches!(normalized.as_str(), "location" | "referer")
}

/// Returns `true` when a cookie carries a session credential rather than a UI preference.
pub(super) fn is_credential_cookie_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    normalized == "jsessionid"
        || normalized.starts_with("glide_session")
        || normalized.starts_with("glide_user")
        || normalized.starts_with("glide_sso")
        || normalized.contains("token")
        || normalized.contains("secret")
}

/// Returns `true` when a query parameter, form key, or JSON field name may hold a secret.
pub(super) fn is_sensitive_field_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    matches!(normalized.as_str(), "code" | "code_verifier")
        || is_sensitive_header_name(&normalized)
        || is_credential_cookie_name(&normalized)
        || SENSITIVE_KEY_FRAGMENTS.iter().any(|fragment| {
            !matches!(*fragment, "basic" | "bearer") && normalized.contains(fragment)
        })
}
