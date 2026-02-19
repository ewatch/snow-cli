use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use http::HeaderMap;
use tokio::sync::RwLock;

use crate::auth::Authenticator;
use crate::config::credentials;
use crate::config::profile::{AuthMethod, OAuthGrantType, Profile};

/// Cached OAuth2 token with expiry tracking.
#[derive(Debug, Clone)]
struct CachedToken {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Instant,
}

impl CachedToken {
    fn is_expired(&self) -> bool {
        // Consider expired 30 seconds early to avoid edge cases
        Instant::now() >= self.expires_at - Duration::from_secs(30)
    }
}

/// Source of credentials — either keychain/env lookup or directly injected (for testing).
#[derive(Debug, Clone)]
enum CredentialSource {
    /// Look up credentials from keychain/env at runtime.
    Keychain { profile_name: String },
    /// Use directly provided credentials (for testing).
    Direct {
        client_secret: String,
        password: Option<String>,
    },
}

/// OAuth 2.0 authenticator supporting client_credentials and password grant types.
///
/// Tokens are cached in memory and automatically refreshed when expired.
#[derive(Debug)]
pub struct OAuth2Auth {
    /// Instance base URL (e.g., https://mycompany.service-now.com)
    instance: String,
    /// OAuth client ID from config
    client_id: String,
    /// Grant type (client_credentials or password)
    grant_type: OAuthGrantType,
    /// Username (required for password grant, from config)
    username: Option<String>,
    /// Where to get client_secret and password from
    credential_source: CredentialSource,
    /// Cached token (shared across authenticate/refresh calls)
    cached_token: Arc<RwLock<Option<CachedToken>>>,
}

/// ServiceNow OAuth token response.
#[derive(Debug, serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    #[allow(dead_code)]
    token_type: Option<String>,
    expires_in: u64,
}

