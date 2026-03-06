pub mod error;
pub mod pagination;

use std::time::Duration;

use http::HeaderMap;
use reqwest::{Client, Method, Response};

use crate::client::error::ApiError;

/// Build an authenticated [`SnowClient`] from the user's configuration.
///
/// Loads the config, resolves the active profile, creates the appropriate
/// authenticator, and constructs the client. An optional `instance_override`
/// (from `--instance` CLI flag) replaces the profile's instance URL.
pub fn build_client(
    profile_name: &str,
    instance_override: Option<&str>,
) -> anyhow::Result<SnowClient> {
    build_client_with_timeout(profile_name, instance_override, None)
}

pub fn build_client_with_timeout(
    profile_name: &str,
    instance_override: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<SnowClient> {
    let config = crate::config::AppConfig::load()?;
    let profile = config
        .active_profile(Some(profile_name))
        .ok_or_else(|| anyhow::anyhow!("{}", config.profile_not_found_message(profile_name)))?;

    let instance_url = instance_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| profile.instance.clone());

    let authenticator = crate::auth::create_authenticator(profile_name, profile)?;
    SnowClient::with_config(
        instance_url,
        authenticator,
        ClientConfig {
            timeout_secs: timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS),
        },
    )
}

/// Default request timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 90;

/// Maximum number of retry attempts on 401 (after token refresh).
const MAX_AUTH_RETRIES: u32 = 1;

const HTTP_DEBUG_ENV_VAR: &str = "SNOW_CLI_DEBUG_HTTP";

const HTTP_DEBUG_INCLUDE_SENSITIVE_ENV_VAR: &str = "SNOW_CLI_DEBUG_HTTP_INCLUDE_SENSITIVE";

const HTTP_DEBUG_BODY_PREVIEW_LIMIT: usize = 2048;

fn parse_http_debug_env_value(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    !(normalized.is_empty() || matches!(normalized.as_str(), "0" | "false" | "off" | "no"))
}

pub(crate) fn is_http_debug_enabled() -> bool {
    std::env::var(HTTP_DEBUG_ENV_VAR)
        .map(|value| parse_http_debug_env_value(&value))
        .unwrap_or(false)
}

pub(crate) fn is_http_debug_sensitive_enabled() -> bool {
    std::env::var(HTTP_DEBUG_INCLUDE_SENSITIVE_ENV_VAR)
        .map(|value| parse_http_debug_env_value(&value))
        .unwrap_or(false)
}

fn is_sensitive_header_name(name: &str) -> bool {
    matches!(
        name,
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "x-user-token"
            | "x-usertoken"
            | "x-auth-token"
    )
}

fn format_header_value_for_http_debug(
    name: &str,
    value: &http::HeaderValue,
    include_sensitive: bool,
) -> String {
    if is_sensitive_header_name(name) && !include_sensitive {
        return "<redacted>".to_string();
    }

    match value.to_str() {
        Ok(text) => text.to_string(),
        Err(_) => format!("<{} bytes binary>", value.as_bytes().len()),
    }
}

fn format_headers_for_http_debug(headers: &http::HeaderMap, include_sensitive: bool) -> String {
    let mut lines = headers
        .iter()
        .map(|(name, value)| {
            let name = name.as_str().to_ascii_lowercase();
            let value = format_header_value_for_http_debug(&name, value, include_sensitive);
            format!("{name}: {value}")
        })
        .collect::<Vec<_>>();

    lines.sort();

    if lines.is_empty() {
        "<none>".to_string()
    } else {
        lines.join("\n")
    }
}

fn format_request_body_for_http_debug(request: &reqwest::Request) -> String {
    let body = match request.body() {
        Some(body) => body,
        None => return "<none>".to_string(),
    };

    let bytes = match body.as_bytes() {
        Some(bytes) => bytes,
        None => return "<streaming body>".to_string(),
    };

    if bytes.is_empty() {
        return "<empty>".to_string();
    }

    let preview_len = bytes.len().min(HTTP_DEBUG_BODY_PREVIEW_LIMIT);
    let preview_bytes = &bytes[..preview_len];

    match std::str::from_utf8(preview_bytes) {
        Ok(text) if preview_len < bytes.len() => {
            format!("{text}... <truncated, {} bytes total>", bytes.len())
        }
        Ok(text) => text.to_string(),
        Err(_) => format!("<{} bytes binary>", bytes.len()),
    }
}

pub(crate) fn log_raw_http_request(request: &reqwest::Request) {
    if !is_http_debug_enabled() {
        return;
    }

    let headers =
        format_headers_for_http_debug(request.headers(), is_http_debug_sensitive_enabled());
    let body = format_request_body_for_http_debug(request);

    tracing::debug!(
        target: "snow_cli::http",
        method = %request.method(),
        url = %request.url(),
        headers = %headers,
        body = %body,
        "Raw HTTP request"
    );
}

pub(crate) fn log_raw_http_response(
    url: &str,
    status: reqwest::StatusCode,
    headers: &http::HeaderMap,
) {
    if !is_http_debug_enabled() {
        return;
    }

    let headers = format_headers_for_http_debug(headers, is_http_debug_sensitive_enabled());

    tracing::debug!(
        target: "snow_cli::http",
        url = %url,
        status = status.as_u16(),
        headers = %headers,
        "Raw HTTP response"
    );
}

pub(crate) fn extract_jsessionid_from_headers(headers: &http::HeaderMap) -> Option<String> {
    for header in headers.get_all(reqwest::header::SET_COOKIE) {
        let set_cookie = match header.to_str() {
            Ok(value) => value,
            Err(_) => continue,
        };

        let cookie_pair = set_cookie.split(';').next().unwrap_or(set_cookie);
        let (name, value) = match cookie_pair.split_once('=') {
            Some(parts) => parts,
            None => continue,
        };

        if name.trim().eq_ignore_ascii_case("JSESSIONID") {
            let session_id = value.trim();
            if !session_id.is_empty() {
                return Some(session_id.to_string());
            }
        }
    }

    None
}

