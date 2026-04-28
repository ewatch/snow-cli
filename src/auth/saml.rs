use async_trait::async_trait;
use http::HeaderMap;

use crate::auth::Authenticator;
use crate::config::credentials;
use crate::config::profile::{AuthMethod, Profile};

#[derive(Debug)]
enum CookieSource {
    Keychain {
        profile_name: String,
    },
    #[cfg(test)]
    Direct {
        cookie_header: String,
    },
}

/// Browser-assisted SAML/SSO authentication.
///
/// The CLI reuses an authenticated ServiceNow Cookie header captured after a
/// browser login. This keeps runtime requests simple and works for both API and
/// UI endpoints that rely on the ServiceNow web session.
#[derive(Debug)]
pub struct SamlAuth {
    cookie_source: CookieSource,
}

impl SamlAuth {
    pub fn new(profile_name: &str, _profile: &Profile) -> anyhow::Result<Self> {
        Ok(Self {
            cookie_source: CookieSource::Keychain {
                profile_name: profile_name.to_string(),
            },
        })
    }

    #[cfg(test)]
    pub fn new_with_cookie(cookie_header: String) -> Self {
        Self {
            cookie_source: CookieSource::Direct { cookie_header },
        }
    }

    fn get_cookie_header(&self) -> anyhow::Result<String> {
        match &self.cookie_source {
            CookieSource::Keychain { profile_name } => {
                credentials::get_credential(profile_name, "session_cookie")?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "No ServiceNow session cookie found for profile '{}'. Run `snow-cli auth login --profile {}` first.",
                        profile_name,
                        profile_name
                    )
                })
            }
            #[cfg(test)]
            CookieSource::Direct { cookie_header } => Ok(cookie_header.clone()),
        }
    }
}

#[async_trait]
impl Authenticator for SamlAuth {
    async fn authenticate(&self) -> anyhow::Result<HeaderMap> {
        let cookie_header = self.get_cookie_header()?;
        let mut headers = HeaderMap::new();
        headers.insert(http::header::COOKIE, cookie_header.parse()?);
        Ok(headers)
    }

    async fn refresh(&mut self) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn auth_type(&self) -> AuthMethod {
        AuthMethod::Saml
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_authenticate_returns_cookie_header() {
        let auth = SamlAuth::new_with_cookie(
            "JSESSIONID=session123; glide_user_route=route456".to_string(),
        );
        let headers = auth.authenticate().await.unwrap();

        assert_eq!(
            headers.get("cookie").unwrap().to_str().unwrap(),
            "JSESSIONID=session123; glide_user_route=route456"
        );
    }

    #[tokio::test]
    async fn test_refresh_returns_false() {
        let mut auth = SamlAuth::new_with_cookie("JSESSIONID=session123".to_string());
        assert!(!auth.refresh().await.unwrap());
    }

    #[test]
    fn test_auth_type_returns_saml() {
        let auth = SamlAuth::new_with_cookie("JSESSIONID=session123".to_string());
        assert_eq!(auth.auth_type(), AuthMethod::Saml);
    }
}
