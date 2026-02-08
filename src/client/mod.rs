pub mod error;
pub mod pagination;

use reqwest::Client;

/// High-level HTTP client for ServiceNow API interactions.
///
/// Wraps `reqwest::Client` with authentication, pagination,
/// and error mapping.
pub struct SnowClient {
    http: Client,
    base_url: String,
    authenticator: Box<dyn crate::auth::Authenticator>,
}

impl SnowClient {
    /// Create a new client for the given instance URL and authenticator.
    pub fn new(
        base_url: String,
        authenticator: Box<dyn crate::auth::Authenticator>,
    ) -> anyhow::Result<Self> {
        let http = Client::builder()
            .user_agent(format!("snow-cli/{}", env!("CARGO_PKG_VERSION")))
            .build()?;

        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            authenticator,
        })
    }

    /// Get the base URL for this client.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get a reference to the underlying reqwest client.
    pub fn http(&self) -> &Client {
        &self.http
    }

    /// Get a reference to the authenticator.
    pub fn authenticator(&self) -> &dyn crate::auth::Authenticator {
        self.authenticator.as_ref()
    }
}
