use thiserror::Error;

/// Configuration failures carry safe guidance, not TOML excerpts or credentials.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct ConfigError {
    pub code: &'static str,
    message: String,
}

impl ConfigError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
