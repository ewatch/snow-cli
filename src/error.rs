use serde::Serialize;
use thiserror::Error;

use crate::client::error::{ApiError, GraphqlError};

/// Top-level error type for the CLI. Existing exit-code families are preserved.
#[derive(Error, Debug)]
pub enum CliError {
    #[error("Configuration error: {message}")]
    Config { code: &'static str, message: String },
    #[error("Authentication error: {message}")]
    Auth {
        code: &'static str,
        message: String,
        status: Option<u16>,
    },
    #[error("API error: {message}")]
    Api {
        code: &'static str,
        message: String,
        status: u16,
        detail: Option<String>,
        instance: String,
    },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

/// Failure markers retain classification, never scripts or server stack traces.
#[derive(Debug, Error)]
pub enum ScriptExecutionError {
    #[error(
        "ServiceNow sandbox validation failed; --sandbox accepts a single KittyScript expression, not a multi-statement script"
    )]
    SandboxValidation,
    #[error("ServiceNow script compilation failed")]
    Compilation,
    #[error("ServiceNow script evaluation failed")]
    Evaluation,
    #[error("ServiceNow reported a background script error")]
    BackgroundMessage,
    #[error("ServiceNow script execution raised a Java exception")]
    JavaException,
}

/// Row failures retain only a count; result rows remain on stdout for inspection.
#[derive(Debug, Error)]
#[error(
    "Import set load completed with {count} row-level error(s); inspect stdout results before retrying because some records may already have changed"
)]
pub struct ImportSetRowError {
    pub count: usize,
}

/// Stable, additive classification independent of individual service error codes.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    Validation,
    Config,
    Auth,
    Policy,
    Api,
    Transport,
    Io,
    Command,
    Internal,
}

/// Guidance is NOT permission to replay a mutation or an automatic retry policy.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetryAdvice {
    /// Identical input cannot resolve this failure; fix the cause first.
    Never,
    /// Back off and retry only an operation independently known to be a safe read.
    SafeReadsOnly,
    /// The command may already have changed records; inspect effects first.
    ManualReview,
    /// No reliable retry classification is available. Never blindly replay.
    Unknown,
}

#[derive(Debug, Serialize)]
pub struct ErrorOutput {
    pub error: ErrorBody,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    pub class: ErrorClass,
    pub retry: RetryAdvice,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
}

impl ErrorOutput {
    fn new(code: &str, message: &str, class: ErrorClass, retry: RetryAdvice) -> Self {
        Self {
            error: ErrorBody {
                code: if !code.is_empty()
                    && code.len() <= 64
                    && code.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                    }) {
                    code.to_string()
                } else {
                    "UNKNOWN_ERROR".to_string()
                },
                message: safe_message(message),
                class,
                retry,
                status: None,
                detail: None,
                instance: None,
            },
        }
    }

    fn api(code: &str, message: &str, status: u16, detail: Option<&str>, instance: &str) -> Self {
        let retry = match status {
            408 | 429 | 500 | 502 | 503 | 504 => RetryAdvice::SafeReadsOnly,
            400..=499 => RetryAdvice::Never,
            _ => RetryAdvice::Unknown,
        };
        let mut output = Self::new(code, message, ErrorClass::Api, retry);
        output.error.status = Some(status);
        output.error.detail = detail.map(safe_message);
        // Only an origin can be emitted, never URL userinfo, path, query, or fragment.
        output.error.instance = url::Url::parse(instance)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .map(|url| url.origin().ascii_serialization());
        output
    }

    fn write(&self) {
        if let Ok(json) = serde_json::to_string(self) {
            eprintln!("{json}");
        } else {
            // Never fall back to printing an unsanitized source error.
            eprintln!(
                "{{\"error\":{{\"code\":\"INTERNAL_ERROR\",\"message\":\"Error serialization failed\",\"class\":\"internal\",\"retry\":\"unknown\"}}}}"
            );
        }
    }
}

