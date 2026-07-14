use serde::Serialize;
use thiserror::Error;

use crate::client::error::{ApiError, GraphqlError};

/// Top-level error type for the CLI.
///
/// All variants serialize to a structured JSON error on stderr.
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

/// JSON-serializable error format written to stderr.
#[derive(Debug, Serialize)]
pub struct ErrorOutput {
    pub error: ErrorBody,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
}

pub fn write_anyhow_error_and_exit_code(error: anyhow::Error) -> i32 {
    if let Some((output, exit_code)) = known_error_output_and_exit_code(&error) {
        if let Ok(json) = serde_json::to_string(&output) {
            eprintln!("{json}");
        } else {
            eprintln!("{error}");
        }
        return exit_code;
    }

    if let Some(io_error) = error.downcast_ref::<std::io::Error>() {
        return CliError::Io(std::io::Error::new(io_error.kind(), io_error.to_string()))
            .write_and_exit_code();
    }

    CliError::Other(error).write_and_exit_code()
}

fn known_error_output_and_exit_code(error: &anyhow::Error) -> Option<(ErrorOutput, i32)> {
    if let Some(policy_error) = error.downcast_ref::<crate::policy::PolicyError>() {
        return Some((
            ErrorOutput {
                error: ErrorBody {
                    code: policy_error.code().to_string(),
                    message: policy_error.to_string(),
                    status: None,
                    detail: Some(format!(
                        "mode={:?}; capability={}",
                        policy_error.mode,
                        policy_error.capability.as_str()
                    )),
                    instance: None,
                },
            },
            7,
        ));
    }

    if let Some(api_error) = error.downcast_ref::<ApiError>() {
        let exit_code = if api_error.status == 404 { 4 } else { 5 };
        return Some((
            ErrorOutput {
                error: ErrorBody {
                    code: api_error.code.clone(),
                    message: api_error.message.clone(),
                    status: Some(api_error.status),
                    detail: api_error.detail.clone(),
                    instance: Some(api_error.instance.clone()),
                },
            },
            exit_code,
        ));
    }

    error.downcast_ref::<GraphqlError>().map(|graphql_error| {
        (
            ErrorOutput {
                error: ErrorBody {
                    code: graphql_error.code().to_string(),
                    message: graphql_error.message().to_string(),
                    status: None,
                    detail: graphql_error.detail.clone(),
                    instance: None,
                },
            },
            5,
        )
    })
}

impl CliError {
    /// Write this error as structured JSON to stderr and return the exit code.
    pub fn write_and_exit_code(&self) -> i32 {
        let output = self.to_error_output();
        if let Ok(json) = serde_json::to_string(&output) {
            eprintln!("{json}");
        } else {
            eprintln!("{self}");
        }
        self.exit_code()
    }

    fn to_error_output(&self) -> ErrorOutput {
        match self {
            CliError::Config { code, message } => ErrorOutput {
                error: ErrorBody {
                    code: code.to_string(),
                    message: message.clone(),
                    status: None,
                    detail: None,
                    instance: None,
                },
            },
            CliError::Auth {
                code,
                message,
                status,
            } => ErrorOutput {
                error: ErrorBody {
                    code: code.to_string(),
                    message: message.clone(),
                    status: *status,
                    detail: None,
                    instance: None,
                },
            },
            CliError::Api {
                code,
                message,
                status,
                detail,
                instance,
            } => ErrorOutput {
                error: ErrorBody {
                    code: code.to_string(),
                    message: message.clone(),
                    status: Some(*status),
                    detail: detail.clone(),
                    instance: Some(instance.clone()),
                },
            },
            CliError::Io(e) => ErrorOutput {
                error: ErrorBody {
                    code: "IO_ERROR".to_string(),
                    message: e.to_string(),
                    status: None,
                    detail: None,
                    instance: None,
                },
            },
            CliError::Other(e) => ErrorOutput {
                error: ErrorBody {
                    code: "INTERNAL_ERROR".to_string(),
                    message: e.to_string(),
                    status: None,
                    detail: None,
                    instance: None,
                },
            },
        }
    }

    fn exit_code(&self) -> i32 {
        match self {
            CliError::Config { .. } => 2,
            CliError::Auth { .. } => 3,
            CliError::Api { status, .. } => {
                if *status == 404 {
                    4
                } else {
                    5
                }
            }
            CliError::Io(_) => 6,
            CliError::Other(_) => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_error_serializes_to_json() {
        let err = CliError::Config {
            code: "CONFIG_NOT_FOUND",
            message: "Config file not found".to_string(),
        };
        let output = err.to_error_output();
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("CONFIG_NOT_FOUND"));
        assert!(json.contains("Config file not found"));
    }

    #[test]
    fn test_graphql_error_uses_structured_sanitized_output() {
        let errors = serde_json::json!([{
            "message": "Unknown field incidentSecret",
            "extensions": {"debug": "must-not-appear"},
            "path": ["privateData"]
        }]);
        let graphql_error = GraphqlError::from_errors(errors.as_array().unwrap());
        let error = anyhow::Error::new(graphql_error);
        let (output, exit_code) = known_error_output_and_exit_code(&error).unwrap();
        let json = serde_json::to_string(&output).unwrap();

        assert_eq!(exit_code, 5);
        assert!(json.contains("GRAPHQL_ERROR"));
        assert!(json.contains("Unknown field incidentSecret"));
        assert!(!json.contains("must-not-appear"));
        assert!(!json.contains("privateData"));
    }

    #[test]
    fn test_api_error_includes_all_fields() {
        let err = CliError::Api {
            code: "NOT_FOUND",
            message: "Table not found".to_string(),
            status: 404,
            detail: Some("No table named 'foobar'".to_string()),
            instance: "https://dev.service-now.com".to_string(),
        };
        let output = err.to_error_output();
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("NOT_FOUND"));
        assert!(json.contains("404"));
        assert!(json.contains("foobar"));
        assert!(json.contains("dev.service-now.com"));
    }

    #[test]
    fn test_exit_codes() {
        assert_eq!(
            CliError::Config {
                code: "X",
                message: String::new()
            }
            .exit_code(),
            2
        );
        assert_eq!(
            CliError::Auth {
                code: "X",
                message: String::new(),
                status: None
            }
            .exit_code(),
            3
        );
        assert_eq!(
            CliError::Api {
                code: "X",
                message: String::new(),
                status: 404,
                detail: None,
                instance: String::new()
            }
            .exit_code(),
            4
        );
    }
}
