use async_trait::async_trait;
use http::HeaderMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::{ClientConfig, SnowClient};

/// A mock authenticator for testing.
/// Injects a fixed Authorization header.
pub(super) struct MockAuth {
    token: String,
    refresh_count: Arc<AtomicU32>,
    refresh_succeeds: bool,
}

impl MockAuth {
    pub(super) fn new(token: &str) -> Self {
        Self {
            token: token.to_string(),
            refresh_count: Arc::new(AtomicU32::new(0)),
            refresh_succeeds: false,
        }
    }

    pub(super) fn with_refresh(mut self) -> Self {
        self.refresh_succeeds = true;
        self
    }

    pub(super) fn refresh_count(&self) -> Arc<AtomicU32> {
        self.refresh_count.clone()
    }
}

#[async_trait]
impl crate::auth::Authenticator for MockAuth {
    async fn authenticate(&self) -> anyhow::Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            format!("Bearer {}", self.token).parse().unwrap(),
        );
        Ok(headers)
    }

    async fn refresh(&mut self) -> anyhow::Result<bool> {
        self.refresh_count.fetch_add(1, Ordering::SeqCst);
        Ok(self.refresh_succeeds)
    }

    fn auth_type(&self) -> crate::config::AuthMethod {
        crate::config::AuthMethod::Basic
    }
}

pub(super) fn test_client(base_url: &str, auth: MockAuth) -> SnowClient {
    SnowClient::with_config(
        base_url.to_string(),
        Box::new(auth),
        ClientConfig::default(),
    )
    .unwrap()
}

pub(super) fn incident_table() -> crate::models::identifiers::TableName {
    "incident".parse().unwrap()
}
