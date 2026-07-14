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
    /// Map an HTTP status code to a standard error code.
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

        Self {
            code: code.to_string(),
            message: message.to_string(),
            status,
            detail: body.map(|body| safe_error_detail(&body)),
            instance: instance.to_string(),
        }
    }
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