/// Bound every diagnostic, including fallback errors; strip controls and known
/// credential patterns. Input/transport/config errors additionally discard their
/// source text entirely. Sensitive HTTP debug does not weaken this boundary.
fn safe_message(message: &str) -> String {
    const MAX_DIAGNOSTIC_CHARS: usize = 500;
    let lower = message.to_ascii_lowercase();
    if [
        "sysparm_query",
        "sysparm_script",
        "query=",
        "query:",
        "script=",
        "script:",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return "Diagnostic text withheld because it may contain a query or script".to_string();
    }
    let clean = crate::client::error::scrub_secrets(message);
    let mut chars = clean.chars();
    let mut bounded: String = chars.by_ref().take(MAX_DIAGNOSTIC_CHARS).collect();
    if chars.next().is_some() {
        bounded.push_str("... <truncated>");
    }
    bounded
}

pub fn write_anyhow_error_and_exit_code(error: anyhow::Error) -> i32 {
    let (output, exit_code) = error_output_and_exit_code(&error);
    output.write();
    exit_code
}

fn error_output_and_exit_code(error: &anyhow::Error) -> (ErrorOutput, i32) {
    known_error_output_and_exit_code(error).unwrap_or_else(|| {
        (
            ErrorOutput::new(
                "INTERNAL_ERROR",
                &error.to_string(),
                ErrorClass::Internal,
                RetryAdvice::Unknown,
            ),
            1,
        )
    })
}

fn known_error_output_and_exit_code(error: &anyhow::Error) -> Option<(ErrorOutput, i32)> {
    // Inspect typed sources through anyhow context, not message substrings.
    if let Some(error) = error.downcast_ref::<crate::models::validation::ValidationError>() {
        return Some((
            ErrorOutput::new(
                "VALIDATION_ERROR",
                &error.to_string(),
                ErrorClass::Validation,
                RetryAdvice::Never,
            ),
            2,
        ));
    }
    if let Some(error) = error.downcast_ref::<crate::config::error::ConfigError>() {
        return Some((
            ErrorOutput::new(
                error.code,
                &error.to_string(),
                ErrorClass::Config,
                RetryAdvice::Never,
            ),
            2,
        ));
    }
    if let Some(error) = error.downcast_ref::<ImportSetRowError>() {
        return Some((
            ErrorOutput::new(
                "IMPORT_SET_ROW_ERRORS",
                &error.to_string(),
                ErrorClass::Command,
                RetryAdvice::ManualReview,
            ),
            1,
        ));
    }
    if let Some(error) = error.downcast_ref::<ScriptExecutionError>() {
        return Some((
            ErrorOutput::new(
                "SCRIPT_EXECUTION_FAILED",
                &error.to_string(),
                ErrorClass::Command,
                RetryAdvice::ManualReview,
            ),
            1,
        ));
    }
    if let Some(error) = error.downcast_ref::<crate::policy::PolicyError>() {
        let mut output = ErrorOutput::new(
            error.code(),
            &error.to_string(),
            ErrorClass::Policy,
            RetryAdvice::Never,
        );
        output.error.detail = Some(format!(
            "mode={:?}; capability={}",
            error.mode,
            error.capability.as_str()
        ));
        return Some((output, 7));
    }
    if let Some(error) = error.downcast_ref::<ApiError>() {
        return Some((
            ErrorOutput::api(
                &error.code,
                &error.message,
                error.status,
                error.detail.as_deref(),
                &error.instance,
            ),
            api_exit_code(error.status),
        ));
    }
    if let Some(error) = error.downcast_ref::<GraphqlError>() {
        let mut output = ErrorOutput::new(
            error.code(),
            error.message(),
            ErrorClass::Api,
            RetryAdvice::ManualReview,
        );
        output.error.detail = error.detail.as_deref().map(safe_message);
        return Some((output, 5));
    }
    if let Some(error) = crate::client::error::classify_transport_error(error) {
        let retry = if error.is_transient() {
            RetryAdvice::SafeReadsOnly
        } else {
            RetryAdvice::Unknown
        };
        return Some((
            ErrorOutput::new(
                error.code(),
                &error.to_string(),
                ErrorClass::Transport,
                retry,
            ),
            5,
        ));
    }
    if let Some(error) = error.downcast_ref::<crate::auth::AuthenticationError>() {
        return Some((
            ErrorOutput::new(
                "AUTH_FAILED",
                &error.to_string(),
                ErrorClass::Auth,
                RetryAdvice::Never,
            ),
            3,
        ));
    }
    if let Some(error) = error.downcast_ref::<crate::snu::broker::BrokerClientError>() {
        return Some((
            ErrorOutput::new(
                &error.code,
                &error.message,
                ErrorClass::Command,
                RetryAdvice::Unknown,
            ),
            1,
        ));
    }
    if let Some(error) = error.downcast_ref::<CliError>() {
        return Some((error.to_error_output(), error.exit_code()));
    }
    if error.downcast_ref::<toml::de::Error>().is_some() {
        return Some((
            ErrorOutput::new(
                "CONFIG_INVALID",
                "Configuration file is invalid; check TOML syntax and required fields.",
                ErrorClass::Config,
                RetryAdvice::Never,
            ),
            2,
        ));
    }
    if error.downcast_ref::<serde_json::Error>().is_some() {
        return Some((
            ErrorOutput::new(
                "INVALID_JSON",
                "Invalid JSON; check input syntax and expected structure.",
                ErrorClass::Validation,
                RetryAdvice::Never,
            ),
            2,
        ));
    }
    error.downcast_ref::<std::io::Error>().map(|error| {
        // OS messages contain paths which may themselves contain private data.
        (
            ErrorOutput::new(
                "IO_ERROR",
                &format!("I/O operation failed ({:?})", error.kind()),
                ErrorClass::Io,
                RetryAdvice::Unknown,
            ),
            6,
        )
    })
}