pub(crate) fn extract_cookie_header_from_headers(headers: &http::HeaderMap) -> Option<String> {
    let cookies = headers
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|header| {
            let set_cookie = header.to_str().ok()?;
            let cookie_pair = set_cookie.split(';').next()?.trim();
            if cookie_pair.is_empty() || cookie_pair.split_once('=').is_none() {
                None
            } else {
                Some(cookie_pair.to_string())
            }
        })
        .collect::<Vec<_>>();

    if cookies.is_empty() {
        None
    } else {
        Some(cookies.join("; "))
    }
}

fn upsert_cookie_in_header(cookie_header: &str, cookie_name: &str, cookie_value: &str) -> String {
    let mut found = false;

    let mut cookies = cookie_header
        .split(';')
        .filter_map(|cookie| {
            let cookie = cookie.trim();
            if cookie.is_empty() {
                None
            } else {
                Some(cookie.to_string())
            }
        })
        .map(|cookie| {
            if let Some((name, _)) = cookie.split_once('=') {
                if name.trim().eq_ignore_ascii_case(cookie_name) {
                    found = true;
                    return format!("{cookie_name}={cookie_value}");
                }
            }

            cookie
        })
        .collect::<Vec<_>>();

    if !found {
        cookies.push(format!("{cookie_name}={cookie_value}"));
    }

    cookies.join("; ")
}

