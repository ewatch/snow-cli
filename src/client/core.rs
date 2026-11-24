use std::fmt;
use std::time::Duration;

use http::HeaderMap;
use reqwest::{Client, Response as TransportResponse, Url};

use crate::policy::ExecutionPolicy;

use super::{DEFAULT_TIMEOUT_SECS, FORM_SCRIPT_ENDPOINT};

#[derive(Clone, PartialEq, Eq)]
pub struct FormSession {
    pub g_ck: String,
}

impl fmt::Debug for FormSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FormSession")
            .field("g_ck", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Default, Clone)]
pub(super) struct SessionState {
    pub(super) jsessionid: Option<String>,
    pub(super) form_cookie_header: Option<String>,
    // Native credentials live only in the authenticator; remember readiness, not secrets.
    pub(super) native_form_bootstrapped: bool,
    pub(super) form_session: Option<FormSession>,
}

/// Opaque response returned by authenticated ServiceNow operations.
///
/// Transport details stay private to the client module while callers retain
/// the status, selected headers, body, and streaming behavior they need.
#[derive(Debug)]
pub struct ClientResponse {
    pub(super) inner: TransportResponse,
}

impl ClientResponse {
    pub fn status(&self) -> u16 {
        self.inner.status().as_u16()
    }

    pub fn headers(&self) -> &HeaderMap {
        self.inner.headers()
    }

    pub fn final_url(&self) -> &str {
        self.inner.url().as_str()
    }

    pub fn content_length(&self) -> Option<u64> {
        self.inner.content_length()
    }

    pub async fn text(self) -> anyhow::Result<String> {
        Ok(self.inner.text().await?)
    }

    pub async fn next_chunk(&mut self) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self.inner.chunk().await?.map(|chunk| chunk.to_vec()))
    }
}

/// Buffered result of an explicitly unauthenticated external HTTP operation.
#[derive(Debug)]
pub struct ExternalResponse {
    pub status: u16,
    pub final_url: String,
    pub body: Vec<u8>,
}

impl ExternalResponse {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Optional execution flags for a ServiceNow background script request.
#[derive(Debug, Default, Clone, Copy)]
pub struct BackgroundScriptOptions {
    pub rollback: bool,
    pub sandbox: bool,
    pub scriptlet: bool,
    pub quota_managed_transaction: bool,
}

/// High-level HTTP client for ServiceNow API interactions.
///
/// Hides the HTTP transport behind authentication, policy enforcement,
/// pagination, and error mapping.
pub struct SnowClient {
    pub(super) http: Client,
    pub(super) base_url: String,
    pub(super) authenticator: Box<dyn crate::auth::Authenticator>,
    pub(super) session: SessionState,
    pub(super) timeout_secs: u64,
    /// Execution policy enforced on every outbound request.
    ///
    /// The policy travels with the client: it is fixed at construction time and
    /// checked per request in `request_inner`, so the read-only HTTP backstop
    /// never depends on process-global state being set at request time.
    pub(super) policy: ExecutionPolicy,
}

impl fmt::Debug for SnowClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnowClient")
            .field("base_url", &self.base_url)
            .field("auth_type", &self.authenticator.auth_type())
            .field("has_jsessionid", &self.session.jsessionid.is_some())
            .field("has_form_session", &self.session.form_session.is_some())
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

/// Configuration for building a SnowClient.
#[derive(Debug)]
pub struct ClientConfig {
    /// Request timeout in seconds.
    pub timeout_secs: u64,
    /// Execution policy the resulting client enforces on every request.
    ///
    /// Defaults to full access. The CLI entry point (`build_client_with_timeout`)
    /// snapshots the active policy into this field; direct callers get full
    /// access by design unless they opt into a stricter policy.
    pub policy: ExecutionPolicy,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            policy: ExecutionPolicy::full_access(),
        }
    }
}

/// Maximum number of same-origin redirects allowed for authenticated non-native requests.
const MAX_REDIRECT_HOPS: usize = 10;

pub(super) fn is_safe_authenticated_url(url: &Url) -> bool {
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    match url.scheme() {
        "https" => true,
        "http" => crate::config::profile::is_loopback_url(url),
        _ => false,
    }
}

pub(super) fn endpoint_requires_form_session(endpoint: &str) -> bool {
    if let Ok(url) = Url::parse(endpoint) {
        return url
            .path()
            .trim_end_matches('/')
            .ends_with(FORM_SCRIPT_ENDPOINT);
    }

    endpoint
        .split(['?', '#'])
        .next()
        .unwrap_or(endpoint)
        .trim_end_matches('/')
        .ends_with(FORM_SCRIPT_ENDPOINT)
}

pub(super) fn same_origin(left: &Url, right: &Url) -> bool {
    left.origin() == right.origin()
}

pub(super) fn validate_external_url(url: &Url, operation: &str) -> anyhow::Result<()> {
    if is_safe_authenticated_url(url) {
        return Ok(());
    }

    anyhow::bail!(
        "Refusing to {operation} from unsafe URL '{}'. External HTTP requires HTTPS, except loopback HTTP for local tests.",
        url
    )
}

