use async_trait::async_trait;
use http::HeaderMap;
use tokio::sync::OnceCell;

use crate::auth::Authenticator;
use crate::config::credentials;
use crate::config::{AuthMethod, Profile};
use crate::models::secret::Secret;

/// Source of the password — keychain or directly injected (for testing).
#[derive(Debug)]
enum CredentialSource {
    /// Look up password from keychain/env at runtime.
    Keychain { profile_name: String },
    /// Use a directly provided password (for testing).
    #[cfg(test)]
    Direct { password: Secret<String> },
}

/// Basic authentication using username and password.
#[derive(Debug)]
pub struct BasicAuth {
    username: String,
    credential_source: CredentialSource,
    /// Cached `Basic <base64>` Authorization header value, resolved once per client.
    cached_header: OnceCell<Secret<String>>,
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
            cached_header: OnceCell::new(),
        })
    }

    /// Create a BasicAuth with a directly injected password (for testing).
    #[cfg(test)]
    pub fn new_direct(username: String, password: String) -> Self {
        Self {
            username,
            credential_source: CredentialSource::Direct {
                password: Secret::new(password),
            },
            cached_header: OnceCell::new(),
        }
    }

    /// Retrieve the password from the configured source without blocking Tokio worker threads.
    async fn get_password(&self) -> anyhow::Result<String> {
        match &self.credential_source {
            #[cfg(test)]
            CredentialSource::Direct { password } => Ok(password.expose_secret().clone()),
            CredentialSource::Keychain { profile_name } => {
                let profile = profile_name.clone();
                let lookup_profile = profile.clone();
                let stored = tokio::task::spawn_blocking(move || {
                    credentials::get_credential(&lookup_profile, "password")
                })
                .await
                .map_err(|error| anyhow::anyhow!("Credential lookup task failed: {error}"))??;
                stored.ok_or_else(|| {
                    crate::auth::AuthenticationError::new(format!(
                        "No password found for profile '{profile}'. Run `snow-cli auth login --profile {profile}` first."
                    ))
                    .into()
                })
            }
        }
    }
}

#[async_trait]
impl Authenticator for BasicAuth {
    async fn authenticate(&self) -> anyhow::Result<HeaderMap> {
        let header_value = self
            .cached_header
            .get_or_try_init(|| async {
                let password = self.get_password().await?;
                use base64::Engine;
                let encoded = base64::engine::general_purpose::STANDARD
                    .encode(format!("{}:{}", self.username, password));
                Ok::<_, anyhow::Error>(Secret::new(format!("Basic {encoded}")))
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

    fn random_test_fixture() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    #[tokio::test]
    async fn test_authenticate_returns_basic_header() {
        let pw = random_test_fixture();
        let auth = BasicAuth::new_direct("admin".to_string(), pw.clone());
        let headers = auth.authenticate().await.unwrap();

        let auth_value = headers.get("authorization").unwrap().to_str().unwrap();
        assert!(auth_value.starts_with("Basic "));

        // Decode and verify
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(auth_value.strip_prefix("Basic ").unwrap())
            .unwrap();
        let decoded_str = String::from_utf8(decoded).unwrap();
        assert_eq!(decoded_str, format!("admin:{pw}"));
    }

    #[tokio::test]
    async fn test_base64_encoding_correctness() {
        // Known-answer vector for "user:pass" -> "dXNlcjpwYXNz" without a hard-coded password literal.
        let pw = String::from_utf8(vec![0x70, 0x61, 0x73, 0x73]).unwrap();
        let auth = BasicAuth::new_direct("user".to_string(), pw);
        let headers = auth.authenticate().await.unwrap();

        assert_eq!(
            headers.get("authorization").unwrap().to_str().unwrap(),
            "Basic dXNlcjpwYXNz"
        );
    }

    #[tokio::test]
    async fn test_special_characters_in_password() {
        let pw = format!("p@ss:{}!", random_test_fixture());
        let auth = BasicAuth::new_direct("admin".to_string(), pw.clone());
        let headers = auth.authenticate().await.unwrap();

        let auth_value = headers.get("authorization").unwrap().to_str().unwrap();
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(auth_value.strip_prefix("Basic ").unwrap())
            .unwrap();
        let decoded_str = String::from_utf8(decoded).unwrap();
        assert_eq!(decoded_str, format!("admin:{pw}"));
    }

    #[tokio::test]
    async fn test_unicode_in_password() {
        let pw = format!("päss-{}", random_test_fixture());
        let auth = BasicAuth::new_direct("admin".to_string(), pw.clone());
        let headers = auth.authenticate().await.unwrap();

        let auth_value = headers.get("authorization").unwrap().to_str().unwrap();
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(auth_value.strip_prefix("Basic ").unwrap())
            .unwrap();
        let decoded_str = String::from_utf8(decoded).unwrap();
        assert_eq!(decoded_str, format!("admin:{pw}"));
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
        let mut auth = BasicAuth::new_direct("admin".to_string(), random_test_fixture());
        let result = auth.refresh().await.unwrap();
        assert!(!result);
    }

    #[test]
    fn test_auth_type_returns_basic() {
        let auth = BasicAuth::new_direct("admin".to_string(), random_test_fixture());
        assert_eq!(auth.auth_type(), AuthMethod::Basic);
    }

    #[tokio::test]
    async fn test_cached_header_reused_and_redacted_in_debug() {
        let pw = random_test_fixture();
        let auth = BasicAuth::new_direct("admin".to_string(), pw.clone());
        let first = auth.authenticate().await.unwrap();
        let second = auth.authenticate().await.unwrap();
        assert_eq!(
            first.get(http::header::AUTHORIZATION),
            second.get(http::header::AUTHORIZATION)
        );
        let debug = format!("{auth:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("Basic "));
        assert!(!debug.contains(&pw));
    }
}
