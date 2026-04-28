pub mod api_key;
pub mod basic;
pub mod oauth2;
pub mod saml;

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
///
/// `profile_name` is the config profile name (e.g., "default", "dev") used
/// as the keychain lookup key. This must match the key used by `auth login`.
pub fn create_authenticator(
    profile_name: &str,
    profile: &crate::config::Profile,
) -> anyhow::Result<Box<dyn Authenticator>> {
    match profile.auth_method {
        AuthMethod::Basic => Ok(Box::new(basic::BasicAuth::new(profile_name, profile)?)),
        AuthMethod::Oauth2 => Ok(Box::new(oauth2::OAuth2Auth::new(profile_name, profile)?)),
        AuthMethod::ApiKey => Ok(Box::new(api_key::ApiKeyAuth::new(profile_name, profile)?)),
        AuthMethod::Mtls => {
            anyhow::bail!(
                "mTLS authentication is not yet implemented. See docs/PLAN.md for roadmap."
            )
        }
        AuthMethod::Saml => Ok(Box::new(saml::SamlAuth::new(profile_name, profile)?)),
    }
}