fn read_cookie_value(cookie_header: &str, cookie_name: &str) -> Option<String> {
    cookie_header.split(';').find_map(|cookie| {
        let (name, value) = cookie.trim().split_once('=')?;
        if name.trim().eq_ignore_ascii_case(cookie_name) {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

fn parse_basic_credentials(auth_headers: &HeaderMap) -> Option<(String, String)> {
    let auth_value = auth_headers
        .get(http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let encoded = auth_value.strip_prefix("Basic ")?;
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (username, password) = decoded.split_once(':')?;
    Some((username.to_string(), password.to_string()))
}

fn logged_in_header_value(headers: &HeaderMap) -> Option<bool> {
    headers
        .get("x-is-logged-in")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.eq_ignore_ascii_case("true"))
}

pub(crate) fn extract_g_ck_from_body(body: &str) -> Option<String> {
    let mut start = 0;

    while let Some(relative_idx) = body[start..].find("g_ck") {
        let token_start = start + relative_idx + "g_ck".len();
        let mut cursor = token_start;

        while let Some(ch) = body[cursor..].chars().next() {
            if ch.is_whitespace() || ch == '"' || ch == '\'' {
                cursor += ch.len_utf8();
            } else {
                break;
            }
        }

        let mut op_found = false;
        let mut inspected = 0usize;
        while let Some(ch) = body[cursor..].chars().next() {
            if ch == '=' || ch == ':' {
                op_found = true;
                cursor += ch.len_utf8();
                break;
            }

            if ch == '\n' || ch == ';' || inspected > 20 {
                break;
            }

            cursor += ch.len_utf8();
            inspected += 1;
        }

        if !op_found {
            start = token_start;
            continue;
        }

        while let Some(ch) = body[cursor..].chars().next() {
            if ch.is_whitespace() {
                cursor += ch.len_utf8();
            } else {
                break;
            }
        }

        let remainder = &body[cursor..];
        if remainder.is_empty() {
            start = token_start;
            continue;
        }

        let first = remainder.chars().next().unwrap_or_default();
        let value = if first == '"' || first == '\'' {
            let quote = first;
            let quoted = &remainder[quote.len_utf8()..];
            quoted.find(quote).map(|end| quoted[..end].to_string())
        } else {
            let end = remainder
                .find(|c: char| c == ';' || c == ',' || c.is_whitespace() || c == '<')
                .unwrap_or(remainder.len());
            Some(
                remainder[..end]
                    .trim_matches(|c| c == '"' || c == '\'' || c == '}')
                    .to_string(),
            )
        };

        if let Some(value) = value {
            if !value.is_empty() {
                return Some(value);
            }
        }

        start = token_start;
    }

    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormSession {
    pub jsessionid: String,
    pub g_ck: String,
    pub cookie_header: String,
}

#[derive(Debug, Default, Clone)]
struct SessionState {
    jsessionid: Option<String>,
    form_session: Option<FormSession>,
}

/// High-level HTTP client for ServiceNow API interactions.
///
/// Wraps `reqwest::Client` with authentication, pagination,
/// and error mapping.
pub struct SnowClient {
    http: Client,
    base_url: String,
    authenticator: Box<dyn crate::auth::Authenticator>,
    session: SessionState,
}

/// Configuration for building a SnowClient.
pub struct ClientConfig {
    /// Request timeout in seconds.
    pub timeout_secs: u64,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }
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
        let http = Client::builder()
            .user_agent(format!("snow-cli/{}", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()?;

        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            authenticator,
            session: SessionState::default(),
        })
    }

    /// Get the base URL for this client.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get a reference to the underlying reqwest client.
    pub fn http(&self) -> &Client {
        &self.http
    }

    /// Get a reference to the authenticator.
    pub fn authenticator(&self) -> &dyn crate::auth::Authenticator {
        self.authenticator.as_ref()
    }

    pub fn jsessionid(&self) -> Option<&str> {
        self.session.jsessionid.as_deref()
    }

    pub fn form_session(&self) -> Option<&FormSession> {
        self.session.form_session.as_ref()
    }

    pub async fn ensure_form_session(
        &mut self,
        bootstrap_path: &str,
    ) -> anyhow::Result<FormSession> {
        if let Some(session) = self.session.form_session.clone() {
            return Ok(session);
        }

        let auth_headers = self.authenticator.authenticate().await?;
        let url = self.url(bootstrap_path);

        tracing::debug!(url = %url, "Bootstrapping form session context");

        let request = self
            .http
            .get(&url)
            .headers(auth_headers.clone())
            .header("Accept", "text/html,application/xhtml+xml")
            .build()?;

        log_raw_http_request(&request);

        let response = self.http.execute(request).await?;

        let status = response.status();
        log_raw_http_response(&url, status, response.headers());
        let response_headers = response.headers().clone();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            anyhow::bail!(
                "Failed to bootstrap form session (status {}) via {}: {}",
                status.as_u16(),
                url,
                body
            );
        }

        if matches!(logged_in_header_value(&response_headers), Some(false)) {
            if let Some((username, password)) = parse_basic_credentials(&auth_headers) {
                tracing::info!(url = %url, "Form bootstrap returned x-is-logged-in=false; attempting explicit login.do form login");

                let cookie_header = self.form_login_cookie_header(&username, &password).await?;
                return self
                    .ensure_form_session_with_cookie(bootstrap_path, &cookie_header)
                    .await;
            }

            anyhow::bail!(
                "Bootstrap at {} returned x-is-logged-in=false. \
                 The current auth method did not establish a UI form session.",
                url
            );
        }

        if let Some(jsessionid) = extract_jsessionid_from_headers(&response_headers) {
            self.session.jsessionid = Some(jsessionid);
        }

        let g_ck = extract_g_ck_from_body(&body).ok_or_else(|| {
            anyhow::anyhow!(
                "Could not extract g_ck token from {} response. Verify the profile user can access Script Background UI.",
                url
            )
        })?;

        let jsessionid = self.session.jsessionid.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "Could not determine JSESSIONID for form session. Ensure the profile is authenticated before running form-based commands."
            )
        })?;

        let cookie_header = extract_cookie_header_from_headers(&response_headers)
            .unwrap_or_else(|| format!("JSESSIONID={jsessionid}"));

        let session = FormSession {
            jsessionid,
            g_ck,
            cookie_header,
        };
        self.session.form_session = Some(session.clone());

        Ok(session)
    }

    async fn form_login_cookie_header(
        &self,
        username: &str,
        password: &str,
    ) -> anyhow::Result<String> {
        let mut login_url = reqwest::Url::parse(&self.url("/login.do"))?;
        login_url
            .query_pairs_mut()
            .append_pair("user_name", username)
            .append_pair("sys_action", "sysverb_login")
            .append_pair("user_password", password);

        tracing::debug!(url = %login_url, "Performing login.do form login");

        let no_redirect_client = reqwest::Client::builder()
            .user_agent(format!("snow-cli/{}", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        let mut current_url = login_url;
        let mut cookie_header = String::new();

        for _ in 0..5 {
            let mut request_builder = no_redirect_client
                .get(current_url.clone())
                .header("Accept", "text/html,application/xhtml+xml");

            if !cookie_header.is_empty() {
                request_builder = request_builder.header("Cookie", cookie_header.clone());
            }

            let request = request_builder.build()?;
            let request_url = request.url().to_string();
            log_raw_http_request(&request);

            let response = no_redirect_client.execute(request).await?;
            let status = response.status();
            log_raw_http_response(&request_url, status, response.headers());

            if let Some(from_response) = extract_cookie_header_from_headers(response.headers()) {
                if cookie_header.is_empty() {
                    cookie_header = from_response;
                } else {
                    for cookie in from_response.split(';') {
                        if let Some((name, value)) = cookie.trim().split_once('=') {
                            cookie_header =
                                upsert_cookie_in_header(&cookie_header, name.trim(), value.trim());
                        }
                    }
                }
            }

            if status.is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| anyhow::anyhow!("login.do redirect missing Location header"))?;
                current_url = current_url.join(location)?;
                continue;
            }

            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                anyhow::bail!(
                    "login.do form login failed with status {} at {}: {}",
                    status.as_u16(),
                    request_url,
                    body
                );
            }

            break;
        }

        if cookie_header.is_empty() {
            anyhow::bail!(
                "login.do form login returned no Set-Cookie headers; cannot establish form session."
            );
        }

        Ok(cookie_header)
    }

    async fn ensure_form_session_with_cookie(
        &mut self,
        bootstrap_path: &str,
        login_cookie_header: &str,
    ) -> anyhow::Result<FormSession> {
        let url = self.url(bootstrap_path);
        let request = self
            .http
            .get(&url)
            .header("Accept", "text/html,application/xhtml+xml")
            .header("Cookie", login_cookie_header)
            .build()?;

        log_raw_http_request(&request);
        let response = self.http.execute(request).await?;
        let status = response.status();
        log_raw_http_response(&url, status, response.headers());
        let response_headers = response.headers().clone();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            anyhow::bail!(
                "Failed to bootstrap form session with login cookies (status {}) via {}: {}",
                status.as_u16(),
                url,
                body
            );
        }

        if matches!(logged_in_header_value(&response_headers), Some(false)) {
            anyhow::bail!(
                "Bootstrap at {} still returned x-is-logged-in=false after login.do.",
                url
            );
        }

        let g_ck = extract_g_ck_from_body(&body).ok_or_else(|| {
            anyhow::anyhow!(
                "Could not extract g_ck token from {} response. Verify the profile user can access Script Background UI.",
                url
            )
        })?;

        let mut cookie_header = login_cookie_header.to_string();
        if let Some(from_bootstrap) = extract_cookie_header_from_headers(&response_headers) {
            for cookie in from_bootstrap.split(';') {
                if let Some((name, value)) = cookie.trim().split_once('=') {
                    cookie_header =
                        upsert_cookie_in_header(&cookie_header, name.trim(), value.trim());
                }
            }
        }

        let jsessionid = read_cookie_value(&cookie_header, "JSESSIONID").ok_or_else(|| {
            anyhow::anyhow!("Could not determine JSESSIONID from login/bootstrap cookies.")
        })?;

        self.session.jsessionid = Some(jsessionid.clone());
        let session = FormSession {
            jsessionid,
            g_ck,
            cookie_header,
        };
        self.session.form_session = Some(session.clone());
        Ok(session)
    }

    /// Build the full URL for an API path.
    ///
    /// If the path starts with `/`, it's treated as absolute on the instance.
    /// Otherwise it's appended to the base URL.
    fn url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else if path.starts_with('/') {
            format!("{}{}", self.base_url, path)
        } else {
            format!("{}/{}", self.base_url, path)
        }
    }

    /// Send an authenticated GET request.
    pub async fn get(&mut self, path: &str) -> anyhow::Result<Response> {
        self.request(Method::GET, path, None, &[]).await
    }

    /// Send an authenticated GET request with query parameters.
    pub async fn get_with_params(
        &mut self,
        path: &str,
        params: &[(&str, &str)],
    ) -> anyhow::Result<Response> {
        self.request(Method::GET, path, None, params).await
    }

    /// Send an authenticated POST request with a JSON body.
    pub async fn post(&mut self, path: &str, body: &str) -> anyhow::Result<Response> {
        self.request(Method::POST, path, Some(body), &[]).await
    }

    /// Send an authenticated PUT request with a JSON body.
    pub async fn put(&mut self, path: &str, body: &str) -> anyhow::Result<Response> {
        self.request(Method::PUT, path, Some(body), &[]).await
    }

    /// Send an authenticated PATCH request with a JSON body.
    pub async fn patch(&mut self, path: &str, body: &str) -> anyhow::Result<Response> {
        self.request(Method::PATCH, path, Some(body), &[]).await
    }

    /// Send an authenticated DELETE request.
    pub async fn delete(&mut self, path: &str) -> anyhow::Result<Response> {
        self.request(Method::DELETE, path, None, &[]).await
    }

    /// Send an authenticated request with custom headers.
    ///
    /// This is used by the `api` raw command to pass user-specified headers.
    pub async fn request_with_headers(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&str>,
        params: &[(&str, &str)],
        extra_headers: &[(String, String)],
    ) -> anyhow::Result<Response> {
        self.request_inner(method, path, body, params, extra_headers)
            .await
    }

    /// Send an authenticated request, with auto-retry on 401.
    async fn request(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&str>,
        params: &[(&str, &str)],
    ) -> anyhow::Result<Response> {
        self.request_inner(method, path, body, params, &[]).await
    }

    /// Internal request implementation with optional extra headers.
    async fn request_inner(
        &mut self,
        method: Method,
        path: &str,
        body: Option<&str>,
        params: &[(&str, &str)],
        extra_headers: &[(String, String)],
    ) -> anyhow::Result<Response> {
        let url = self.url(path);

        for attempt in 0..=MAX_AUTH_RETRIES {
            let auth_headers = self.authenticator.authenticate().await?;

            let mut request = self
                .http
                .request(method.clone(), &url)
                .header("Accept", "application/json")
                .header("Content-Type", "application/json");

            // Add auth headers
            for (key, value) in auth_headers.iter() {
                request = request.header(key, value);
            }

            // Add custom headers (may override defaults like Content-Type)
            for (key, value) in extra_headers {
                request = request.header(key.as_str(), value.as_str());
            }

            // Add query parameters
            if !params.is_empty() {
                request = request.query(params);
            }

            // Add body
            if let Some(body) = body {
                request = request.body(body.to_string());
            }

            tracing::debug!(
                method = %method,
                url = %url,
                attempt = attempt + 1,
                "Sending request"
            );

            let request = request.build()?;
            log_raw_http_request(&request);

            let response = self.http.execute(request).await?;

            if let Some(jsessionid) = extract_jsessionid_from_headers(response.headers()) {
                self.session.jsessionid = Some(jsessionid.clone());
                if let Some(form_session) = self.session.form_session.as_mut() {
                    form_session.jsessionid = jsessionid.clone();
                    form_session.cookie_header = upsert_cookie_in_header(
                        &form_session.cookie_header,
                        "JSESSIONID",
                        &jsessionid,
                    );
                }
                tracing::debug!(
                    url = %url,
                    jsessionid = %jsessionid,
                    "Captured JSESSIONID from response"
                );
            }

            let status = response.status();
            log_raw_http_response(&url, status, response.headers());
            tracing::debug!(
                status = status.as_u16(),
                url = %url,
                "Received response"
            );

            // If unauthorized and we haven't retried yet, try refreshing credentials
            if status == reqwest::StatusCode::UNAUTHORIZED && attempt < MAX_AUTH_RETRIES {
                tracing::info!("Received 401, attempting credential refresh");
                let refreshed = self.authenticator.refresh().await?;
                if refreshed {
                    tracing::debug!("Credentials refreshed, retrying request");
                    continue;
                }
                tracing::debug!("Credential refresh not supported, returning 401 error");
            }

            // Check for error status codes
            if !status.is_success() {
                let status_code = status.as_u16();
                let body_text = response.text().await.ok();
                let api_error =
                    ApiError::from_status(status_code, &self.base_url, body_text.clone());

                tracing::error!(
                    code = %api_error.code,
                    status = status_code,
                    detail = ?body_text,
                    "API request failed"
                );

                return Err(api_error.into());
            }

            return Ok(response);
        }

        unreachable!("Loop should have returned by now")
    }

    /// Send a request and deserialize the JSON response body.
    pub async fn get_json<T: serde::de::DeserializeOwned>(
        &mut self,
        path: &str,
    ) -> anyhow::Result<T> {
        let response = self.get(path).await?;
        let body = response.text().await?;
        tracing::debug!(body_len = body.len(), "Parsing JSON response");
        let value: T = serde_json::from_str(&body)?;
        Ok(value)
    }

    /// Send a request with query params and deserialize the JSON response body.
    pub async fn get_json_with_params<T: serde::de::DeserializeOwned>(
        &mut self,
        path: &str,
        params: &[(&str, &str)],
    ) -> anyhow::Result<T> {
        let response = self.get_with_params(path, params).await?;
        let body = response.text().await?;
        tracing::debug!(body_len = body.len(), "Parsing JSON response");
        let value: T = serde_json::from_str(&body)?;
        Ok(value)
    }

    /// Send a POST request and deserialize the JSON response body.
    pub async fn post_json<T: serde::de::DeserializeOwned>(
        &mut self,
        path: &str,
        body: &str,
    ) -> anyhow::Result<T> {
        let response = self.post(path, body).await?;
        let resp_body = response.text().await?;
        tracing::debug!(body_len = resp_body.len(), "Parsing JSON response");
        let value: T = serde_json::from_str(&resp_body)?;
        Ok(value)
    }

    /// Send a PATCH request and deserialize the JSON response body.
    pub async fn patch_json<T: serde::de::DeserializeOwned>(
        &mut self,
        path: &str,
        body: &str,
    ) -> anyhow::Result<T> {
        let response = self.patch(path, body).await?;
        let resp_body = response.text().await?;
        tracing::debug!(body_len = resp_body.len(), "Parsing JSON response");
        let value: T = serde_json::from_str(&resp_body)?;
        Ok(value)
    }

    /// Fetch paginated records from the Table API.
    ///
    /// Automatically follows pagination using `sysparm_offset` and `sysparm_limit`
    /// until all records are fetched or the configured limit is reached.
    pub async fn get_table_records(
        &mut self,
        table: &str,
        query: Option<&str>,
        fields: Option<&str>,
        pagination: &pagination::PaginationConfig,
        order_by: Option<&str>,
    ) -> anyhow::Result<Vec<crate::models::record::Record>> {
        let path = format!("/api/now/table/{table}");
        let mut all_records = Vec::new();
        let mut offset: usize = 0;
        let page_size = pagination.page_size;
        let limit = pagination.limit;

        loop {
            let mut params: Vec<(&str, String)> = vec![
                ("sysparm_limit", page_size.to_string()),
                ("sysparm_offset", offset.to_string()),
            ];

            if let Some(q) = query {
                params.push(("sysparm_query", q.to_string()));
            }
            if let Some(f) = fields {
                params.push(("sysparm_fields", f.to_string()));
            }
            if let Some(o) = order_by {
                params.push(("sysparm_orderby", o.to_string()));
            }

            // Convert to &str pairs for the request
            let param_refs: Vec<(&str, &str)> =
                params.iter().map(|(k, v)| (*k, v.as_str())).collect();

            let response: crate::models::record::TableResponse =
                self.get_json_with_params(&path, &param_refs).await?;

            let count = response.result.len();
            tracing::debug!(
                table = table,
                offset = offset,
                fetched = count,
                total_so_far = all_records.len() + count,
                "Fetched page"
            );

            all_records.extend(response.result);

            // Check if we've reached the configured limit
            if let Some(lim) = limit {
                if all_records.len() >= lim {
                    all_records.truncate(lim);
                    break;
                }
            }

            // If we got fewer records than the page size, we've fetched everything
            if count < page_size {
                break;
            }

            offset += page_size;
        }

        Ok(all_records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use http::HeaderMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// A mock authenticator for testing.
    /// Injects a fixed Authorization header.
    struct MockAuth {
        token: String,
        refresh_count: Arc<AtomicU32>,
        refresh_succeeds: bool,
    }

    impl MockAuth {
        fn new(token: &str) -> Self {
            Self {
                token: token.to_string(),
                refresh_count: Arc::new(AtomicU32::new(0)),
                refresh_succeeds: false,
            }
        }

        fn with_refresh(mut self) -> Self {
            self.refresh_succeeds = true;
            self
        }

        fn refresh_count(&self) -> Arc<AtomicU32> {
            self.refresh_count.clone()
        }
    }

    #[async_trait]
    impl crate::auth::Authenticator for MockAuth {
        async fn authenticate(&self) -> anyhow::Result<HeaderMap> {
            let mut headers = HeaderMap::new();
            headers.insert(
                http::header::AUTHORIZATION,
                format!("Bearer {}", self.token).parse().unwrap(),
            );
            Ok(headers)
        }

        async fn refresh(&mut self) -> anyhow::Result<bool> {
            self.refresh_count.fetch_add(1, Ordering::SeqCst);
            Ok(self.refresh_succeeds)
        }

        fn auth_type(&self) -> crate::config::AuthMethod {
            crate::config::AuthMethod::Basic
        }
    }

    fn test_client(base_url: &str, auth: MockAuth) -> SnowClient {
        SnowClient::with_config(
            base_url.to_string(),
            Box::new(auth),
            ClientConfig::default(),
        )
        .unwrap()
    }

    #[test]
    fn test_url_building_absolute_path() {
        let auth = MockAuth::new("test");
        let client = test_client("https://test.service-now.com", auth);
        assert_eq!(
            client.url("/api/now/table/incident"),
            "https://test.service-now.com/api/now/table/incident"
        );
    }

    #[test]
    fn test_url_building_relative_path() {
        let auth = MockAuth::new("test");
        let client = test_client("https://test.service-now.com", auth);
        assert_eq!(
            client.url("api/now/table/incident"),
            "https://test.service-now.com/api/now/table/incident"
        );
    }

    #[test]
    fn test_url_building_full_url_passthrough() {
        let auth = MockAuth::new("test");
        let client = test_client("https://test.service-now.com", auth);
        assert_eq!(
            client.url("https://other.service-now.com/api/now/table/incident"),
            "https://other.service-now.com/api/now/table/incident"
        );
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
    fn test_parse_http_debug_env_value() {
        assert!(parse_http_debug_env_value("1"));
        assert!(parse_http_debug_env_value("true"));
        assert!(parse_http_debug_env_value("yes"));
        assert!(!parse_http_debug_env_value("0"));
        assert!(!parse_http_debug_env_value("false"));
        assert!(!parse_http_debug_env_value("off"));
        assert!(!parse_http_debug_env_value(""));
    }

    #[test]
    fn test_format_headers_for_http_debug_redacts_sensitive_headers() {
        let request = reqwest::Client::new()
            .post("https://example.com/api")
            .header("Authorization", "Bearer very-secret")
            .header("Cookie", "JSESSIONID=abc123")
            .header("X-Trace-Id", "trace-42")
            .build()
            .unwrap();

        let headers = format_headers_for_http_debug(request.headers(), false);
        assert!(headers.contains("authorization: <redacted>"));
        assert!(headers.contains("cookie: <redacted>"));
        assert!(headers.contains("x-trace-id: trace-42"));
    }

    #[test]
    fn test_format_headers_for_http_debug_includes_sensitive_headers_when_enabled() {
        let request = reqwest::Client::new()
            .post("https://example.com/api")
            .header("Authorization", "Bearer very-secret")
            .header("Cookie", "JSESSIONID=abc123")
            .header("X-Trace-Id", "trace-42")
            .build()
            .unwrap();

        let headers = format_headers_for_http_debug(request.headers(), true);
        assert!(headers.contains("authorization: Bearer very-secret"));
        assert!(headers.contains("cookie: JSESSIONID=abc123"));
        assert!(headers.contains("x-trace-id: trace-42"));
    }

    #[test]
    fn test_format_request_body_for_http_debug_handles_missing_and_truncated_body() {
        let no_body_request = reqwest::Client::new()
            .get("https://example.com/api")
            .build()
            .unwrap();
        assert_eq!(
            format_request_body_for_http_debug(&no_body_request),
            "<none>"
        );

        let long_body = "x".repeat(HTTP_DEBUG_BODY_PREVIEW_LIMIT + 16);
        let long_body_request = reqwest::Client::new()
            .post("https://example.com/api")
            .body(long_body)
            .build()
            .unwrap();

        let body_preview = format_request_body_for_http_debug(&long_body_request);
        assert!(body_preview.contains("<truncated, "));
    }

    #[test]
    fn test_extract_jsessionid_from_single_cookie_header() {
        let mut headers = HeaderMap::new();
        headers.append(
            reqwest::header::SET_COOKIE,
            http::HeaderValue::from_static("JSESSIONID=abc123; Path=/; HttpOnly; Secure"),
        );

        assert_eq!(
            extract_jsessionid_from_headers(&headers),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn test_extract_jsessionid_from_multiple_set_cookie_headers() {
        let mut headers = HeaderMap::new();
        headers.append(
            reqwest::header::SET_COOKIE,
            http::HeaderValue::from_static("glide_user_route=route123; Path=/"),
        );
        headers.append(
            reqwest::header::SET_COOKIE,
            http::HeaderValue::from_static("JSESSIONID=session456; Path=/; HttpOnly"),
        );

        assert_eq!(
            extract_jsessionid_from_headers(&headers),
            Some("session456".to_string())
        );
    }

    #[test]
    fn test_extract_jsessionid_returns_none_when_missing() {
        let mut headers = HeaderMap::new();
        headers.append(
            reqwest::header::SET_COOKIE,
            http::HeaderValue::from_static("glide_user_route=route123; Path=/"),
        );

        assert_eq!(extract_jsessionid_from_headers(&headers), None);
    }

    #[test]
    fn test_extract_cookie_header_from_multiple_set_cookie_headers() {
        let mut headers = HeaderMap::new();
        headers.append(
            reqwest::header::SET_COOKIE,
            http::HeaderValue::from_static("glide_user_route=route123; Path=/"),
        );
        headers.append(
            reqwest::header::SET_COOKIE,
            http::HeaderValue::from_static("JSESSIONID=session456; Path=/; HttpOnly"),
        );

        assert_eq!(
            extract_cookie_header_from_headers(&headers),
            Some("glide_user_route=route123; JSESSIONID=session456".to_string())
        );
    }

    #[test]
    fn test_extract_cookie_header_returns_none_when_missing() {
        let headers = HeaderMap::new();

        assert_eq!(extract_cookie_header_from_headers(&headers), None);
    }

    #[test]
    fn test_extract_g_ck_from_javascript_assignment() {
        let body = r#"<script>window.g_ck = 'token-123';</script>"#;
        assert_eq!(extract_g_ck_from_body(body), Some("token-123".to_string()));
    }

    #[test]
    fn test_extract_g_ck_from_json_shape() {
        let body = r#"{"g_ck":"abc_xyz_789"}"#;
        assert_eq!(
            extract_g_ck_from_body(body),
            Some("abc_xyz_789".to_string())
        );
    }

    #[test]
    fn test_extract_g_ck_returns_none_when_missing() {
        let body = "<html><body>No token here</body></html>";
        assert_eq!(extract_g_ck_from_body(body), None);
    }

    #[test]
    fn test_upsert_cookie_in_header_replaces_existing_cookie() {
        let header = "glide_user_route=route123; JSESSIONID=old-session";
        let updated = upsert_cookie_in_header(header, "JSESSIONID", "new-session");
        assert_eq!(updated, "glide_user_route=route123; JSESSIONID=new-session");
    }

    #[test]
    fn test_upsert_cookie_in_header_appends_missing_cookie() {
        let header = "glide_user_route=route123";
        let updated = upsert_cookie_in_header(header, "JSESSIONID", "new-session");
        assert_eq!(updated, "glide_user_route=route123; JSESSIONID=new-session");
    }

    #[tokio::test]
    async fn test_ensure_form_session_bootstraps_and_caches_values() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/sys.scripts.modern.do"))
            .and(header("Authorization", "Bearer form-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header(
                        reqwest::header::SET_COOKIE.as_str(),
                        "JSESSIONID=form-session-123; Path=/; HttpOnly",
                    )
                    .set_body_string("<script>var g_ck = 'form-gck-456';</script>"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("form-token"));

        let first = client
            .ensure_form_session("/sys.scripts.modern.do")
            .await
            .unwrap();
        let second = client
            .ensure_form_session("/sys.scripts.modern.do")
            .await
            .unwrap();

        assert_eq!(first.jsessionid, "form-session-123");
        assert_eq!(first.g_ck, "form-gck-456");
        assert_eq!(first.cookie_header, "JSESSIONID=form-session-123");
        assert_eq!(first, second);
        assert_eq!(client.jsessionid(), Some("form-session-123"));
        assert_eq!(
            client.form_session(),
            Some(&FormSession {
                jsessionid: "form-session-123".to_string(),
                g_ck: "form-gck-456".to_string(),
                cookie_header: "JSESSIONID=form-session-123".to_string(),
            })
        );
    }

    #[tokio::test]
    async fn test_ensure_form_session_errors_when_g_ck_missing() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/sys.scripts.modern.do"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header(
                        reqwest::header::SET_COOKIE.as_str(),
                        "JSESSIONID=form-session-123; Path=/; HttpOnly",
                    )
                    .set_body_string("<html><body>no token</body></html>"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("form-token"));
        let result = client.ensure_form_session("/sys.scripts.modern.do").await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Could not extract g_ck token")
        );
    }

    #[tokio::test]
    async fn test_get_sends_auth_header() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(header("Authorization", "Bearer test-token"))
            .and(header("Accept", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("test-token"));
        let response = client.get("/api/now/table/incident").await.unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_get_with_query_params() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_query", "active=true"))
            .and(query_param("sysparm_limit", "10"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let response = client
            .get_with_params(
                "/api/now/table/incident",
                &[("sysparm_query", "active=true"), ("sysparm_limit", "10")],
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_post_sends_body() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/now/table/incident"))
            .and(header("Content-Type", "application/json"))
            .and(header("Authorization", "Bearer post-token"))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_json(serde_json::json!({"result": {"sys_id": "new123"}})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let body = r#"{"short_description":"Test incident"}"#;
        let mut client = test_client(&server.uri(), MockAuth::new("post-token"));
        let response = client.post("/api/now/table/incident", body).await.unwrap();
        assert_eq!(response.status(), 201);
    }

    #[tokio::test]
    async fn test_put_request() {
        let server = MockServer::start().await;

        Mock::given(method("PUT"))
            .and(path("/api/now/table/incident/abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let response = client
            .put("/api/now/table/incident/abc123", r#"{"state":"2"}"#)
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_patch_request() {
        let server = MockServer::start().await;

        Mock::given(method("PATCH"))
            .and(path("/api/now/table/incident/abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let response = client
            .patch("/api/now/table/incident/abc123", r#"{"state":"3"}"#)
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_delete_request() {
        let server = MockServer::start().await;

        Mock::given(method("DELETE"))
            .and(path("/api/now/table/incident/abc123"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let response = client
            .delete("/api/now/table/incident/abc123")
            .await
            .unwrap();
        assert_eq!(response.status(), 204);
    }

    #[tokio::test]
    async fn test_404_returns_api_error() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/nonexistent"))
            .respond_with(ResponseTemplate::new(404).set_body_string("Record not found"))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let result = client.get("/api/now/table/nonexistent").await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        let api_err = err.downcast_ref::<ApiError>().unwrap();
        assert_eq!(api_err.code, "NOT_FOUND");
        assert_eq!(api_err.status, 404);
        assert_eq!(api_err.detail, Some("Record not found".to_string()));
    }

    #[tokio::test]
    async fn test_500_returns_server_error() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal error"))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let result = client.get("/api/now/table/incident").await;
        assert!(result.is_err());

        let api_err = result.unwrap_err().downcast::<ApiError>().unwrap();
        assert_eq!(api_err.code, "SERVER_ERROR");
        assert_eq!(api_err.status, 500);
    }

    #[tokio::test]
    async fn test_401_triggers_refresh_and_retry() {
        let server = MockServer::start().await;

        // First request returns 401, second returns 200 (after refresh)
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;

        let auth = MockAuth::new("token").with_refresh();
        let refresh_count = auth.refresh_count();
        let mut client = test_client(&server.uri(), auth);

        let response = client.get("/api/now/table/incident").await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(refresh_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_401_without_refresh_returns_error() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
            .mount(&server)
            .await;

        // MockAuth without .with_refresh() — refresh returns false
        let auth = MockAuth::new("token");
        let refresh_count = auth.refresh_count();
        let mut client = test_client(&server.uri(), auth);

        let result = client.get("/api/now/table/incident").await;
        assert!(result.is_err());

        let api_err = result.unwrap_err().downcast::<ApiError>().unwrap();
        assert_eq!(api_err.code, "UNAUTHORIZED");
        assert_eq!(refresh_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_get_json_deserializes_response() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [
                    {"sys_id": "abc123", "number": "INC0010001"},
                    {"sys_id": "def456", "number": "INC0010002"}
                ]
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let response: crate::models::record::TableResponse =
            client.get_json("/api/now/table/incident").await.unwrap();

        assert_eq!(response.result.len(), 2);
        assert_eq!(response.result[0].sys_id(), Some("abc123"));
        assert_eq!(response.result[1].get_str("number"), Some("INC0010002"));
    }

    #[tokio::test]
    async fn test_post_json_deserializes_response() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/now/table/incident"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "result": {"sys_id": "new789", "number": "INC0010003"}
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let response: crate::models::record::SingleRecordResponse = client
            .post_json(
                "/api/now/table/incident",
                r#"{"short_description":"New incident"}"#,
            )
            .await
            .unwrap();

        assert_eq!(response.result.sys_id(), Some("new789"));
    }

    #[tokio::test]
    async fn test_get_table_records_single_page() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_limit", "100"))
            .and(query_param("sysparm_offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [
                    {"sys_id": "1", "number": "INC001"},
                    {"sys_id": "2", "number": "INC002"}
                ]
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default();
        let records = client
            .get_table_records("incident", None, None, &pagination, None)
            .await
            .unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].sys_id(), Some("1"));
    }

    #[tokio::test]
    async fn test_get_table_records_with_query_and_fields() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_query", "active=true"))
            .and(query_param("sysparm_fields", "sys_id,number"))
            .and(query_param("sysparm_orderby", "number"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [{"sys_id": "1", "number": "INC001"}]
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default();
        let records = client
            .get_table_records(
                "incident",
                Some("active=true"),
                Some("sys_id,number"),
                &pagination,
                Some("number"),
            )
            .await
            .unwrap();

        assert_eq!(records.len(), 1);
    }

    #[tokio::test]
    async fn test_get_table_records_pagination_multiple_pages() {
        let server = MockServer::start().await;

        // Page 1: 2 records (page_size = 2, so fetches next page)
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_limit", "2"))
            .and(query_param("sysparm_offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [
                    {"sys_id": "1", "number": "INC001"},
                    {"sys_id": "2", "number": "INC002"}
                ]
            })))
            .mount(&server)
            .await;

        // Page 2: 1 record (less than page_size, stops)
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_limit", "2"))
            .and(query_param("sysparm_offset", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [
                    {"sys_id": "3", "number": "INC003"}
                ]
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default().with_page_size(2);
        let records = client
            .get_table_records("incident", None, None, &pagination, None)
            .await
            .unwrap();

        assert_eq!(records.len(), 3);
        assert_eq!(records[0].sys_id(), Some("1"));
        assert_eq!(records[2].sys_id(), Some("3"));
    }

    #[tokio::test]
    async fn test_get_table_records_respects_limit() {
        let server = MockServer::start().await;

        // Returns 3 records per page, but we limit to 2
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [
                    {"sys_id": "1"},
                    {"sys_id": "2"},
                    {"sys_id": "3"}
                ]
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default()
            .with_page_size(10)
            .with_limit(Some(2));

        let records = client
            .get_table_records("incident", None, None, &pagination, None)
            .await
            .unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].sys_id(), Some("1"));
        assert_eq!(records[1].sys_id(), Some("2"));
    }

    #[tokio::test]
    async fn test_get_table_records_empty_result() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": []
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let pagination = pagination::PaginationConfig::default();
        let records = client
            .get_table_records("incident", None, None, &pagination, None)
            .await
            .unwrap();

        assert!(records.is_empty());
    }

    #[tokio::test]
    async fn test_rate_limited_returns_error() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(ResponseTemplate::new(429).set_body_string("Rate limited"))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let result = client.get("/api/now/table/incident").await;
        assert!(result.is_err());

        let api_err = result.unwrap_err().downcast::<ApiError>().unwrap();
        assert_eq!(api_err.code, "RATE_LIMITED");
        assert_eq!(api_err.status, 429);
    }

    #[tokio::test]
    async fn test_forbidden_returns_error() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let result = client.get("/api/now/table/incident").await;
        assert!(result.is_err());

        let api_err = result.unwrap_err().downcast::<ApiError>().unwrap();
        assert_eq!(api_err.code, "FORBIDDEN");
        assert_eq!(api_err.status, 403);
    }

    #[tokio::test]
    async fn test_patch_json_deserializes_response() {
        let server = MockServer::start().await;

        Mock::given(method("PATCH"))
            .and(path("/api/now/table/incident/abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": {"sys_id": "abc123", "state": "2", "number": "INC001"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let response: crate::models::record::SingleRecordResponse = client
            .patch_json("/api/now/table/incident/abc123", r#"{"state":"2"}"#)
            .await
            .unwrap();

        assert_eq!(response.result.sys_id(), Some("abc123"));
        assert_eq!(response.result.get_str("state"), Some("2"));
    }

    #[tokio::test]
    async fn test_get_single_record_with_fields() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/incident/abc123"))
            .and(query_param("sysparm_fields", "sys_id,number"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": {"sys_id": "abc123", "number": "INC001"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let response: crate::models::record::SingleRecordResponse = client
            .get_json_with_params(
                "/api/now/table/incident/abc123",
                &[("sysparm_fields", "sys_id,number")],
            )
            .await
            .unwrap();

        assert_eq!(response.result.sys_id(), Some("abc123"));
        assert_eq!(response.result.get_str("number"), Some("INC001"));
    }

    #[tokio::test]
    async fn test_delete_returns_204_no_content() {
        let server = MockServer::start().await;

        Mock::given(method("DELETE"))
            .and(path("/api/now/table/incident/del123"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let response = client
            .delete("/api/now/table/incident/del123")
            .await
            .unwrap();
        assert_eq!(response.status(), 204);
    }
}
