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

/// A user-supplied input file could not be read. Names the argument, never
/// the path: paths may contain private data and are kept out of error output.
#[derive(Debug, Error)]
#[error("{label} could not be read ({kind:?}); check the path passed to {argument}")]
pub struct InputFileError {
    pub label: &'static str,
    pub argument: &'static str,
    pub kind: std::io::ErrorKind,
}

/// Some records in a `data import` were rejected by ServiceNow; the
/// structured per-record report has already been printed to stdout.
#[derive(Error, Debug)]
#[error(
    "Import completed with {count} failed record(s) for table '{table}'; see the failures and hint in the import report"
)]
pub struct ImportRecordErrors {
    pub count: usize,
    pub table: String,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<crate::models::validation::ValidationIssue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issues_truncated: Option<bool>,
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
                issues: Vec::new(),
                issue_count: None,
                issues_truncated: None,
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
        output.error.detail = detail.map(safe_message).filter(|d| !d.is_empty());
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

/// Maximum Unicode scalar values retained in structured stderr diagnostics.
pub(crate) const MAX_DIAGNOSTIC_CHARS: usize = 500;

/// Returns `true` when `lower` contains a structured `query:` / `script:`
/// key-value dump (for example at the start of a diagnostic or after a
/// punctuation delimiter like `{`, `,`, `;`, `"`, `'`, or `:`), as opposed to
/// natural prose such as `"Invalid query: unknown field foo"`.
fn has_structured_colon_marker(lower: &str, marker: &str) -> bool {
    let bytes = lower.as_bytes();
    let mut offset = 0;
    while let Some(rel) = lower[offset..].find(marker) {
        let start = offset + rel;
        let prefix = &lower[..start];
        let preceded_by_word_char = prefix
            .trim_end_matches(|ch: char| ch.is_ascii_whitespace())
            .bytes()
            .next_back()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_');
        let directly_joined_word =
            start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
        if !directly_joined_word && !preceded_by_word_char {
            return true;
        }
        offset = start + marker.len();
    }
    false
}

/// Bound every diagnostic, including fallback errors; strip controls and known
/// credential patterns. Input/transport/config errors additionally discard their
/// source text entirely. Sensitive HTTP debug does not weaken this boundary.
fn safe_message(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    let has_param_or_assign = [
        "sysparm_query",
        "sysparm_script",
        "query=",
        "script=",
        "\"query\":",
        "'query':",
        "\"script\":",
        "'script':",
    ]
    .iter()
    .any(|marker| lower.contains(marker));
    if has_param_or_assign
        || has_structured_colon_marker(&lower, "query:")
        || has_structured_colon_marker(&lower, "script:")
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
        let mut output = ErrorOutput::new(
            "VALIDATION_ERROR",
            &error.to_string(),
            ErrorClass::Validation,
            RetryAdvice::Never,
        );
        // Bound stderr for agent consumption; data validate provides the full report.
        const MAX_VALIDATION_ISSUES: usize = 100;
        if !error.issues().is_empty() {
            output.error.issue_count = Some(error.issues().len());
            output.error.issues_truncated = Some(error.issues().len() > MAX_VALIDATION_ISSUES);
            output.error.issues = error
                .issues()
                .iter()
                .take(MAX_VALIDATION_ISSUES)
                .map(|issue| crate::models::validation::ValidationIssue {
                    kind: issue.kind,
                    field: issue.field.as_deref().map(safe_message),
                    record_index: issue.record_index,
                    message: safe_message(&issue.message),
                })
                .collect();
        }
        return Some((output, 2));
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
    if let Some(error) = error.downcast_ref::<InputFileError>() {
        return Some((
            ErrorOutput::new(
                "IO_ERROR",
                &error.to_string(),
                ErrorClass::Io,
                RetryAdvice::Unknown,
            ),
            6,
        ));
    }
    if let Some(error) = error.downcast_ref::<ImportRecordErrors>() {
        return Some((
            ErrorOutput::new(
                "IMPORT_RECORD_ERRORS",
                &error.to_string(),
                ErrorClass::Command,
                RetryAdvice::ManualReview,
            ),
            1,
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
    if let Some(error) = error.downcast_ref::<crate::client::NativeSessionSwitchError>() {
        let mut output = ErrorOutput::new(
            "SESSION_SWITCH_UNCERTAIN",
            &error.to_string(),
            ErrorClass::Command,
            RetryAdvice::ManualReview,
        );
        let source = match error {
            crate::client::NativeSessionSwitchError::RequestFailed(source)
            | crate::client::NativeSessionSwitchError::VerificationFailed(source) => Some(source),
            crate::client::NativeSessionSwitchError::Mismatch
            | crate::client::NativeSessionSwitchError::IncompleteContext => None,
        };
        // Render only classified sources through the existing sanitization boundary.
        // Unknown sources remain retained in the error chain, not exposed as raw text.
        if let Some((cause, _)) = source.and_then(known_error_output_and_exit_code) {
            output.error.status = cause.error.status;
            output.error.instance = cause.error.instance;
            output.error.detail = Some(safe_message(&format!(
                "Cause: {}: {}",
                cause.error.code, cause.error.message
            )));
        }
        return Some((output, 1));
    }
    if let Some(auth_error) = error.downcast_ref::<crate::auth::AuthenticationError>() {
        let mut output = ErrorOutput::new(
            "AUTH_FAILED",
            &auth_error.to_string(),
            ErrorClass::Auth,
            RetryAdvice::Never,
        );
        if let Some(api) = error.downcast_ref::<ApiError>() {
            output.error.status = Some(api.status);
            output.error.instance = url::Url::parse(&api.instance)
                .ok()
                .filter(|url| matches!(url.scheme(), "http" | "https"))
                .map(|url| url.origin().ascii_serialization());
        }
        return Some((output, 3));
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
        output.error.detail = error
            .detail
            .as_deref()
            .map(safe_message)
            .filter(|d| !d.is_empty());
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
    fn validation_issues_are_scrubbed_bounded_and_preserved_through_context() {
        use crate::models::validation::{ValidationError, ValidationIssue};
        let issues = (0..101)
            .map(|index| ValidationIssue {
                kind: "unknown_field",
                field: Some("password=private-field".to_string()),
                record_index: Some(index),
                message: format!("token=private-value {}", "é".repeat(1000)),
            })
            .collect();
        let error = anyhow::Error::new(ValidationError::new("Fix the dataset").with_issues(issues))
            .context("private context");
        let (output, exit) = error_output_and_exit_code(&error);
        assert_eq!(exit, 2);
        assert_eq!(output.error.issue_count, Some(101));
        assert_eq!(output.error.issues_truncated, Some(true));
        assert_eq!(output.error.issues.len(), 100);
        let json = serde_json::to_string(&output).unwrap();
        assert!(!json.contains("private-"));
        assert!(!json.contains("private context"));
        assert!(output.error.issues[0].message.chars().count() < 520);
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
            r#"{"query": "descriptionLIKEprivate"}"#,
            "context, script: gs.print('private')",
        ] {
            let output =
                ErrorOutput::new("BAD_REQUEST", message, ErrorClass::Api, RetryAdvice::Never);
            assert!(!output.error.message.contains("private"), "{message}");
        }
    }

    #[test]
    fn prose_diagnostics_mentioning_query_or_script_are_preserved() {
        for message in [
            "Invalid query: unknown field foo",
            "Invalid script: unexpected token at line 3",
            "GraphQL query: syntax error",
        ] {
            let output =
                ErrorOutput::new("BAD_REQUEST", message, ErrorClass::Api, RetryAdvice::Never);
            assert_eq!(output.error.message, message);
        }
    }

    #[test]
    fn authentication_error_wrapping_api_error_keeps_auth_exit_code_and_status() {
        let api_err = ApiError::from_status(
            401,
            "https://user:secret@dev.service-now.com/oauth_token.do",
            Some(r#"{"error":"invalid_client","error_description":"secret"}"#.into()),
        );
        let err = crate::auth::classify_auth_failure(anyhow::Error::new(api_err));
        let (output, exit) = error_output_and_exit_code(&err);
        assert_eq!(exit, 3);
        assert_eq!(output.error.code, "AUTH_FAILED");
        assert_eq!(output.error.class, ErrorClass::Auth);
        assert_eq!(output.error.retry, RetryAdvice::Never);
        assert_eq!(output.error.status, Some(401));
        assert_eq!(
            output.error.instance.as_deref(),
            Some("https://dev.service-now.com")
        );
        assert_eq!(
            output.error.message,
            "OAuth2 token request failed: invalid_client"
        );
    }

    #[test]
    fn graphql_error_omission_notice_survives_safe_message_bounding() {
        let errors: Vec<serde_json::Value> = (0..10)
            .map(|i| serde_json::json!({"message": format!("Error {i}: {}", "a".repeat(120))}))
            .collect();
        let error = anyhow::Error::new(GraphqlError::from_errors(&errors));
        let (output, exit) = error_output_and_exit_code(&error);
        assert_eq!(exit, 5);
        let detail = output.error.detail.as_deref().unwrap();
        assert!(detail.chars().count() <= MAX_DIAGNOSTIC_CHARS, "{detail}");
        assert!(
            detail.contains("additional GraphQL errors omitted>"),
            "{detail}"
        );
        assert!(!detail.ends_with("... <truncated>"), "{detail}");
    }

    #[test]
    fn blank_api_error_body_omits_detail_from_json() {
        let error = anyhow::Error::new(ApiError::from_status(
            500,
            "https://dev.service-now.com",
            Some("   \n\t  ".into()),
        ));
        let (output, exit) = error_output_and_exit_code(&error);
        assert_eq!(exit, 5);
        assert!(output.error.detail.is_none());
        let json = serde_json::to_string(&output).unwrap();
        assert!(!json.contains("\"detail\""), "{json}");
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
    fn switch_failures_preserve_sources_without_exposing_unclassified_text() {
        use crate::client::NativeSessionSwitchError;
        for wrap in [
            NativeSessionSwitchError::RequestFailed,
            NativeSessionSwitchError::VerificationFailed,
        ] {
            let error = anyhow::Error::new(wrap(anyhow::Error::new(ApiError::from_status(
                503,
                "https://dev.service-now.com",
                None,
            ))));
            assert!(error.chain().any(|source| source.is::<ApiError>()));
            let (output, exit) = error_output_and_exit_code(&error);
            assert_eq!(exit, 1);
            assert_eq!(output.error.status, Some(503));
            assert_eq!(output.error.retry, RetryAdvice::ManualReview);
            assert_eq!(
                output.error.instance.as_deref(),
                Some("https://dev.service-now.com")
            );

            let error = anyhow::Error::new(wrap(anyhow::anyhow!("unclassified private body")));
            let (output, _) = error_output_and_exit_code(&error);
            assert!(output.error.detail.is_none());
            assert!(
                !serde_json::to_string(&output)
                    .unwrap()
                    .contains("private body")
            );
            assert!(
                error
                    .chain()
                    .any(|source| source.to_string() == "unclassified private body")
            );
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

    /// The bundled agent skill documents the error contract in `SKILL.md` §10.
    /// Agents act on that text, so it must track the real classification.
    mod skill_contract {
        use super::*;
        use std::collections::BTreeMap;

        const SKILL_MD: &str = include_str!("../skills/snow-cli/SKILL.md");

        fn errors_section() -> &'static str {
            let start = SKILL_MD
                .find("## 10. Errors")
                .expect("SKILL.md has a `## 10. Errors` section");
            let rest = &SKILL_MD[start..];
            let end = rest[3..].find("\n## ").map_or(rest.len(), |i| i + 3);
            &rest[..end]
        }

        /// `Exit: 0 ok · 1 command/script failure · …` as exit code → label.
        fn documented_exit_codes() -> BTreeMap<i32, &'static str> {
            let line = errors_section()
                .lines()
                .find_map(|line| line.strip_prefix("Exit: "))
                .expect("§10 has an `Exit: ` line");
            line.trim_end_matches('.')
                .split(" · ")
                .map(|entry| {
                    let (code, label) = entry.split_once(' ').expect("`<code> <label>`");
                    (code.parse().expect("numeric exit code"), label)
                })
                .collect()
        }

        fn retry_name(retry: RetryAdvice) -> String {
            // Exhaustive on purpose: a new variant must be documented in §10.
            match retry {
                RetryAdvice::Never
                | RetryAdvice::SafeReadsOnly
                | RetryAdvice::ManualReview
                | RetryAdvice::Unknown => {}
            }
            serde_json::to_value(retry)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        }

        #[test]
        fn every_retry_advice_is_documented() {
            let section = errors_section();
            for retry in [
                RetryAdvice::Never,
                RetryAdvice::SafeReadsOnly,
                RetryAdvice::ManualReview,
                RetryAdvice::Unknown,
            ] {
                let name = retry_name(retry);
                assert!(
                    section.contains(&format!("`{name}`"))
                        || section.contains(&format!("`retry: {name}`")),
                    "SKILL.md §10 does not document retry `{name}`"
                );
            }
        }

        #[test]
        fn manual_review_codes_are_listed() {
            let section = errors_section();
            let manual_review_errors = [
                anyhow::Error::new(ScriptExecutionError::Evaluation),
                anyhow::Error::new(ImportRecordErrors {
                    count: 1,
                    table: "incident".into(),
                }),
                anyhow::Error::new(ImportSetRowError { count: 1 }),
                anyhow::Error::new(crate::client::NativeSessionSwitchError::Mismatch),
                anyhow::Error::new(GraphqlError { detail: None }),
            ];
            for error in manual_review_errors {
                let (output, _) = error_output_and_exit_code(&error);
                assert_eq!(output.error.retry, RetryAdvice::ManualReview);
                let code = output.error.code;
                assert!(
                    section.contains(&format!("`{code}`")),
                    "SKILL.md §10 must list `{code}` under `manual_review`"
                );
            }
        }

        #[test]
        fn exit_codes_match_their_documented_labels() {
            let documented = documented_exit_codes();
            assert_eq!(
                documented.keys().copied().collect::<Vec<_>>(),
                (0..=7).collect::<Vec<_>>(),
                "SKILL.md §10 should list exit codes 0 through 7"
            );
            let policy = anyhow::Error::new(crate::policy::PolicyError {
                operation: "table create".into(),
                mode: crate::policy::PolicyMode::ReadOnly,
                capability: crate::policy::CommandCapability::RemoteWrite,
                reason: "read-only".into(),
            });
            let validation =
                anyhow::Error::new(crate::models::validation::ValidationError::new("bad input"));
            // (exit code the CLI returns, word the documented label must contain)
            let cases = [
                (
                    error_output_and_exit_code(&anyhow::Error::new(
                        ScriptExecutionError::Evaluation,
                    ))
                    .1,
                    "script",
                ),
                (error_output_and_exit_code(&validation).1, "validation"),
                (
                    CliError::Config {
                        code: "CONFIG_NOT_FOUND",
                        message: String::new(),
                    }
                    .exit_code(),
                    "config",
                ),
                (
                    CliError::Auth {
                        code: "AUTH_FAILED",
                        message: String::new(),
                        status: None,
                    }
                    .exit_code(),
                    "credentials",
                ),
                (api_exit_code(404), "404"),
                (api_exit_code(403), "403"),
                (api_exit_code(400), "400"),
                (
                    CliError::Io(std::io::Error::other("disk")).exit_code(),
                    "I/O",
                ),
                (error_output_and_exit_code(&policy).1, "read-only"),
            ];
            for (exit, word) in cases {
                let label = documented
                    .get(&exit)
                    .unwrap_or_else(|| panic!("exit {exit} is not documented in SKILL.md §10"));
                assert!(
                    label.contains(word),
                    "SKILL.md §10 labels exit {exit} as `{label}`, expected it to mention `{word}`"
                );
            }
        }
    }
}
