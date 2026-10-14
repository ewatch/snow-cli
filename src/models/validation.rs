use serde::Serialize;
use thiserror::Error;

/// Actionable validation metadata, never the rejected record's values.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationIssue {
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_index: Option<usize>,
    pub message: String,
}

/// A deterministic rejection of local input before execution.
/// Messages must be authored guidance, never raw payloads, queries, or scripts.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct ValidationError {
    message: String,
    issues: Vec<ValidationIssue>,
}

impl ValidationError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            issues: Vec::new(),
        }
    }

    pub fn with_issues(mut self, issues: Vec<ValidationIssue>) -> Self {
        self.issues = issues;
        self
    }

    pub fn issues(&self) -> &[ValidationIssue] {
        &self.issues
    }
}
