use async_trait::async_trait;
use http::HeaderMap;
use tokio::sync::OnceCell;

use crate::auth::Authenticator;
use crate::config::credentials;
use crate::config::profile::{AuthMethod, Profile};
use crate::models::secret::Secret;

/// Source of the API token — keychain or directly injected (for testing).
#[derive(Debug)]
enum TokenSource {
    /// Look up token from keychain/env at runtime.
    Keychain { profile_name: String },
    /// Use a directly provided token (for testing).
    #[cfg(test)]
    Direct { token: String },
}

/// API Key / Token authentication.
///
/// Reads a pre-existing API token from the OS keychain and sends it
/// as a `Authorization: Bearer <token>` header. No refresh capability —
/// users must manually rotate tokens.
#[derive(Debug)]
pub struct ApiKeyAuth {
    /// Where to get the token from.
    token_source: TokenSource,
    /// Cached `Bearer <token>` Authorization header value, resolved once per client.
    cached_header: OnceCell<Secret<String>>,
}

impl ApiKeyAuth {
    pub fn new(profile_name: &str, _profile: &Profile) -> anyhow::Result<Self> {
        Ok(Self {
            token_source: TokenSource::Keychain {
                profile_name: profile_name.to_string(),
            },
            cached_header: OnceCell::new(),
        })
    }

    /// Create an ApiKeyAuth with a directly injected token (for testing).
    #[cfg(test)]
    pub fn new_with_token(token: String) -> Self {
        Self {
            token_source: TokenSource::Direct { token },
            cached_header: OnceCell::new(),
        }
    }

    /// Retrieve the token from the configured source without blocking Tokio worker threads.
    async fn get_token(&self) -> anyhow::Result<String> {
        match &self.token_source {
            #[cfg(test)]
            TokenSource::Direct { token } => Ok(token.clone()),
            TokenSource::Keychain { profile_name } => {
                let profile = profile_name.clone();
                let lookup_profile = profile.clone();
                let stored = tokio::task::spawn_blocking(move || {
                    credentials::get_credential(&lookup_profile, "api_token")
                })
                .await
                .map_err(|error| anyhow::anyhow!("Credential lookup task failed: {error}"))??;
                stored.ok_or_else(|| {
                    crate::auth::AuthenticationError::new(format!(
                        "No API token found for profile '{profile}'. \
                         Run `snow-cli auth login --profile {profile} --token <token>` first."
                    ))
                    .into()
                })
            }
        }
    }
}

#[async_trait]
impl Authenticator for ApiKeyAuth {
    async fn authenticate(&self) -> anyhow::Result<HeaderMap> {
        let header_value = self
            .cached_header
            .get_or_try_init(|| async {
                let token = self.get_token().await?;
                Ok::<_, anyhow::Error>(Secret::new(format!("Bearer {token}")))
            })
            .await?;

        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            header_value.expose_secret().parse()?,
        );
        Ok(headers)
    }

    async fn refresh(&mut self) -> anyhow::Result<bool> {
        // API key auth does not support refresh.
        Ok(false)
    }

    fn auth_type(&self) -> AuthMethod {
        AuthMethod::ApiKey
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_authenticate_returns_bearer_header() {
        let auth = ApiKeyAuth::new_with_token("my-api-token-123".to_string());
        let headers = auth.authenticate().await.unwrap();

        assert_eq!(
            headers.get("authorization").unwrap().to_str().unwrap(),
            "Bearer my-api-token-123"
        );
    }

    #[tokio::test]
    async fn test_authenticate_with_complex_token() {
        let auth =
            ApiKeyAuth::new_with_token("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.abc.xyz".to_string());
        let headers = auth.authenticate().await.unwrap();

        assert_eq!(
            headers.get("authorization").unwrap().to_str().unwrap(),
            "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.abc.xyz"
        );
    }

    #[tokio::test]
    async fn test_refresh_returns_false() {
        let mut auth = ApiKeyAuth::new_with_token("token".to_string());
        let result = auth.refresh().await.unwrap();
        assert!(!result);
    }

    #[test]
    fn test_auth_type_returns_api_key() {
        let auth = ApiKeyAuth::new_with_token("token".to_string());
        assert_eq!(auth.auth_type(), AuthMethod::ApiKey);
    }

    #[test]
    fn test_new_from_profile() {
        let profile = Profile {
            instance: "https://test.service-now.com".to_string(),
            auth_method: AuthMethod::ApiKey,
            username: None,
            client_id: None,
            oauth_grant_type: None,
            oauth_scope: None,
            oauth_redirect_host: None,
            oauth_redirect_port: None,
            oauth_redirect_path: None,
            cert_path: None,
            key_path: None,
            sso_login_url: None,
        };
        let auth = ApiKeyAuth::new("test", &profile);
        assert!(auth.is_ok());
    }

    #[tokio::test]
    async fn test_cached_header_reused_and_redacted_in_debug() {
        let auth = ApiKeyAuth::new_with_token("api-secret-token-xyz".to_string());
        let first = auth.authenticate().await.unwrap();
        let second = auth.authenticate().await.unwrap();
        assert_eq!(
            first.get(http::header::AUTHORIZATION),
            second.get(http::header::AUTHORIZATION)
        );
        let debug = format!("{auth:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("Bearer api-secret-token-xyz"));
    }
}
