pub mod api_key;
pub mod basic;
pub mod browser_session;
pub mod native_session;
pub mod oauth2;

use async_trait::async_trait;
use http::HeaderMap;

use crate::config::AuthMethod;

/// Safe credential guidance without credential values or keychain internals.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct AuthenticationError {
    message: String,
}

impl AuthenticationError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub(crate) fn classify_auth_failure(error: anyhow::Error) -> anyhow::Error {
    if error.downcast_ref::<AuthenticationError>().is_some() {
        error
    } else {
        error.context(AuthenticationError::new(
            "Authentication failed; check stored credentials or run snow-cli auth login.",
        ))
    }
}

/// Trait that all authentication methods implement.
#[async_trait]
pub trait Authenticator: Send + Sync {
    /// Returns HTTP headers to attach to a request for authentication.
    async fn authenticate(&self) -> anyhow::Result<HeaderMap>;

    /// Refresh credentials if supported (e.g., OAuth token refresh).
    /// Returns Ok(true) if refresh succeeded, Ok(false) if not applicable.
    async fn refresh(&mut self) -> anyhow::Result<bool>;

    /// Observe session-cookie rotation after a same-instance response.
    async fn observe_response_headers(&mut self, _headers: &HeaderMap) -> anyhow::Result<()> {
        Ok(())
    }

    fn native_session_credentials(&self) -> Option<crate::config::native_session::NativeSession> {
        None
    }
    async fn update_native_csrf(&mut self, _token: &str) -> anyhow::Result<()> {
        Ok(())
    }

    /// Returns the authentication method type.
    fn auth_type(&self) -> AuthMethod;
}

/// Validate that a profile has all required configuration fields for its auth method.
///
/// This does NOT check whether credentials (passwords, tokens) are stored in the
/// keychain — it only validates the static profile configuration (e.g., that a
/// `username` is set for basic auth, or a `client_id` is set for OAuth2).
///
/// Call this before creating an authenticator to give clear, early error messages.
pub fn validate_profile_config(
    profile_name: &str,
    profile: &crate::config::Profile,
) -> anyhow::Result<()> {
    validate_profile_config_inner(profile_name, profile).map_err(|error| {
        crate::config::error::ConfigError::new("CONFIG_INVALID", error.to_string()).into()
    })
}

