pub mod basic;

use async_trait::async_trait;
use http::HeaderMap;

use crate::config::AuthMethod;

/// Trait that all authentication methods implement.
#[async_trait]
pub trait Authenticator: Send + Sync {
    /// Returns HTTP headers to attach to a request for authentication.
    async fn authenticate(&self) -> anyhow::Result<HeaderMap>;

    /// Refresh credentials if supported (e.g., OAuth token refresh).
    /// Returns Ok(true) if refresh succeeded, Ok(false) if not applicable.
    async fn refresh(&mut self) -> anyhow::Result<bool>;

    /// Returns the authentication method type.
    fn auth_type(&self) -> AuthMethod;
}

/// Create an authenticator based on the profile's auth method.
///
/// This is a factory function that dispatches to the correct implementation
/// based on the `auth_method` field in the profile configuration.
pub fn create_authenticator(
    profile: &crate::config::Profile,
) -> anyhow::Result<Box<dyn Authenticator>> {
    match profile.auth_method {
        AuthMethod::Basic => Ok(Box::new(basic::BasicAuth::new(profile)?)),
        AuthMethod::Oauth2 => {
            todo!("OAuth2 authenticator not yet implemented")
        }
        AuthMethod::ApiKey => {
            todo!("API key authenticator not yet implemented")
        }
        AuthMethod::Mtls => {
            todo!("mTLS authenticator not yet implemented")
        }
        AuthMethod::Saml => {
            todo!("SAML authenticator not yet implemented")
        }
    }
}
