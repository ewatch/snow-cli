use async_trait::async_trait;
use http::HeaderMap;

use crate::auth::Authenticator;
use crate::config::credentials;
use crate::config::profile::{AuthMethod, Profile};

/// Source of the API token — keychain or directly injected (for testing).
#[derive(Debug)]
enum TokenSource {
    /// Look up token from keychain/env at runtime.
    Keychain { profile_name: String },
    /// Use a directly provided token (for testing).
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
}

impl ApiKeyAuth {
    pub fn new(profile: &Profile) -> anyhow::Result<Self> {
        Ok(Self {
            token_source: TokenSource::Keychain {
                profile_name: profile.instance.clone(),
            },
        })
    }

    /// Create an ApiKeyAuth with a directly injected token (for testing).
    #[cfg(test)]
    pub fn new_with_token(token: String) -> Self {
        Self {
            token_source: TokenSource::Direct { token },
        }
    }

    /// Retrieve the token from the configured source.
    fn get_token(&self) -> anyhow::Result<String> {
        match &self.token_source {
            TokenSource::Direct { token } => Ok(token.clone()),
            TokenSource::Keychain { profile_name } => {
                credentials::get_credential(profile_name, "api_token")?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "No API token found in keychain for profile. \
                         Run `snow-cli auth login --token <token>` first."
                    )
                })
            }
        }
    }
}

#[async_trait]
impl Authenticator for ApiKeyAuth {
    async fn authenticate(&self) -> anyhow::Result<HeaderMap> {
        let token = self.get_token()?;

        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            format!("Bearer {token}").parse()?,
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
            cert_path: None,
            key_path: None,
        };
        let auth = ApiKeyAuth::new(&profile);
        assert!(auth.is_ok());
    }
}
