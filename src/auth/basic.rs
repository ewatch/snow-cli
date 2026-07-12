use async_trait::async_trait;
use http::HeaderMap;

use crate::auth::Authenticator;
use crate::config::credentials;
use crate::config::{AuthMethod, Profile};

/// Source of the password — keychain or directly injected (for testing).
#[derive(Debug)]
enum CredentialSource {
    /// Look up password from keychain/env at runtime.
    Keychain { profile_name: String },
    /// Use a directly provided password (for testing).
    #[allow(dead_code)]
    Direct { password: String },
}

/// Basic authentication using username and password.
#[derive(Debug)]
pub struct BasicAuth {
    username: String,
    credential_source: CredentialSource,
}

impl BasicAuth {
    pub fn new(profile_name: &str, profile: &Profile) -> anyhow::Result<Self> {
        let username = profile.username.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "Basic auth requires a username in the profile configuration. \
                 Use: snow-cli profile edit {} --username <user>",
                profile_name
            )
        })?;

        Ok(Self {
            username,
            credential_source: CredentialSource::Keychain {
                profile_name: profile_name.to_string(),
            },
        })
    }

    /// Create a BasicAuth with a directly injected password (for testing).
    #[cfg(test)]
    pub fn new_direct(username: String, password: String) -> Self {
        Self {
            username,
            credential_source: CredentialSource::Direct { password },
        }
    }

    /// Retrieve the password from the configured source.
    fn get_password(&self) -> anyhow::Result<String> {
        match &self.credential_source {
            CredentialSource::Direct { password } => Ok(password.clone()),
            CredentialSource::Keychain { profile_name } => credentials::get_credential(
                profile_name,
                "password",
            )?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No password found for profile '{}'. Run `snow-cli auth login --profile {}` first.",
                    profile_name,
                    profile_name
                )
            }),
        }
    }
}

#[async_trait]
impl Authenticator for BasicAuth {
    async fn authenticate(&self) -> anyhow::Result<HeaderMap> {
        let password = self.get_password()?;

        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", self.username, password));

        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            format!("Basic {encoded}").parse()?,
        );
        Ok(headers)
    }

    async fn refresh(&mut self) -> anyhow::Result<bool> {
        // Basic auth does not support refresh.
        Ok(false)
    }

    fn auth_type(&self) -> AuthMethod {
        AuthMethod::Basic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_authenticate_returns_basic_header() {
        let auth = BasicAuth::new_direct("admin".to_string(), "password123".to_string());
        let headers = auth.authenticate().await.unwrap();

        let auth_value = headers.get("authorization").unwrap().to_str().unwrap();
        assert!(auth_value.starts_with("Basic "));

        // Decode and verify
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(auth_value.strip_prefix("Basic ").unwrap())
            .unwrap();
        let decoded_str = String::from_utf8(decoded).unwrap();
        assert_eq!(decoded_str, "admin:password123");
    }

    #[tokio::test]
    async fn test_base64_encoding_correctness() {
        let auth = BasicAuth::new_direct("user".to_string(), "pass".to_string());
        let headers = auth.authenticate().await.unwrap();

        // "user:pass" base64 = "dXNlcjpwYXNz"
        assert_eq!(
            headers.get("authorization").unwrap().to_str().unwrap(),
            "Basic dXNlcjpwYXNz"
        );
    }

    #[tokio::test]
    async fn test_special_characters_in_password() {
        let auth = BasicAuth::new_direct("admin".to_string(), "p@ss:w0rd!".to_string());
        let headers = auth.authenticate().await.unwrap();

        let auth_value = headers.get("authorization").unwrap().to_str().unwrap();
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(auth_value.strip_prefix("Basic ").unwrap())
            .unwrap();
        let decoded_str = String::from_utf8(decoded).unwrap();
        assert_eq!(decoded_str, "admin:p@ss:w0rd!");
    }

    #[tokio::test]
    async fn test_unicode_in_password() {
        let auth = BasicAuth::new_direct("admin".to_string(), "pässwörd".to_string());
        let headers = auth.authenticate().await.unwrap();

        let auth_value = headers.get("authorization").unwrap().to_str().unwrap();
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(auth_value.strip_prefix("Basic ").unwrap())
            .unwrap();
        let decoded_str = String::from_utf8(decoded).unwrap();
        assert_eq!(decoded_str, "admin:pässwörd");
    }

    #[test]
    fn test_new_requires_username() {
        let profile = Profile {
            instance: "https://test.service-now.com".to_string(),
            auth_method: AuthMethod::Basic,
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
        let result = BasicAuth::new("test", &profile);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("username"));
    }

    #[test]
    fn test_new_from_profile_with_username() {
        let profile = Profile {
            instance: "https://test.service-now.com".to_string(),
            auth_method: AuthMethod::Basic,
            username: Some("admin".to_string()),
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
        let result = BasicAuth::new("test", &profile);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_refresh_returns_false() {
        let mut auth = BasicAuth::new_direct("admin".to_string(), "pass".to_string());
        let result = auth.refresh().await.unwrap();
        assert!(!result);
    }

    #[test]
    fn test_auth_type_returns_basic() {
        let auth = BasicAuth::new_direct("admin".to_string(), "pass".to_string());
        assert_eq!(auth.auth_type(), AuthMethod::Basic);
    }
}
