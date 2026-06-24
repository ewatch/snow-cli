use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::Engine;
use http::HeaderMap;
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

use crate::auth::Authenticator;
use crate::config::credentials;
use crate::config::profile::{AuthMethod, OAuthGrantType, Profile};

pub const DEFAULT_OAUTH_REDIRECT_HOST: &str = "127.0.0.1";
pub const DEFAULT_OAUTH_REDIRECT_PORT: u16 = 8080;
pub const DEFAULT_OAUTH_REDIRECT_PATH: &str = "/oauth/callback";
pub const DEFAULT_OAUTH_SCOPE: &str = "useraccount";

/// Cached OAuth2 token with expiry tracking.
#[derive(Clone)]
struct CachedToken {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Instant,
}

impl fmt::Debug for CachedToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CachedToken")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl CachedToken {
    fn is_expired(&self) -> bool {
        // Consider expired 30 seconds early to avoid edge cases
        Instant::now() >= self.expires_at - Duration::from_secs(30)
    }
}

/// OAuth token persisted in the keychain for authorization-code profiles.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct StoredOAuthToken {
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl fmt::Debug for StoredOAuthToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredOAuthToken")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("token_type", &self.token_type)
            .field("expires_at", &self.expires_at)
            .field("scope", &self.scope)
            .finish()
    }
}

impl StoredOAuthToken {
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|expires_at| now_epoch_secs().saturating_add(30) >= expires_at)
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone)]
struct TokenSet {
    cached: CachedToken,
    stored: StoredOAuthToken,
}

/// Source of credentials — either keychain/env lookup or directly injected (for testing).
#[derive(Debug, Clone)]
enum CredentialSource {
    /// Look up credentials from keychain/env at runtime.
    Keychain { profile_name: String },
    /// Use directly provided credentials (for testing).
    Direct {
        client_secret: Option<String>,
        password: Option<String>,
        oauth_token: Option<StoredOAuthToken>,
    },
}

/// OAuth 2.0 authenticator supporting client_credentials, password, and
/// authorization-code grant types.
///
/// Tokens are cached in memory and automatically refreshed when expired.
#[derive(Debug)]
pub struct OAuth2Auth {
    /// Instance base URL (e.g., https://mycompany.service-now.com)
    instance: String,
    /// OAuth client ID from config
    client_id: String,
    /// Grant type (client_credentials, password, or authorization_code)
    grant_type: OAuthGrantType,
    /// Username (required for password grant, from config)
    username: Option<String>,
    /// Where to get client_secret, password, and persisted tokens from
    credential_source: CredentialSource,
    /// Cached token (shared across authenticate/refresh calls)
    cached_token: Arc<RwLock<Option<CachedToken>>>,
}

/// ServiceNow OAuth token response.
#[derive(Debug, serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    token_type: Option<String>,
    expires_in: u64,
    scope: Option<String>,
}

impl OAuth2Auth {
    pub fn new(profile_name: &str, profile: &Profile) -> anyhow::Result<Self> {
        let client_id = profile.client_id.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "OAuth2 auth requires `client_id` in the profile configuration. \
                 Use: snow-cli profile edit {} --client-id <id>",
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
                 Use: snow-cli profile edit {} --username <user>",
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
        client_secret: impl Into<Option<String>>,
        password: Option<String>,
    ) -> Self {
        Self {
            instance,
            client_id,
            grant_type,
            username,
            credential_source: CredentialSource::Direct {
                client_secret: client_secret.into(),
                password,
                oauth_token: None,
            },
            cached_token: Arc::new(RwLock::new(None)),
        }
    }

    /// Create an authorization-code authenticator with a directly injected token (for testing).
    #[cfg(test)]
    pub fn new_with_stored_token(
        instance: String,
        client_id: String,
        client_secret: impl Into<Option<String>>,
        oauth_token: StoredOAuthToken,
    ) -> Self {
        Self {
            instance,
            client_id,
            grant_type: OAuthGrantType::AuthorizationCode,
            username: None,
            credential_source: CredentialSource::Direct {
                client_secret: client_secret.into(),
                password: None,
                oauth_token: Some(oauth_token),
            },
            cached_token: Arc::new(RwLock::new(None)),
        }
    }

    /// Token endpoint URL for ServiceNow.
    fn token_url(&self) -> String {
        token_url_for_instance(&self.instance)
    }

