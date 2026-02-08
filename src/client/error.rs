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
            detail: body,
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
    fn test_from_status_includes_detail() {
        let err = ApiError::from_status(
            400,
            "https://test.service-now.com",
            Some("Invalid encoded query".to_string()),
        );
        assert_eq!(err.detail, Some("Invalid encoded query".to_string()));
    }

    #[test]
    fn test_display() {
        let err = ApiError::from_status(404, "https://test.service-now.com", None);
        let display = format!("{err}");
        assert!(display.contains("NOT_FOUND"));
        assert!(display.contains("404"));
    }
}