fn api_exit_code(status: u16) -> i32 {
    if status == 404 { 4 } else { 5 }
}

impl CliError {
    pub fn write_and_exit_code(&self) -> i32 {
        self.to_error_output().write();
        self.exit_code()
    }

    fn to_error_output(&self) -> ErrorOutput {
        match self {
            Self::Config { code, message } => {
                ErrorOutput::new(code, message, ErrorClass::Config, RetryAdvice::Never)
            }
            Self::Auth {
                code,
                message,
                status,
            } => {
                let mut output =
                    ErrorOutput::new(code, message, ErrorClass::Auth, RetryAdvice::Never);
                output.error.status = *status;
                output
            }
            Self::Api {
                code,
                message,
                status,
                detail,
                instance,
            } => ErrorOutput::api(code, message, *status, detail.as_deref(), instance),
            Self::Io(error) => ErrorOutput::new(
                "IO_ERROR",
                &format!("I/O operation failed ({:?})", error.kind()),
                ErrorClass::Io,
                RetryAdvice::Unknown,
            ),
            Self::Other(error) => error_output_and_exit_code(error).0,
        }
    }

    fn exit_code(&self) -> i32 {
        match self {
            Self::Config { .. } => 2,
            Self::Auth { .. } => 3,
            Self::Api { status, .. } => api_exit_code(*status),
            Self::Io(_) => 6,
            Self::Other(error) => error_output_and_exit_code(error).1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_context_preserves_classification_and_safe_details() {
        let error = anyhow::Error::new(crate::models::validation::ValidationError::new(
            "Invalid JSON data",
        ))
        .context("private payload that must not appear");
        let (output, exit) = error_output_and_exit_code(&error);
        assert_eq!(exit, 2);
        assert_eq!(output.error.class, ErrorClass::Validation);
        assert_eq!(output.error.retry, RetryAdvice::Never);
        assert!(
            !serde_json::to_string(&output)
                .unwrap()
                .contains("private payload")
        );
    }

    #[test]
    fn api_codes_exits_and_retry_advice() {
        for (status, exit, retry) in [
            (400, 5, RetryAdvice::Never),
            (401, 5, RetryAdvice::Never),
            (403, 5, RetryAdvice::Never),
            (404, 4, RetryAdvice::Never),
            (429, 5, RetryAdvice::SafeReadsOnly),
            (500, 5, RetryAdvice::SafeReadsOnly),
            (501, 5, RetryAdvice::Unknown),
            (503, 5, RetryAdvice::SafeReadsOnly),
        ] {
            let error = anyhow::Error::new(ApiError::from_status(
                status,
                "https://dev.service-now.com",
                None,
            ));
            let (output, actual_exit) = error_output_and_exit_code(&error);
            assert_eq!(actual_exit, exit);
            assert_eq!(output.error.retry, retry);
        }
    }

    #[test]
    fn messages_are_bounded_scrubbed_and_instance_is_origin_only() {
        let error = CliError::Api {
            code: "BAD_REQUEST",
            message: format!("password=secret {}", "é".repeat(1000)),
            status: 400,
            detail: Some("Authorization: Bearer private-token".into()),
            instance: "https://user:password@dev.service-now.com/private?secret=query#fragment"
                .into(),
        };
        let output = error.to_error_output();
        let json = serde_json::to_string(&output).unwrap();
        for private in [
            "password@",
            "private-token",
            "secret=query",
            "#fragment",
            "password=secret",
        ] {
            assert!(!json.contains(private));
        }
        assert_eq!(
            output.error.instance.as_deref(),
            Some("https://dev.service-now.com")
        );
        assert!(output.error.message.chars().count() < 520);
    }

    #[test]
    fn query_and_script_diagnostics_are_withheld_not_partially_scrubbed() {
        for message in [
            "sysparm_query=description=private",
            "script: gs.print('private secret');",
            "query: descriptionLIKEprivate",
        ] {
            let output =
                ErrorOutput::new("BAD_REQUEST", message, ErrorClass::Api, RetryAdvice::Never);
            assert!(!output.error.message.contains("private"));
        }
    }

    #[test]
    fn command_failures_require_manual_review() {
        for error in [
            anyhow::Error::new(ImportSetRowError { count: 1 }),
            anyhow::Error::new(ScriptExecutionError::Evaluation),
        ] {
            let (output, exit) = error_output_and_exit_code(&error);
            assert_eq!(exit, 1);
            assert_eq!(output.error.retry, RetryAdvice::ManualReview);
        }
    }

    #[test]
    fn graphql_error_uses_sanitized_output() {
        let errors = serde_json::json!([{"message": "Unknown field incidentSecret", "extensions": {"debug": "must-not-appear"}, "path": ["privateData"]}]);
        let error = anyhow::Error::new(GraphqlError::from_errors(errors.as_array().unwrap()));
        let (output, exit) = error_output_and_exit_code(&error);
        let json = serde_json::to_string(&output).unwrap();
        assert_eq!(exit, 5);
        assert_eq!(output.error.retry, RetryAdvice::ManualReview);
        assert!(json.contains("GRAPHQL_ERROR"));
        assert!(!json.contains("must-not-appear"));
        assert!(!json.contains("privateData"));
    }

    #[test]
    fn io_and_fallback_have_explicit_unknown_retry_semantics() {
        let (output, exit) = error_output_and_exit_code(
            &std::io::Error::new(std::io::ErrorKind::NotFound, "/private-path").into(),
        );
        assert_eq!(exit, 6);
        assert_eq!(output.error.class, ErrorClass::Io);
        assert!(!output.error.message.contains("private-path"));
        let (output, exit) = error_output_and_exit_code(&anyhow::anyhow!("unclassified failure"));
        assert_eq!(exit, 1);
        assert_eq!(output.error.code, "INTERNAL_ERROR");
        assert_eq!(output.error.retry, RetryAdvice::Unknown);
    }

    #[test]
    fn untrusted_codes_cannot_be_used_as_a_diagnostic_escape_hatch() {
        for code in ["token=private", "lowercase", "", &"A".repeat(65)] {
            let output =
                ErrorOutput::new(code, "failure", ErrorClass::Command, RetryAdvice::Unknown);
            assert_eq!(output.error.code, "UNKNOWN_ERROR");
        }
    }

    #[test]
    fn snu_broker_error_keeps_code() {
        let error = anyhow::Error::new(crate::snu::broker::BrokerClientError {
            code: "SNU_GATE_BLOCKED".into(),
            message: "backgroundScripts is blocked".into(),
        });
        let (output, exit) = error_output_and_exit_code(&error);
        assert_eq!(output.error.code, "SNU_GATE_BLOCKED");
        assert_eq!(exit, 1);
    }
}