    /// Build the form body for the initial token request.
    fn build_token_request_body(&self, client_secret: &str, password: Option<&str>) -> String {
        match self.grant_type {
            OAuthGrantType::ClientCredentials => build_form_body(&[
                ("grant_type", Some("client_credentials")),
                ("client_id", Some(self.client_id.as_str())),
                ("client_secret", Some(client_secret)),
            ]),
            OAuthGrantType::Password => {
                let username = self.username.as_deref().unwrap_or("");
                let pw = password.unwrap_or("");
                build_form_body(&[
                    ("grant_type", Some("password")),
                    ("client_id", Some(self.client_id.as_str())),
                    ("client_secret", Some(client_secret)),
                    ("username", Some(username)),
                    ("password", Some(pw)),
                ])
            }
            OAuthGrantType::AuthorizationCode => String::new(),
        }
    }

    /// Build the form body for a refresh token request.
    fn build_refresh_request_body(
        &self,
        client_id: &str,
        client_secret: Option<&str>,
        refresh_token: &str,
    ) -> String {
        build_form_body(&[
            ("grant_type", Some("refresh_token")),
            ("client_id", Some(client_id)),
            ("client_secret", client_secret),
            ("refresh_token", Some(refresh_token)),
        ])
    }

    /// Retrieve the client_secret from the credential source, if one is configured.
    fn get_client_secret_optional(&self) -> anyhow::Result<Option<String>> {
        match &self.credential_source {
            CredentialSource::Direct { client_secret, .. } => Ok(client_secret.clone()),
            CredentialSource::Keychain { profile_name } => {
                credentials::get_credential(profile_name, "client_secret")
            }
        }
    }

    /// Retrieve the client_secret from the credential source when the grant requires it.
    fn get_required_client_secret(&self) -> anyhow::Result<String> {
        self.get_client_secret_optional()?
            .ok_or_else(|| match &self.credential_source {
                CredentialSource::Keychain { profile_name } => anyhow::anyhow!(
                    "No client_secret found for profile '{}'. \
                 Run `snow-cli auth login --profile {} --client-secret <secret>` first.",
                    profile_name,
                    profile_name
                ),
                CredentialSource::Direct { .. } => anyhow::anyhow!(
                    "No client_secret provided for OAuth2 {:?} grant.",
                    self.grant_type
                ),
            })
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

    /// Retrieve the persisted authorization-code OAuth token.
    fn get_stored_oauth_token(&self) -> anyhow::Result<Option<StoredOAuthToken>> {
        match &self.credential_source {
            CredentialSource::Direct { oauth_token, .. } => Ok(oauth_token.clone()),
            CredentialSource::Keychain { profile_name } => {
                let Some(raw) = credentials::get_credential(profile_name, "oauth_token")? else {
                    return Ok(None);
                };
                let token = serde_json::from_str(&raw)?;
                Ok(Some(token))
            }
        }
    }

    /// Persist an authorization-code OAuth token after refresh.
    fn store_oauth_token(&self, token: &StoredOAuthToken) -> anyhow::Result<()> {
        if let CredentialSource::Keychain { profile_name } = &self.credential_source {
            credentials::store_credential(
                profile_name,
                "oauth_token",
                &serde_json::to_string(token)?,
            )?;
        }
        Ok(())
    }

    /// Exchange credentials for an access token via the token endpoint.
    async fn fetch_token(&self) -> anyhow::Result<CachedToken> {
        if self.grant_type == OAuthGrantType::AuthorizationCode {
            anyhow::bail!(
                "No OAuth access token is stored for this profile. Run `snow-cli auth login --profile <profile>` to complete the authorization-code browser flow."
            );
        }

        let client_secret = self.get_required_client_secret()?;
        let password = self.get_password()?;

        let body = self.build_token_request_body(&client_secret, password.as_deref());
        Ok(self.send_token_request(&body).await?.cached)
    }

    /// Attempt to refresh using the stored refresh token.
    async fn refresh_token(&self, refresh_tok: &str) -> anyhow::Result<TokenSet> {
        let client_secret = self.get_client_secret_optional()?;

        let body =
            self.build_refresh_request_body(&self.client_id, client_secret.as_deref(), refresh_tok);
        self.send_token_request(&body).await
    }

    /// Send a token request to the ServiceNow OAuth endpoint.
    async fn send_token_request(&self, body: &str) -> anyhow::Result<TokenSet> {
        send_oauth_token_request(&self.token_url(), body).await
    }

    async fn authenticate_authorization_code(&self) -> anyhow::Result<HeaderMap> {
        if let Some(token) = self.valid_cached_token().await? {
            return Ok(token);
        }

        let Some(stored_token) = self.get_stored_oauth_token()? else {
            anyhow::bail!(
                "No OAuth token found for this profile. Run `snow-cli auth login --profile <profile>` to complete the authorization-code browser flow."
            );
        };

        let token = if !stored_token.is_expired() {
            stored_to_cached(&stored_token)
        } else if let Some(refresh_token) = stored_token.refresh_token.as_deref() {
            tracing::debug!("Stored OAuth2 access token expired; refreshing with refresh_token");
            let refreshed = self.refresh_token(refresh_token).await?;
            self.store_oauth_token(&refreshed.stored)?;
            refreshed.cached
        } else {
            anyhow::bail!(
                "Stored OAuth token is expired and no refresh_token is available. Run `snow-cli auth login --profile <profile>` again."
            );
        };

        let headers = bearer_headers(&token.access_token)?;
        let mut cached = self.cached_token.write().await;
        *cached = Some(token);
        Ok(headers)
    }

    async fn valid_cached_token(&self) -> anyhow::Result<Option<HeaderMap>> {
        let cached = self.cached_token.read().await;
        if let Some(ref token) = *cached
            && !token.is_expired()
        {
            return Ok(Some(bearer_headers(&token.access_token)?));
        }
        Ok(None)
    }
}

pub fn token_url_for_instance(instance: &str) -> String {
    format!("{}/oauth_token.do", instance.trim_end_matches('/'))
}

pub fn authorization_url(
    profile: &Profile,
    redirect_uri: &str,
    state: &str,
    code_challenge: &str,
) -> anyhow::Result<String> {
    let client_id = profile.client_id.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "OAuth2 authorization-code login requires `client_id` in the profile configuration."
        )
    })?;
    let scope = profile
        .oauth_scope
        .as_deref()
        .unwrap_or(DEFAULT_OAUTH_SCOPE)
        .trim();

    let mut url = format!(
        "{}/oauth_auth.do?response_type=code&client_id={}&redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256",
        profile.instance.trim_end_matches('/'),
        urlencoded(client_id),
        urlencoded(redirect_uri),
        urlencoded(state),
        urlencoded(code_challenge),
    );

    if !scope.is_empty() {
        url.push_str("&scope=");
        url.push_str(&urlencoded(scope));
    }

    Ok(url)
}

