use thiserror::Error;

/// A deterministic rejection of local input before execution.
/// Messages must be authored guidance, never raw payloads, queries, or scripts.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct ValidationError {
    message: String,
}

impl ValidationError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}