impl OAuth2Auth {
    pub fn new(profile_name: &str, profile: &Profile) -> anyhow::Result<Self> {
        let client_id = profile.client_id.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "OAuth2 auth requires `client_id` in the profile configuration. \
                 Use: snow-cli config set-profile {} --client-id <id>",
                profile_name
            )
        })?;

        let grant_type = profile
            .oauth_grant_type
            .clone()
            .unwrap_or(OAuthGrantType::ClientCredentials);

        if grant_type == OAuthGrantType::Password && profile.username.is_none() {
            anyhow::bail!(
                "OAuth2 password grant requires `username` in the profile configuration. \
                 Use: snow-cli config set-profile {} --username <user>",
                profile_name
            );
        }

        Ok(Self {
            instance: profile.instance.clone(),
            client_id,
            grant_type,
            username: profile.username.clone(),
            credential_source: CredentialSource::Keychain {
                profile_name: profile_name.to_string(),
            },
            cached_token: Arc::new(RwLock::new(None)),
        })
    }

    /// Create an OAuth2Auth with directly injected credentials (for testing).
    ///
    /// Bypasses keychain lookups entirely.
    #[cfg(test)]
    pub fn new_with_credentials(
        instance: String,
        client_id: String,
        grant_type: OAuthGrantType,
        username: Option<String>,
        client_secret: String,
        password: Option<String>,
    ) -> Self {
        Self {
            instance,
            client_id,
            grant_type,
            username,
            credential_source: CredentialSource::Direct {
                client_secret,
                password,
            },
            cached_token: Arc::new(RwLock::new(None)),
        }
    }

    /// Token endpoint URL for ServiceNow.
    fn token_url(&self) -> String {
        format!("{}/oauth_token.do", self.instance.trim_end_matches('/'))
    }

    /// Build the form body for the initial token request.
    fn build_token_request_body(&self, client_secret: &str, password: Option<&str>) -> String {
        match self.grant_type {
            OAuthGrantType::ClientCredentials => {
                format!(
                    "grant_type=client_credentials&client_id={}&client_secret={}",
                    urlencoded(&self.client_id),
                    urlencoded(client_secret),
                )
            }
            OAuthGrantType::Password => {
                let username = self.username.as_deref().unwrap_or("");
                let pw = password.unwrap_or("");
                format!(
                    "grant_type=password&client_id={}&client_secret={}&username={}&password={}",
                    urlencoded(&self.client_id),
                    urlencoded(client_secret),
                    urlencoded(username),
                    urlencoded(pw),
                )
            }
        }
    }

    /// Build the form body for a refresh token request.
    fn build_refresh_request_body(
        &self,
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> String {
        format!(
            "grant_type=refresh_token&client_id={}&client_secret={}&refresh_token={}",
            urlencoded(client_id),
            urlencoded(client_secret),
            urlencoded(refresh_token),
        )
    }

    /// Retrieve the client_secret from the credential source.
    fn get_client_secret(&self) -> anyhow::Result<String> {
        match &self.credential_source {
            CredentialSource::Direct { client_secret, .. } => Ok(client_secret.clone()),
            CredentialSource::Keychain { profile_name } => {
                credentials::get_credential(profile_name, "client_secret")?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "No client_secret found for profile '{}'. \
                         Run `snow-cli auth login --profile {} --client-secret <secret>` first.",
                        profile_name,
                        profile_name
                    )
                })
            }
        }
    }

    /// Retrieve the password from the credential source (for password grant).
    fn get_password(&self) -> anyhow::Result<Option<String>> {
        if self.grant_type != OAuthGrantType::Password {
            return Ok(None);
        }
        match &self.credential_source {
            CredentialSource::Direct { password, .. } => match password {
                Some(pw) => Ok(Some(pw.clone())),
                None => anyhow::bail!("No password provided for OAuth2 password grant."),
            },
            CredentialSource::Keychain { profile_name } => {
                let pw =
                    credentials::get_credential(profile_name, "password")?.ok_or_else(|| {
                        anyhow::anyhow!(
                            "No password found for profile '{}'. \
                         Run `snow-cli auth login --profile {} --password <password>` first.",
                            profile_name,
                            profile_name
                        )
                    })?;
                Ok(Some(pw))
            }
        }
    }

    /// Exchange credentials for an access token via the token endpoint.
    async fn fetch_token(&self) -> anyhow::Result<CachedToken> {
        let client_secret = self.get_client_secret()?;
        let password = self.get_password()?;

        let body = self.build_token_request_body(&client_secret, password.as_deref());
        self.send_token_request(&body).await
    }

    /// Attempt to refresh using the stored refresh token.
    async fn refresh_token(&self, refresh_tok: &str) -> anyhow::Result<CachedToken> {
        let client_secret = self.get_client_secret()?;

        let body = self.build_refresh_request_body(&self.client_id, &client_secret, refresh_tok);
        self.send_token_request(&body).await
    }

    /// Send a token request to the ServiceNow OAuth endpoint.
    async fn send_token_request(&self, body: &str) -> anyhow::Result<CachedToken> {
        let http_client = reqwest::Client::new();
        let url = self.token_url();

        tracing::debug!(url = %url, "Requesting OAuth2 token");

        let request = http_client
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body.to_string())
            .build()?;

        crate::client::log_raw_http_request(&request);

        let response = http_client.execute(request).await?;

        if let Some(jsessionid) = crate::client::extract_jsessionid_from_headers(response.headers())
        {
            tracing::debug!(
                url = %url,
                jsessionid = %jsessionid,
                "Captured JSESSIONID from OAuth2 token response"
            );
        }

        let status = response.status();
        crate::client::log_raw_http_response(&url, status, response.headers());
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            tracing::error!(
                status = status.as_u16(),
                body = %error_body,
                "OAuth2 token request failed"
            );
            anyhow::bail!(
                "OAuth2 token request failed with status {}: {}",
                status.as_u16(),
                error_body
            );
        }

        let token_response: TokenResponse = response.json().await?;

        tracing::debug!(
            expires_in = token_response.expires_in,
            has_refresh = token_response.refresh_token.is_some(),
            "OAuth2 token acquired"
        );

        Ok(CachedToken {
            access_token: token_response.access_token,
            refresh_token: token_response.refresh_token,
            expires_at: Instant::now() + Duration::from_secs(token_response.expires_in),
        })
    }
}