pub fn oauth_redirect_host(profile: &Profile) -> &str {
    profile
        .oauth_redirect_host
        .as_deref()
        .unwrap_or(DEFAULT_OAUTH_REDIRECT_HOST)
}

pub fn validate_oauth_redirect_host(host: &str) -> anyhow::Result<()> {
    if matches!(host, "127.0.0.1" | "::1") || host.eq_ignore_ascii_case("localhost") {
        return Ok(());
    }

    anyhow::bail!(
        "OAuth redirect host '{}' is not allowed. Use a loopback host such as 127.0.0.1, ::1, or localhost.",
        host
    )
}

pub fn oauth_redirect_port(profile: &Profile) -> u16 {
    profile
        .oauth_redirect_port
        .unwrap_or(DEFAULT_OAUTH_REDIRECT_PORT)
}

pub fn oauth_redirect_path(profile: &Profile) -> String {
    normalize_redirect_path(
        profile
            .oauth_redirect_path
            .as_deref()
            .unwrap_or(DEFAULT_OAUTH_REDIRECT_PATH),
    )
}

pub async fn exchange_authorization_code(
    profile: &Profile,
    code: &str,
    redirect_uri: &str,
    client_secret: Option<&str>,
    code_verifier: &str,
) -> anyhow::Result<StoredOAuthToken> {
    let client_id = profile.client_id.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "OAuth2 authorization-code login requires `client_id` in the profile configuration."
        )
    })?;
    let body = build_form_body(&[
        ("grant_type", Some("authorization_code")),
        ("client_id", Some(client_id)),
        ("client_secret", client_secret),
        ("code", Some(code)),
        ("redirect_uri", Some(redirect_uri)),
        ("code_verifier", Some(code_verifier)),
    ]);
    let token_set =
        send_oauth_token_request(&token_url_for_instance(&profile.instance), &body).await?;
    Ok(token_set.stored)
}