fn validate_profile_config_inner(
    profile_name: &str,
    profile: &crate::config::Profile,
) -> anyhow::Result<()> {
    match &profile.auth_method {
        AuthMethod::Basic => {
            if profile.username.is_none() {
                anyhow::bail!(
                    "Profile '{}' uses basic auth but is missing a username. \
                     Fix it with: snow-cli profile edit {} --username <user>",
                    profile_name,
                    profile_name
                );
            }
        }
        AuthMethod::Oauth2 => {
            if profile.client_id.is_none() {
                anyhow::bail!(
                    "Profile '{}' uses OAuth2 but is missing a client_id. \
                     Fix it with: snow-cli profile edit {} --client-id <id>",
                    profile_name,
                    profile_name
                );
            }

            let grant_type = profile
                .oauth_grant_type
                .clone()
                .unwrap_or(crate::config::profile::OAuthGrantType::ClientCredentials);

            if grant_type == crate::config::profile::OAuthGrantType::Password
                && profile.username.is_none()
            {
                anyhow::bail!(
                    "Profile '{}' uses OAuth2 password grant but is missing a username. \
                     Fix it with: snow-cli profile edit {} --username <user>",
                    profile_name,
                    profile_name
                );
            }
        }
        AuthMethod::ApiKey => {
            // No required profile config fields — only the stored api_token matters,
            // which is validated when credentials are accessed.
        }
        AuthMethod::Mtls => {
            if profile.cert_path.is_none() {
                anyhow::bail!(
                    "Profile '{}' uses mTLS but is missing cert_path. \
                     Fix it with: snow-cli profile edit {} --cert-path <path>",
                    profile_name,
                    profile_name
                );
            }
            if profile.key_path.is_none() {
                anyhow::bail!(
                    "Profile '{}' uses mTLS but is missing key_path. \
                     Fix it with: snow-cli profile edit {} --key-path <path>",
                    profile_name,
                    profile_name
                );
            }
        }
        AuthMethod::NativeSession => {}
        AuthMethod::BrowserSession => {
            // No required profile config fields. The session cookie is provided at
            // runtime via the SNOW_SESSION_COOKIE environment variable.
        }
    }

    Ok(())
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
    // Validate profile config first so errors are clear and actionable.
    validate_profile_config(profile_name, profile)?;

    match profile.auth_method {
        AuthMethod::NativeSession => Ok(Box::new(native_session::NativeSessionAuth::stored(
            profile_name,
            &profile.instance,
        )?)),
        AuthMethod::Basic => Ok(Box::new(basic::BasicAuth::new(profile_name, profile)?)),
        AuthMethod::Oauth2 => Ok(Box::new(oauth2::OAuth2Auth::new(profile_name, profile)?)),
        AuthMethod::ApiKey => Ok(Box::new(api_key::ApiKeyAuth::new(profile_name, profile)?)),
        AuthMethod::Mtls => {
            anyhow::bail!(
                "mTLS authentication is not yet implemented. See docs/PLAN.md for roadmap."
            )
        }
        AuthMethod::BrowserSession => Ok(Box::new(browser_session::BrowserSessionAuth::new(
            profile_name,
            profile,
        )?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::profile::{OAuthGrantType, Profile};

    fn make_profile(auth_method: AuthMethod) -> Profile {
        Profile {
            instance: "https://test.service-now.com".to_string(),
            auth_method,
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
        }
    }

    #[test]
    fn test_validate_basic_requires_username() {
        let profile = make_profile(AuthMethod::Basic);
        let err = validate_profile_config("test", &profile)
            .unwrap_err()
            .to_string();
        assert!(err.contains("username"));
        assert!(err.contains("test"));
    }

    #[test]
    fn test_validate_basic_passes_with_username() {
        let mut profile = make_profile(AuthMethod::Basic);
        profile.username = Some("admin".to_string());
        assert!(validate_profile_config("test", &profile).is_ok());
    }

    #[test]
    fn test_validate_oauth2_requires_client_id() {
        let profile = make_profile(AuthMethod::Oauth2);
        let err = validate_profile_config("test", &profile)
            .unwrap_err()
            .to_string();
        assert!(err.contains("client_id"));
    }

    #[test]
    fn test_validate_oauth2_password_grant_requires_username() {
        let mut profile = make_profile(AuthMethod::Oauth2);
        profile.client_id = Some("client123".to_string());
        profile.oauth_grant_type = Some(OAuthGrantType::Password);
        let err = validate_profile_config("test", &profile)
            .unwrap_err()
            .to_string();
        assert!(err.contains("username"));
    }

    #[test]
    fn test_validate_oauth2_client_credentials_no_username_required() {
        let mut profile = make_profile(AuthMethod::Oauth2);
        profile.client_id = Some("client123".to_string());
        profile.oauth_grant_type = Some(OAuthGrantType::ClientCredentials);
        assert!(validate_profile_config("test", &profile).is_ok());
    }

    #[test]
    fn test_validate_api_key_always_passes() {
        let profile = make_profile(AuthMethod::ApiKey);
        assert!(validate_profile_config("test", &profile).is_ok());
    }

    #[test]
    fn test_validate_browser_session_always_passes() {
        let profile = make_profile(AuthMethod::BrowserSession);
        assert!(validate_profile_config("test", &profile).is_ok());
    }

    #[test]
    fn test_validate_mtls_requires_cert_and_key_paths() {
        let profile = make_profile(AuthMethod::Mtls);
        let err = validate_profile_config("test", &profile)
            .unwrap_err()
            .to_string();
        assert!(err.contains("cert_path"));
    }

    #[test]
    fn test_validate_mtls_requires_key_path_too() {
        let mut profile = make_profile(AuthMethod::Mtls);
        profile.cert_path = Some(std::path::PathBuf::from("/tmp/cert.pem"));
        let err = validate_profile_config("test", &profile)
            .unwrap_err()
            .to_string();
        assert!(err.contains("key_path"));
    }
}