/// Simple URL encoding for form values.
fn urlencoded(s: &str) -> String {
    // Encode characters that are not unreserved per RFC 3986
    let mut result = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", b));
            }
        }
    }
    result
}

#[async_trait]
impl Authenticator for OAuth2Auth {
    async fn authenticate(&self) -> anyhow::Result<HeaderMap> {
        // Check cached token
        {
            let cached = self.cached_token.read().await;
            if let Some(ref token) = *cached {
                if !token.is_expired() {
                    let mut headers = HeaderMap::new();
                    headers.insert(
                        http::header::AUTHORIZATION,
                        format!("Bearer {}", token.access_token).parse()?,
                    );
                    return Ok(headers);
                }
            }
        }

        // Token missing or expired — fetch a new one
        let token = self.fetch_token().await?;

        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            format!("Bearer {}", token.access_token).parse()?,
        );

        // Cache the token
        {
            let mut cached = self.cached_token.write().await;
            *cached = Some(token);
        }

        Ok(headers)
    }

    async fn refresh(&mut self) -> anyhow::Result<bool> {
        // Try to use refresh_token if available
        let refresh_tok = {
            let cached = self.cached_token.read().await;
            cached.as_ref().and_then(|t| t.refresh_token.clone())
        };

        let new_token = if let Some(ref rt) = refresh_tok {
            tracing::debug!("Attempting OAuth2 token refresh");
            match self.refresh_token(rt).await {
                Ok(t) => t,
                Err(e) => {
                    tracing::debug!(error = %e, "Refresh token failed, falling back to full auth");
                    self.fetch_token().await?
                }
            }
        } else {
            tracing::debug!("No refresh token available, re-authenticating");
            self.fetch_token().await?
        };

        let mut cached = self.cached_token.write().await;
        *cached = Some(new_token);

        Ok(true)
    }

    fn auth_type(&self) -> AuthMethod {
        AuthMethod::Oauth2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // --- Unit tests ---

    #[test]
    fn test_urlencoded_simple() {
        assert_eq!(urlencoded("hello"), "hello");
        assert_eq!(urlencoded("hello world"), "hello%20world");
        assert_eq!(urlencoded("a=b&c=d"), "a%3Db%26c%3Dd");
    }

    #[test]
    fn test_urlencoded_special_chars() {
        assert_eq!(urlencoded("p@ss:w0rd!"), "p%40ss%3Aw0rd%21");
    }

    #[test]
    fn test_token_url() {
        let profile = Profile {
            instance: "https://mycompany.service-now.com".to_string(),
            auth_method: AuthMethod::Oauth2,
            username: None,
            client_id: Some("client123".to_string()),
            oauth_grant_type: Some(OAuthGrantType::ClientCredentials),
            cert_path: None,
            key_path: None,
        };
        let auth = OAuth2Auth::new("test", &profile).unwrap();
        assert_eq!(
            auth.token_url(),
            "https://mycompany.service-now.com/oauth_token.do"
        );
    }

    #[test]
    fn test_token_url_strips_trailing_slash() {
        let profile = Profile {
            instance: "https://mycompany.service-now.com/".to_string(),
            auth_method: AuthMethod::Oauth2,
            username: None,
            client_id: Some("client123".to_string()),
            oauth_grant_type: Some(OAuthGrantType::ClientCredentials),
            cert_path: None,
            key_path: None,
        };
        let auth = OAuth2Auth::new("test", &profile).unwrap();
        assert_eq!(
            auth.token_url(),
            "https://mycompany.service-now.com/oauth_token.do"
        );
    }

    #[test]
    fn test_build_client_credentials_body() {
        let profile = Profile {
            instance: "https://test.service-now.com".to_string(),
            auth_method: AuthMethod::Oauth2,
            username: None,
            client_id: Some("my_client".to_string()),
            oauth_grant_type: Some(OAuthGrantType::ClientCredentials),
            cert_path: None,
            key_path: None,
        };
        let auth = OAuth2Auth::new("test", &profile).unwrap();
        let body = auth.build_token_request_body("my_secret", None);
        assert_eq!(
            body,
            "grant_type=client_credentials&client_id=my_client&client_secret=my_secret"
        );
    }

    #[test]
    fn test_build_password_body() {
        let profile = Profile {
            instance: "https://test.service-now.com".to_string(),
            auth_method: AuthMethod::Oauth2,
            username: Some("admin".to_string()),
            client_id: Some("my_client".to_string()),
            oauth_grant_type: Some(OAuthGrantType::Password),
            cert_path: None,
            key_path: None,
        };
        let auth = OAuth2Auth::new("test", &profile).unwrap();
        let body = auth.build_token_request_body("my_secret", Some("p@ss"));
        assert_eq!(
            body,
            "grant_type=password&client_id=my_client&client_secret=my_secret&username=admin&password=p%40ss"
        );
    }

    #[test]
    fn test_build_refresh_body() {
        let profile = Profile {
            instance: "https://test.service-now.com".to_string(),
            auth_method: AuthMethod::Oauth2,
            username: None,
            client_id: Some("my_client".to_string()),
            oauth_grant_type: Some(OAuthGrantType::ClientCredentials),
            cert_path: None,
            key_path: None,
        };
        let auth = OAuth2Auth::new("test", &profile).unwrap();
        let body = auth.build_refresh_request_body("my_client", "my_secret", "refresh_xyz");
        assert_eq!(
            body,
            "grant_type=refresh_token&client_id=my_client&client_secret=my_secret&refresh_token=refresh_xyz"
        );
    }

    #[test]
    fn test_new_requires_client_id() {
        let profile = Profile {
            instance: "https://test.service-now.com".to_string(),
            auth_method: AuthMethod::Oauth2,
            username: None,
            client_id: None,
            oauth_grant_type: Some(OAuthGrantType::ClientCredentials),
            cert_path: None,
            key_path: None,
        };
        let result = OAuth2Auth::new("test", &profile);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("client_id"));
    }

    #[test]
    fn test_password_grant_requires_username() {
        let profile = Profile {
            instance: "https://test.service-now.com".to_string(),
            auth_method: AuthMethod::Oauth2,
            username: None,
            client_id: Some("client123".to_string()),
            oauth_grant_type: Some(OAuthGrantType::Password),
            cert_path: None,
            key_path: None,
        };
        let result = OAuth2Auth::new("test", &profile);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("username"));
    }

    #[test]
    fn test_defaults_to_client_credentials() {
        let profile = Profile {
            instance: "https://test.service-now.com".to_string(),
            auth_method: AuthMethod::Oauth2,
            username: None,
            client_id: Some("client123".to_string()),
            oauth_grant_type: None,
            cert_path: None,
            key_path: None,
        };
        let auth = OAuth2Auth::new("test-profile", &profile).unwrap();
        assert_eq!(auth.grant_type, OAuthGrantType::ClientCredentials);
    }

    #[test]
    fn test_cached_token_expiry() {
        let token = CachedToken {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_at: Instant::now() + Duration::from_secs(1800),
        };
        assert!(!token.is_expired());

        let expired_token = CachedToken {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_at: Instant::now() - Duration::from_secs(1),
        };
        assert!(expired_token.is_expired());
    }

    // --- Wiremock integration tests ---

    /// Helper to create an OAuth2Auth pointing at the mock server.
    fn test_oauth2_client_credentials(server_uri: &str) -> OAuth2Auth {
        OAuth2Auth::new_with_credentials(
            server_uri.to_string(),
            "test_client_id".to_string(),
            OAuthGrantType::ClientCredentials,
            None,
            "test_client_secret".to_string(),
            None,
        )
    }

    /// Helper to create an OAuth2Auth with password grant pointing at the mock server.
    fn test_oauth2_password(server_uri: &str) -> OAuth2Auth {
        OAuth2Auth::new_with_credentials(
            server_uri.to_string(),
            "test_client_id".to_string(),
            OAuthGrantType::Password,
            Some("admin".to_string()),
            "test_client_secret".to_string(),
            Some("test_password".to_string()),
        )
    }

    /// Standard token response JSON.
    fn token_response_json(access_token: &str, expires_in: u64) -> serde_json::Value {
        serde_json::json!({
            "access_token": access_token,
            "token_type": "Bearer",
            "expires_in": expires_in
        })
    }

    /// Token response with refresh token.
    fn token_response_with_refresh(
        access_token: &str,
        refresh_token: &str,
        expires_in: u64,
    ) -> serde_json::Value {
        serde_json::json!({
            "access_token": access_token,
            "refresh_token": refresh_token,
            "token_type": "Bearer",
            "expires_in": expires_in
        })
    }

    #[tokio::test]
    async fn test_client_credentials_token_exchange() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/oauth_token.do"))
            .and(header("Content-Type", "application/x-www-form-urlencoded"))
            .and(body_string_contains("grant_type=client_credentials"))
            .and(body_string_contains("client_id=test_client_id"))
            .and(body_string_contains("client_secret=test_client_secret"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(token_response_json("access_abc", 3600)),
            )
            .expect(1)
            .mount(&server)
            .await;

        let auth = test_oauth2_client_credentials(&server.uri());
        let headers = auth.authenticate().await.unwrap();

        assert_eq!(
            headers.get("authorization").unwrap().to_str().unwrap(),
            "Bearer access_abc"
        );
    }

    #[tokio::test]
    async fn test_password_grant_token_exchange() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/oauth_token.do"))
            .and(body_string_contains("grant_type=password"))
            .and(body_string_contains("client_id=test_client_id"))
            .and(body_string_contains("client_secret=test_client_secret"))
            .and(body_string_contains("username=admin"))
            .and(body_string_contains("password=test_password"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(token_response_json("pw_token", 3600)),
            )
            .expect(1)
            .mount(&server)
            .await;

        let auth = test_oauth2_password(&server.uri());
        let headers = auth.authenticate().await.unwrap();

        assert_eq!(
            headers.get("authorization").unwrap().to_str().unwrap(),
            "Bearer pw_token"
        );
    }

    #[tokio::test]
    async fn test_token_caching_avoids_duplicate_requests() {
        let server = MockServer::start().await;

        // Should only be called once — second authenticate() uses cached token
        Mock::given(method("POST"))
            .and(path("/oauth_token.do"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(token_response_json("cached_tok", 3600)),
            )
            .expect(1)
            .mount(&server)
            .await;

        let auth = test_oauth2_client_credentials(&server.uri());

        let headers1 = auth.authenticate().await.unwrap();
        let headers2 = auth.authenticate().await.unwrap();

        assert_eq!(
            headers1.get("authorization").unwrap().to_str().unwrap(),
            "Bearer cached_tok"
        );
        assert_eq!(
            headers2.get("authorization").unwrap().to_str().unwrap(),
            "Bearer cached_tok"
        );
    }

    #[tokio::test]
    async fn test_expired_token_triggers_refetch() {
        let server = MockServer::start().await;

        // Will be called twice: initial fetch + refetch after expiry
        Mock::given(method("POST"))
            .and(path("/oauth_token.do"))
            .respond_with(
                // expires_in=0 means token is immediately expired (within the 30s buffer)
                ResponseTemplate::new(200).set_body_json(token_response_json("expired_tok", 0)),
            )
            .expect(2)
            .mount(&server)
            .await;

        let auth = test_oauth2_client_credentials(&server.uri());

        // First call fetches token
        auth.authenticate().await.unwrap();
        // Token has 0s expiry, so it's immediately expired — second call refetches
        auth.authenticate().await.unwrap();
    }

    #[tokio::test]
    async fn test_refresh_uses_refresh_token() {
        let server = MockServer::start().await;

        // Initial token request returns a refresh_token
        Mock::given(method("POST"))
            .and(path("/oauth_token.do"))
            .and(body_string_contains("grant_type=client_credentials"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(token_response_with_refresh(
                    "initial_tok",
                    "refresh_xyz",
                    3600,
                )),
            )
            .expect(1)
            .mount(&server)
            .await;

        // Refresh request uses the refresh_token
        Mock::given(method("POST"))
            .and(path("/oauth_token.do"))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("refresh_token=refresh_xyz"))
            .and(body_string_contains("client_id=test_client_id"))
            .and(body_string_contains("client_secret=test_client_secret"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(token_response_json("refreshed_tok", 3600)),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut auth = test_oauth2_client_credentials(&server.uri());

        // Initial authenticate — fetches token with refresh_token
        let headers = auth.authenticate().await.unwrap();
        assert_eq!(
            headers.get("authorization").unwrap().to_str().unwrap(),
            "Bearer initial_tok"
        );

        // Call refresh — should use the refresh_token
        let refreshed = auth.refresh().await.unwrap();
        assert!(refreshed);

        // Subsequent authenticate should use the refreshed token from cache
        let headers2 = auth.authenticate().await.unwrap();
        assert_eq!(
            headers2.get("authorization").unwrap().to_str().unwrap(),
            "Bearer refreshed_tok"
        );
    }

    #[tokio::test]
    async fn test_refresh_without_refresh_token_reauthenticates() {
        let server = MockServer::start().await;

        // Token response without refresh_token — called twice (initial + re-auth on refresh)
        Mock::given(method("POST"))
            .and(path("/oauth_token.do"))
            .and(body_string_contains("grant_type=client_credentials"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(token_response_json("reauth_tok", 3600)),
            )
            .expect(2)
            .mount(&server)
            .await;

        let mut auth = test_oauth2_client_credentials(&server.uri());

        // Initial authenticate
        auth.authenticate().await.unwrap();

        // Refresh with no refresh_token falls back to full re-auth
        let refreshed = auth.refresh().await.unwrap();
        assert!(refreshed);
    }

    #[tokio::test]
    async fn test_refresh_token_failure_falls_back_to_reauth() {
        let server = MockServer::start().await;

        // Initial token request returns a refresh_token
        Mock::given(method("POST"))
            .and(path("/oauth_token.do"))
            .and(body_string_contains("grant_type=client_credentials"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(token_response_with_refresh(
                    "tok1",
                    "bad_refresh",
                    3600,
                )),
            )
            .expect(2) // called once for initial, once for fallback re-auth
            .mount(&server)
            .await;

        // Refresh token request fails
        Mock::given(method("POST"))
            .and(path("/oauth_token.do"))
            .and(body_string_contains("grant_type=refresh_token"))
            .respond_with(
                ResponseTemplate::new(400).set_body_string("invalid_grant: refresh token expired"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut auth = test_oauth2_client_credentials(&server.uri());

        // Initial authenticate
        auth.authenticate().await.unwrap();

        // Refresh: tries refresh_token, fails, falls back to client_credentials re-auth
        let refreshed = auth.refresh().await.unwrap();
        assert!(refreshed);
    }

    #[tokio::test]
    async fn test_token_endpoint_error_returns_error() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/oauth_token.do"))
            .respond_with(ResponseTemplate::new(400).set_body_string(
                r#"{"error":"invalid_client","error_description":"Bad credentials"}"#,
            ))
            .mount(&server)
            .await;

        let auth = test_oauth2_client_credentials(&server.uri());
        let result = auth.authenticate().await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("400"));
        assert!(err_msg.contains("invalid_client"));
    }

    #[tokio::test]
    async fn test_token_endpoint_500_returns_error() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/oauth_token.do"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&server)
            .await;

        let auth = test_oauth2_client_credentials(&server.uri());
        let result = auth.authenticate().await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("500"));
    }

    #[tokio::test]
    async fn test_auth_type_returns_oauth2() {
        let auth = test_oauth2_client_credentials("https://test.service-now.com");
        assert_eq!(auth.auth_type(), AuthMethod::Oauth2);
    }
}