async fn send_oauth_token_request(url: &str, body: &str) -> anyhow::Result<TokenSet> {
    let http_client = reqwest::Client::new();

    tracing::debug!(url = %url, "Requesting OAuth2 token");

    let request = http_client
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body.to_string())
        .build()?;

    crate::client::log_raw_http_request(&request);

    let response = http_client.execute(request).await?;

    if let Some(jsessionid) = crate::client::extract_jsessionid_from_headers(response.headers()) {
        tracing::debug!(
            url = %url,
            jsessionid = %jsessionid,
            "Captured JSESSIONID from OAuth2 token response"
        );
    }

    let status = response.status();
    crate::client::log_raw_http_response(url, status, response.headers());
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

    Ok(token_response.into_token_set())
}

impl TokenResponse {
    fn into_token_set(self) -> TokenSet {
        let expires_at = now_epoch_secs().saturating_add(self.expires_in);
        let refresh_token = self.refresh_token;
        let access_token = self.access_token;
        TokenSet {
            cached: CachedToken {
                access_token: access_token.clone(),
                refresh_token: refresh_token.clone(),
                expires_at: Instant::now() + Duration::from_secs(self.expires_in),
            },
            stored: StoredOAuthToken {
                access_token,
                refresh_token,
                token_type: self.token_type,
                expires_at: Some(expires_at),
                scope: self.scope,
            },
        }
    }
}

fn stored_to_cached(stored: &StoredOAuthToken) -> CachedToken {
    let expires_at = stored
        .expires_at
        .map(|expires_at| {
            let now = now_epoch_secs();
            if expires_at > now {
                Instant::now() + Duration::from_secs(expires_at - now)
            } else {
                Instant::now() - Duration::from_secs(1)
            }
        })
        .unwrap_or_else(|| Instant::now() + Duration::from_secs(3600));

    CachedToken {
        access_token: stored.access_token.clone(),
        refresh_token: stored.refresh_token.clone(),
        expires_at,
    }
}

fn bearer_headers(access_token: &str) -> anyhow::Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        http::header::AUTHORIZATION,
        format!("Bearer {access_token}").parse()?,
    );
    Ok(headers)
}