pub(crate) fn resolve_authenticated_url(base_url: &str, path: &str) -> anyhow::Result<String> {
    let base = Url::parse(base_url)
        .map_err(|error| anyhow::anyhow!("Invalid instance URL '{}': {}", base_url, error))?;
    if !is_safe_authenticated_url(&base) {
        anyhow::bail!(
            "Refusing to use unsafe instance URL '{}'. Authenticated requests require HTTPS, except loopback HTTP for local tests.",
            base_url
        );
    }

    let candidate = if path.starts_with("http://") || path.starts_with("https://") {
        Url::parse(path)
            .map_err(|error| anyhow::anyhow!("Invalid request URL '{}': {}", path, error))?
    } else if path.starts_with('/') {
        base.join(path)?
    } else {
        base.join(&format!("/{path}"))?
    };

    if !is_safe_authenticated_url(&candidate) {
        anyhow::bail!(
            "Refusing to send credentials to unsafe URL '{}'. Authenticated requests require HTTPS, except loopback HTTP for local tests.",
            candidate
        );
    }

    if !same_origin(&base, &candidate) {
        anyhow::bail!(
            "Refusing to send credentials to non-instance URL '{}'. Expected same origin as '{}'.",
            candidate,
            base
        );
    }

    Ok(candidate.to_string())
}

impl SnowClient {
    /// Create a new client for the given instance URL and authenticator.
    pub fn new(
        base_url: String,
        authenticator: Box<dyn crate::auth::Authenticator>,
    ) -> anyhow::Result<Self> {
        Self::with_config(base_url, authenticator, ClientConfig::default())
    }

    /// Create a new client with custom configuration.
    pub fn with_config(
        base_url: String,
        authenticator: Box<dyn crate::auth::Authenticator>,
        config: ClientConfig,
    ) -> anyhow::Result<Self> {
        let base_url = crate::config::profile::validate_instance_url(&base_url)?;
        let parsed_base = Url::parse(&base_url)
            .map_err(|error| anyhow::anyhow!("Invalid instance URL '{}': {}", base_url, error))?;
        if !is_safe_authenticated_url(&parsed_base) {
            anyhow::bail!(
                "Refusing to use unsafe instance URL '{}'. Authenticated requests require HTTPS, except loopback HTTP for local tests.",
                base_url
            );
        }

        if let Some(session) = authenticator.native_session_credentials() {
            session.ensure_instance(&base_url)?;
        }
        let redirect_policy =
            if authenticator.auth_type() == crate::config::AuthMethod::NativeSession {
                reqwest::redirect::Policy::none()
            } else {
                let expected_base = parsed_base.clone();
                reqwest::redirect::Policy::custom(move |attempt| {
                    if attempt.previous().len() >= MAX_REDIRECT_HOPS {
                        return attempt.error("Too many redirects");
                    }
                    if !is_safe_authenticated_url(attempt.url())
                        || !same_origin(&expected_base, attempt.url())
                    {
                        return attempt
                            .error("Refusing to follow redirect to unsafe or cross-origin URL");
                    }
                    attempt.follow()
                })
            };
        let http = Client::builder()
            .redirect(redirect_policy)
            .user_agent(format!("snow-cli/{}", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()?;

        Ok(Self {
            http,
            base_url,
            authenticator,
            session: SessionState::default(),
            timeout_secs: config.timeout_secs,
            policy: config.policy,
        })
    }

    /// Get the base URL for this client.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn jsessionid(&self) -> Option<&str> {
        self.session.jsessionid.as_deref()
    }

    pub fn form_session(&self) -> Option<&FormSession> {
        self.session.form_session.as_ref()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::test_support::*;

    #[test]
    fn test_url_building_absolute_path() {
        let auth = MockAuth::new("test");
        let client = test_client("https://test.service-now.com", auth);
        assert_eq!(
            client.url("/api/now/table/incident").unwrap(),
            "https://test.service-now.com/api/now/table/incident"
        );
    }

    #[test]
    fn test_url_building_relative_path() {
        let auth = MockAuth::new("test");
        let client = test_client("https://test.service-now.com", auth);
        assert_eq!(
            client.url("api/now/table/incident").unwrap(),
            "https://test.service-now.com/api/now/table/incident"
        );
    }

    #[test]
    fn test_url_building_allows_same_origin_full_url() {
        let auth = MockAuth::new("test");
        let client = test_client("https://test.service-now.com", auth);
        assert_eq!(
            client
                .authenticated_url("https://test.service-now.com/api/now/table/incident")
                .unwrap(),
            "https://test.service-now.com/api/now/table/incident"
        );
    }

    #[test]
    fn test_url_building_rejects_off_origin_full_url() {
        let auth = MockAuth::new("test");
        let client = test_client("https://test.service-now.com", auth);
        let err = client
            .authenticated_url("https://other.service-now.com/api/now/table/incident")
            .unwrap_err()
            .to_string();
        assert!(err.contains("non-instance URL"));
    }

    #[test]
    fn test_url_building_strips_trailing_slash() {
        let auth = MockAuth::new("test");
        let client = test_client("https://test.service-now.com/", auth);
        assert_eq!(client.base_url(), "https://test.service-now.com");
    }

    #[test]
    fn test_default_client_config() {
        let config = ClientConfig::default();
        assert_eq!(config.timeout_secs, DEFAULT_TIMEOUT_SECS);
    }

    #[test]
    fn test_is_safe_authenticated_url_loopback_and_ipv6() {
        for safe in [
            "https://dev12345.service-now.com/api/now/table/incident",
            "http://127.0.0.1:8080/api",
            "http://127.0.0.2:8080/api",
            "http://localhost:8080/api",
            "http://[::1]:8080/api",
        ] {
            let url = Url::parse(safe).unwrap();
            assert!(is_safe_authenticated_url(&url), "expected safe URL: {safe}");
        }

        for unsafe_url in [
            "http://127.attacker.example/api",
            "http://127.0.0.1.nip.io/api",
            "http://localhost.attacker.example/api",
            "http://dev12345.service-now.com/api",
            "https://user:pass@dev12345.service-now.com/api",
        ] {
            let url = Url::parse(unsafe_url).unwrap();
            assert!(
                !is_safe_authenticated_url(&url),
                "expected unsafe URL to be rejected: {unsafe_url}"
            );
        }
    }
}
