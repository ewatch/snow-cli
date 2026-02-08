use async_trait::async_trait;
use http::HeaderMap;

use crate::auth::Authenticator;
use crate::config::credentials;
use crate::config::{AuthMethod, Profile};

/// Basic authentication using username and password.
pub struct BasicAuth {
    username: String,
    profile_name: String,
}

impl BasicAuth {
    pub fn new(profile: &Profile) -> anyhow::Result<Self> {
        let username = profile.username.clone().ok_or_else(|| {
            anyhow::anyhow!("Basic auth requires a username in the profile configuration")
        })?;

        Ok(Self {
            username,
            // We use the instance as a proxy for profile name here.
            // In a full implementation, the profile name would be passed separately.
            profile_name: profile.instance.clone(),
        })
    }
}

#[async_trait]
impl Authenticator for BasicAuth {
    async fn authenticate(&self) -> anyhow::Result<HeaderMap> {
        let password =
            credentials::get_credential(&self.profile_name, "password")?.ok_or_else(|| {
                anyhow::anyhow!(
                    "No password found in keychain for profile. Run `snow-cli auth login` first."
                )
            })?;

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