fn normalize_redirect_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return DEFAULT_OAUTH_REDIRECT_PATH.to_string();
    }
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn pkce_code_challenge_s256(code_verifier: &str) -> String {
    let digest = Sha256::digest(code_verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// Simple URL encoding for form values.
fn build_form_body(params: &[(&str, Option<&str>)]) -> String {
    let mut body = String::new();

    for (key, value) in params {
        let Some(value) = value else {
            continue;
        };

        if !body.is_empty() {
            body.push('&');
        }
        body.push_str(key);
        body.push('=');
        body.push_str(&urlencoded(value));
    }

    body
}

pub fn urlencoded(s: &str) -> String {
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
        if self.grant_type == OAuthGrantType::AuthorizationCode {
            return self.authenticate_authorization_code().await;
        }

        if let Some(headers) = self.valid_cached_token().await? {
            return Ok(headers);
        }

        // Token missing or expired — fetch a new one
        let token = self.fetch_token().await?;
        let headers = bearer_headers(&token.access_token)?;

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
        }
        .or_else(|| {
            if self.grant_type == OAuthGrantType::AuthorizationCode {
                self.get_stored_oauth_token()
                    .ok()
                    .flatten()
                    .and_then(|token| token.refresh_token)
            } else {
                None
            }
        });

        let new_token = if let Some(ref rt) = refresh_tok {
            tracing::debug!("Attempting OAuth2 token refresh");
            match self.refresh_token(rt).await {
                Ok(token_set) => {
                    if self.grant_type == OAuthGrantType::AuthorizationCode {
                        self.store_oauth_token(&token_set.stored)?;
                    }
                    token_set.cached
                }
                Err(e) if self.grant_type != OAuthGrantType::AuthorizationCode => {
                    tracing::debug!(error = %e, "Refresh token failed, falling back to full auth");
                    self.fetch_token().await?
                }
                Err(e) => return Err(e),
            }
        } else if self.grant_type == OAuthGrantType::AuthorizationCode {
            anyhow::bail!(
                "No OAuth refresh_token is available. Run `snow-cli auth login --profile <profile>` again."
            );
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
    use wiremock::matchers::{body_string, body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // --- Unit tests ---

    #[test]
    fn stored_oauth_token_debug_redacts_secret_values() {
        let token = StoredOAuthToken {
            access_token: "access-secret-value".to_string(),
            refresh_token: Some("refresh-secret-value".to_string()),
            token_type: Some("Bearer".to_string()),
            expires_at: Some(1_725_000_000),
            scope: Some("useraccount".to_string()),
        };

        let debug = format!("{token:?}");

        assert!(!debug.contains("access-secret-value"));
        assert!(!debug.contains("refresh-secret-value"));
        assert!(debug.contains("<redacted>"));
        assert!(debug.contains("Bearer"));
        assert!(debug.contains("useraccount"));
    }

    #[test]
    fn cached_oauth_token_debug_redacts_secret_values() {
        let token = CachedToken {
            access_token: "cached-access-secret".to_string(),
            refresh_token: Some("cached-refresh-secret".to_string()),
            expires_at: Instant::now(),
        };

        let debug = format!("{token:?}");

        assert!(!debug.contains("cached-access-secret"));
        assert!(!debug.contains("cached-refresh-secret"));
        assert!(debug.contains("<redacted>"));
    }

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
    fn test_authorization_url_uses_scope_state_and_redirect_uri() {
        let profile = Profile {
            instance: "https://mycompany.service-now.com/".to_string(),
            auth_method: AuthMethod::Oauth2,
            username: None,
            client_id: Some("client 123".to_string()),
            oauth_grant_type: Some(OAuthGrantType::AuthorizationCode),
            oauth_scope: Some("useraccount email".to_string()),
            oauth_redirect_host: None,
            oauth_redirect_port: None,
            oauth_redirect_path: None,
            cert_path: None,
            key_path: None,
            sso_login_url: None,
        };

        let url = authorization_url(
            &profile,
            "http://127.0.0.1:8080/oauth/callback",
            "state-123",
            "pkce-challenge-123",
        )
        .unwrap();

        assert!(url.starts_with("https://mycompany.service-now.com/oauth_auth.do?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=client%20123"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8080%2Foauth%2Fcallback"));
        assert!(url.contains("state=state-123"));
        assert!(url.contains("code_challenge=pkce-challenge-123"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("scope=useraccount%20email"));
    }

    #[test]
    fn test_pkce_code_challenge_s256_matches_rfc_vector() {
        assert_eq!(
            pkce_code_challenge_s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn test_validate_oauth_redirect_host_requires_loopback() {
        assert!(validate_oauth_redirect_host("127.0.0.1").is_ok());
        assert!(validate_oauth_redirect_host("localhost").is_ok());
        assert!(validate_oauth_redirect_host("::1").is_ok());
        assert!(validate_oauth_redirect_host("0.0.0.0").is_err());
        assert!(validate_oauth_redirect_host("192.168.1.10").is_err());
        assert!(validate_oauth_redirect_host("example.com").is_err());
    }

    #[test]
    fn test_oauth_redirect_defaults_and_normalization() {
        let profile = Profile {
            instance: "https://test.service-now.com".to_string(),
            auth_method: AuthMethod::Oauth2,
            username: None,
            client_id: Some("client123".to_string()),
            oauth_grant_type: Some(OAuthGrantType::AuthorizationCode),
            oauth_scope: None,
            oauth_redirect_host: Some("localhost".to_string()),
            oauth_redirect_port: Some(8484),
            oauth_redirect_path: Some("callback".to_string()),
            cert_path: None,
            key_path: None,
            sso_login_url: None,
        };

        assert_eq!(oauth_redirect_host(&profile), "localhost");
        assert_eq!(oauth_redirect_port(&profile), 8484);
        assert_eq!(oauth_redirect_path(&profile), "/callback");
    }

    #[test]
    fn test_stored_oauth_token_expiry_buffer() {
        let valid = StoredOAuthToken {
            access_token: "access".to_string(),
            refresh_token: None,
            token_type: Some("Bearer".to_string()),
            expires_at: Some(now_epoch_secs() + 3600),
            scope: None,
        };
        assert!(!valid.is_expired());

        let almost_expired = StoredOAuthToken {
            expires_at: Some(now_epoch_secs() + 10),
            ..valid
        };
        assert!(almost_expired.is_expired());
    }

    #[test]
    fn test_token_url() {
        let profile = Profile {
            instance: "https://mycompany.service-now.com".to_string(),
            auth_method: AuthMethod::Oauth2,
            username: None,
            client_id: Some("client123".to_string()),
            oauth_grant_type: Some(OAuthGrantType::ClientCredentials),
            oauth_scope: None,
            oauth_redirect_host: None,
            oauth_redirect_port: None,
            oauth_redirect_path: None,
            cert_path: None,
            key_path: None,
            sso_login_url: None,
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
            oauth_scope: None,
            oauth_redirect_host: None,
            oauth_redirect_port: None,
            oauth_redirect_path: None,
            cert_path: None,
            key_path: None,
            sso_login_url: None,
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
            oauth_scope: None,
            oauth_redirect_host: None,
            oauth_redirect_port: None,
            oauth_redirect_path: None,
            cert_path: None,
            key_path: None,
            sso_login_url: None,
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
            oauth_scope: None,
            oauth_redirect_host: None,
            oauth_redirect_port: None,
            oauth_redirect_path: None,
            cert_path: None,
            key_path: None,
            sso_login_url: None,
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
            oauth_scope: None,
            oauth_redirect_host: None,
            oauth_redirect_port: None,
            oauth_redirect_path: None,
            cert_path: None,
            key_path: None,
            sso_login_url: None,
        };
        let auth = OAuth2Auth::new("test", &profile).unwrap();
        let body = auth.build_refresh_request_body("my_client", Some("my_secret"), "refresh_xyz");
        assert_eq!(
            body,
            "grant_type=refresh_token&client_id=my_client&client_secret=my_secret&refresh_token=refresh_xyz"
        );
    }

    #[test]
    fn test_build_refresh_body_without_client_secret() {
        let profile = Profile {
            instance: "https://test.service-now.com".to_string(),
            auth_method: AuthMethod::Oauth2,
            username: None,
            client_id: Some("my_client".to_string()),
            oauth_grant_type: Some(OAuthGrantType::AuthorizationCode),
            oauth_scope: None,
            oauth_redirect_host: None,
            oauth_redirect_port: None,
            oauth_redirect_path: None,
            cert_path: None,
            key_path: None,
            sso_login_url: None,
        };
        let auth = OAuth2Auth::new("test", &profile).unwrap();
        let body = auth.build_refresh_request_body("my_client", None, "refresh_xyz");
        assert_eq!(
            body,
            "grant_type=refresh_token&client_id=my_client&refresh_token=refresh_xyz"
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
            oauth_scope: None,
            oauth_redirect_host: None,
            oauth_redirect_port: None,
            oauth_redirect_path: None,
            cert_path: None,
            key_path: None,
            sso_login_url: None,
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
            oauth_scope: None,
            oauth_redirect_host: None,
            oauth_redirect_port: None,
            oauth_redirect_path: None,
            cert_path: None,
            key_path: None,
            sso_login_url: None,
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
            oauth_scope: None,
            oauth_redirect_host: None,
            oauth_redirect_port: None,
            oauth_redirect_path: None,
            cert_path: None,
            key_path: None,
            sso_login_url: None,
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
    async fn test_authorization_code_exchange() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/oauth_token.do"))
            .and(body_string_contains("grant_type=authorization_code"))
            .and(body_string_contains("client_id=test_client_id"))
            .and(body_string_contains("client_secret=test_client_secret"))
            .and(body_string_contains("code=auth_code_123"))
            .and(body_string_contains(
                "redirect_uri=http%3A%2F%2F127.0.0.1%3A8080%2Foauth%2Fcallback",
            ))
            .and(body_string_contains("code_verifier=verifier_123"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(token_response_with_refresh(
                    "auth_code_access",
                    "refresh_auth_code",
                    3600,
                )),
            )
            .expect(1)
            .mount(&server)
            .await;

        let profile = Profile {
            instance: server.uri(),
            auth_method: AuthMethod::Oauth2,
            username: None,
            client_id: Some("test_client_id".to_string()),
            oauth_grant_type: Some(OAuthGrantType::AuthorizationCode),
            oauth_scope: None,
            oauth_redirect_host: None,
            oauth_redirect_port: None,
            oauth_redirect_path: None,
            cert_path: None,
            key_path: None,
            sso_login_url: None,
        };

        let token = exchange_authorization_code(
            &profile,
            "auth_code_123",
            "http://127.0.0.1:8080/oauth/callback",
            Some("test_client_secret"),
            "verifier_123",
        )
        .await
        .unwrap();

        assert_eq!(token.access_token, "auth_code_access");
        assert_eq!(token.refresh_token, Some("refresh_auth_code".to_string()));
        assert!(!token.is_expired());
    }

    #[tokio::test]
    async fn test_authorization_code_exchange_without_client_secret() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/oauth_token.do"))
            .and(body_string(
                "grant_type=authorization_code&client_id=test_client_id&code=auth_code_123&redirect_uri=http%3A%2F%2F127.0.0.1%3A8080%2Foauth%2Fcallback&code_verifier=verifier_123"
                    .to_string(),
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(token_response_with_refresh(
                    "auth_code_access_public",
                    "refresh_auth_code_public",
                    3600,
                )),
            )
            .expect(1)
            .mount(&server)
            .await;

        let profile = Profile {
            instance: server.uri(),
            auth_method: AuthMethod::Oauth2,
            username: None,
            client_id: Some("test_client_id".to_string()),
            oauth_grant_type: Some(OAuthGrantType::AuthorizationCode),
            oauth_scope: None,
            oauth_redirect_host: None,
            oauth_redirect_port: None,
            oauth_redirect_path: None,
            cert_path: None,
            key_path: None,
            sso_login_url: None,
        };

        let token = exchange_authorization_code(
            &profile,
            "auth_code_123",
            "http://127.0.0.1:8080/oauth/callback",
            None,
            "verifier_123",
        )
        .await
        .unwrap();

        assert_eq!(token.access_token, "auth_code_access_public");
        assert_eq!(
            token.refresh_token,
            Some("refresh_auth_code_public".to_string())
        );
        assert!(!token.is_expired());
    }

    #[tokio::test]
    async fn test_authorization_code_authenticate_uses_stored_token() {
        let auth = OAuth2Auth::new_with_stored_token(
            "https://test.service-now.com".to_string(),
            "test_client_id".to_string(),
            Some("test_client_secret".to_string()),
            StoredOAuthToken {
                access_token: "stored_access".to_string(),
                refresh_token: Some("stored_refresh".to_string()),
                token_type: Some("Bearer".to_string()),
                expires_at: Some(now_epoch_secs() + 3600),
                scope: Some("useraccount".to_string()),
            },
        );

        let headers = auth.authenticate().await.unwrap();
        assert_eq!(
            headers.get("authorization").unwrap().to_str().unwrap(),
            "Bearer stored_access"
        );
    }

    #[tokio::test]
    async fn test_authorization_code_expired_token_refreshes() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/oauth_token.do"))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("refresh_token=stored_refresh"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(token_response_json("refreshed_access", 3600)),
            )
            .expect(1)
            .mount(&server)
            .await;

        let auth = OAuth2Auth::new_with_stored_token(
            server.uri(),
            "test_client_id".to_string(),
            Some("test_client_secret".to_string()),
            StoredOAuthToken {
                access_token: "expired_access".to_string(),
                refresh_token: Some("stored_refresh".to_string()),
                token_type: Some("Bearer".to_string()),
                expires_at: Some(now_epoch_secs().saturating_sub(60)),
                scope: Some("useraccount".to_string()),
            },
        );

        let headers = auth.authenticate().await.unwrap();
        assert_eq!(
            headers.get("authorization").unwrap().to_str().unwrap(),
            "Bearer refreshed_access"
        );
    }

    #[tokio::test]
    async fn test_authorization_code_expired_token_refreshes_without_client_secret() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/oauth_token.do"))
            .and(body_string(
                "grant_type=refresh_token&client_id=test_client_id&refresh_token=stored_refresh"
                    .to_string(),
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(token_response_json("refreshed_public_access", 3600)),
            )
            .expect(1)
            .mount(&server)
            .await;

        let auth = OAuth2Auth::new_with_stored_token(
            server.uri(),
            "test_client_id".to_string(),
            Option::<String>::None,
            StoredOAuthToken {
                access_token: "expired_access".to_string(),
                refresh_token: Some("stored_refresh".to_string()),
                token_type: Some("Bearer".to_string()),
                expires_at: Some(now_epoch_secs().saturating_sub(60)),
                scope: Some("useraccount".to_string()),
            },
        );

        let headers = auth.authenticate().await.unwrap();
        assert_eq!(
            headers.get("authorization").unwrap().to_str().unwrap(),
            "Bearer refreshed_public_access"
        );
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
