use std::time::Duration;

use bytes::Bytes;
use http::header::{HeaderName, HeaderValue};
use http::{HeaderMap, Method};
use reqwest::{Client, Response as TransportResponse, Url};

use crate::cli::spinner::SnowflakeSpinner;
use crate::client::error::{ApiError, GraphqlError};

use super::core::{
    endpoint_requires_form_session, is_safe_authenticated_url, validate_external_url,
};
use super::debug::redact_url_str_for_http_debug;
use super::*;

const MAX_AUTH_RETRIES: u32 = 1;
const MAX_EXTERNAL_REDIRECT_HOPS: usize = 10;

async fn buffered_external_response(
    response: TransportResponse,
) -> anyhow::Result<ExternalResponse> {
    let status = response.status();
    let final_url = response.url().clone();
    validate_external_url(&final_url, "follow redirects")?;
    log_raw_http_response(final_url.as_str(), status, response.headers());
    let body = response.bytes().await?.to_vec();
    Ok(ExternalResponse {
        status: status.as_u16(),
        final_url: final_url.to_string(),
        body,
    })
}

/// Post an unauthenticated OAuth token form to a validated endpoint.
pub async fn post_oauth_token_form(url: &str, body: &str) -> anyhow::Result<ExternalResponse> {
    let url = Url::parse(url)?;
    validate_external_url(&url, "request OAuth tokens")?;
    let http = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("snow-cli/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .build()?;
    let request = http
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body.to_string())
        .build()?;
    log_raw_http_request(&request);
    buffered_external_response(http.execute(request).await?).await
}

/// Fetch one unauthenticated remote skill resource from a validated URL.
pub async fn fetch_skill_resource(url: &url::Url) -> anyhow::Result<ExternalResponse> {
    validate_external_url(url, "fetch a skill resource")?;
    let redirect_policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= MAX_EXTERNAL_REDIRECT_HOPS {
            return attempt.error("Too many redirects");
        }
        if !is_safe_authenticated_url(attempt.url()) {
            return attempt.error(
                "Refusing to follow redirect to unsafe URL; external HTTP requires HTTPS except loopback HTTP",
            );
        }
        attempt.follow()
    });
    let http = Client::builder()
        .redirect(redirect_policy)
        .user_agent(format!("snow-cli/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .build()?;
    let request = http.get(url.clone()).header("Accept", "*/*").build()?;
    log_raw_http_request(&request);
    buffered_external_response(http.execute(request).await?).await
}

impl SnowClient {
    /// Build the full URL for an API path.
    ///
    /// If the path starts with `/`, it's treated as absolute on the instance.
    /// Otherwise it's appended to the base URL.
    pub(super) fn url(&self, path: &str) -> anyhow::Result<String> {
        self.authenticated_url(path)
    }

    pub(crate) fn authenticated_url(&self, path: &str) -> anyhow::Result<String> {
        resolve_authenticated_url(&self.base_url, path)
    }

    /// Apply the common policy and origin checks required before any
    /// authenticated ServiceNow network operation.
    pub(super) fn authorize_instance_request(
        &self,
        method: &Method,
        path: &str,
    ) -> anyhow::Result<String> {
        self.policy.ensure_request_allowed(method, path)?;
        self.authenticated_url(path)
    }

    /// Execute an authenticated request with a shared 401 refresh-retry loop
    /// and same-origin session header observation.
    pub(super) async fn execute_with_auth_retry<F>(
        &mut self,
        use_streaming_client: bool,
        mut build_request: F,
    ) -> anyhow::Result<(TransportResponse, HeaderMap)>
    where
        F: FnMut(&Client, &HeaderMap) -> anyhow::Result<reqwest::Request>,
    {
        for attempt in 0..=MAX_AUTH_RETRIES {
            let auth_headers = self
                .authenticator
                .authenticate()
                .await
                .map_err(crate::auth::classify_auth_failure)?;
            let http = if use_streaming_client {
                &self.http_streaming
            } else {
                &self.http
            };
            let request = build_request(http, &auth_headers)?;
            let redacted_url = redact_url_str_for_http_debug(request.url().as_str());
            tracing::debug!(
                method = %request.method(),
                url = %redacted_url,
                attempt = attempt + 1,
                "Sending request"
            );
            log_raw_http_request(&request);

            let response = http.execute(request).await?;
            let status = response.status();
            log_raw_http_response(response.url().as_str(), status, response.headers());
            let same_origin_response = self.is_same_origin_url(response.url());
            self.observe_session_response_headers(response.url(), response.headers())
                .await?;
            if same_origin_response && extract_jsessionid_from_headers(response.headers()).is_some()
            {
                tracing::debug!(
                    url = %redacted_url,
                    has_jsessionid = true,
                    "Captured JSESSIONID from response"
                );
            }
            tracing::debug!(
                status = status.as_u16(),
                url = %redacted_url,
                "Received response"
            );

            if status == reqwest::StatusCode::UNAUTHORIZED && attempt < MAX_AUTH_RETRIES {
                tracing::info!("Received 401, attempting credential refresh");
                let refreshed = self
                    .authenticator
                    .refresh()
                    .await
                    .map_err(crate::auth::classify_auth_failure)?;
                if refreshed {
                    tracing::debug!("Credentials refreshed, retrying request");
                    continue;
                }
                tracing::debug!("Credential refresh not supported, returning 401 error");
            }

            return Ok((response, auth_headers));
        }

        unreachable!("Loop should have returned by now")
    }

    /// Upload an attachment through the authenticated ServiceNow attachment interface.
    pub async fn upload_attachment(
        &mut self,
        table: &str,
        table_sys_id: &str,
        file_name: &str,
        file_bytes: Vec<u8>,
    ) -> anyhow::Result<ClientResponse> {
        const PATH: &str = "/api/now/attachment/upload";
        let url = self.authorize_instance_request(&Method::POST, PATH)?;
        let file_bytes = Bytes::from(file_bytes);
        let file_len = file_bytes.len() as u64;
        let (response, _) = self
            .execute_with_auth_retry(false, |http, auth_headers| {
                let file_part = reqwest::multipart::Part::stream_with_length(
                    reqwest::Body::from(file_bytes.clone()),
                    file_len,
                )
                .file_name(file_name.to_string());
                let form = reqwest::multipart::Form::new()
                    .text("table_name", table.to_string())
                    .text("table_sys_id", table_sys_id.to_string())
                    .text("file_name", file_name.to_string())
                    .part("file", file_part);
                let mut headers = HeaderMap::new();
                headers.insert(
                    http::header::ACCEPT,
                    HeaderValue::from_static("application/json"),
                );
                merge_auth_headers(&mut headers, auth_headers);
                Ok(http
                    .post(&url)
                    .query(&[
                        ("table_name", table),
                        ("table_sys_id", table_sys_id),
                        ("file_name", file_name),
                    ])
                    .headers(headers)
                    .multipart(form)
                    .build()?)
            })
            .await?;
        Ok(ClientResponse { inner: response })
    }

    /// Start a streaming attachment download through the authenticated client seam.
    pub async fn download_attachment(&mut self, path: &str) -> anyhow::Result<ClientResponse> {
        let url = self.authorize_instance_request(&Method::GET, path)?;
        let (response, _) = self
            .execute_with_auth_retry(true, |http, auth_headers| {
                let mut headers = HeaderMap::new();
                headers.insert(http::header::ACCEPT, HeaderValue::from_static("*/*"));
                merge_auth_headers(&mut headers, auth_headers);
                Ok(http.get(&url).headers(headers).build()?)
            })
            .await?;
        Ok(ClientResponse { inner: response })
    }

    /// Execute a background script using the form or JSON protocol required by the endpoint.
    pub async fn execute_background_script(
        &mut self,
        endpoint: &str,
        script: &str,
        scope: &str,
        options: BackgroundScriptOptions,
    ) -> anyhow::Result<BackgroundScriptResponse> {
        let url = self.authorize_instance_request(&Method::POST, endpoint)?;
        let mut redactions = super::script_response::ScriptRedactions::default();

        let response = if endpoint_requires_form_session(endpoint) {
            let mut form_attempt = 0;
            loop {
                self.bootstrap_form_session(FORM_SCRIPT_BOOTSTRAP_PATH)
                    .await?;
                let Some(session) = self.current_form_session() else {
                    anyhow::bail!("No form-session CSRF token is available");
                };
                let Some(cookie_header) = self.current_form_cookie_header() else {
                    anyhow::bail!("No form-session cookies are available");
                };
                let mut form_fields = vec![
                    ("script", script.to_string()),
                    ("runscript", "Run script".to_string()),
                    ("sysparm_ck", session.g_ck.clone()),
                    ("sys_scope", scope.to_string()),
                ];
                if options.rollback {
                    form_fields.push(("record_for_rollback", "on".to_string()));
                }
                if options.sandbox {
                    form_fields.push(("sandbox", "on".to_string()));
                }
                if options.scriptlet {
                    form_fields.push(("scriptlet", "on".to_string()));
                }
                if options.quota_managed_transaction {
                    form_fields.push(("quota_managed_transaction", "on".to_string()));
                }

                let request = self
                    .http
                    .post(&url)
                    .header("Accept", "text/html,application/xhtml+xml")
                    .header("Cookie", cookie_header)
                    .header("X-UserToken", session.g_ck)
                    .form(&form_fields)
                    .build()?;

                redactions.observe_request(request.headers());
                log_raw_http_request(&request);
                let response = self.http.execute(request).await?;
                log_raw_http_response(
                    response.url().as_str(),
                    response.status(),
                    response.headers(),
                );
                redactions.observe_response(response.headers());
                self.observe_session_response_headers(response.url(), response.headers())
                    .await?;

                let session_stale = response.status() == reqwest::StatusCode::UNAUTHORIZED
                    || super::session::logged_in_header_value(response.headers()) == Some(false);
                if session_stale {
                    self.invalidate_form_session();
                    if response.status() == reqwest::StatusCode::UNAUTHORIZED
                        && form_attempt < MAX_AUTH_RETRIES
                    {
                        form_attempt += 1;
                        let _ = self
                            .authenticator
                            .refresh()
                            .await
                            .map_err(crate::auth::classify_auth_failure)?;
                        continue;
                    }
                }
                break response;
            }
        } else {
            let body = serde_json::to_string(&serde_json::json!({
                "script": script,
                "scope": scope
            }))?;
            let (response, _) = self
                .execute_with_auth_retry(false, |http, auth_headers| {
                    let mut headers = HeaderMap::new();
                    headers.insert(
                        http::header::ACCEPT,
                        HeaderValue::from_static("application/json"),
                    );
                    headers.insert(
                        http::header::CONTENT_TYPE,
                        HeaderValue::from_static("application/json"),
                    );
                    merge_auth_headers(&mut headers, auth_headers);
                    let request = http
                        .post(&url)
                        .headers(headers)
                        .body(body.clone())
                        .build()?;
                    redactions.observe_request(request.headers());
                    Ok(request)
                })
                .await?;
            redactions.observe_response(response.headers());
            response
        };

        let status = response.status().as_u16();
        let final_url = response.url().to_string();
        let server_timing = response
            .headers()
            .get("server-timing")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = redactions.redact(response.text().await?);
        Ok(BackgroundScriptResponse {
            status,
            final_url,
            server_timing,
            body,
        })
    }

    /// Send an authenticated GET request.
    pub async fn get(&mut self, path: &str) -> anyhow::Result<ClientResponse> {
        self.request(Method::GET, path, None, &[]).await
    }

    /// Send an authenticated GET request with query parameters.
    pub async fn get_with_params(
        &mut self,
        path: &str,
        params: &[(&str, &str)],
    ) -> anyhow::Result<ClientResponse> {
        self.request(Method::GET, path, None, params).await
    }

    /// Send an authenticated POST request with a JSON body.
    pub async fn post(&mut self, path: &str, body: &str) -> anyhow::Result<ClientResponse> {
        self.request(Method::POST, path, Some(body), &[]).await
    }

    /// Send an authenticated PUT request with a JSON body.
    pub async fn put(&mut self, path: &str, body: &str) -> anyhow::Result<ClientResponse> {
        self.request(Method::PUT, path, Some(body), &[]).await
    }

    /// Send an authenticated PATCH request with a JSON body.
    pub async fn patch(&mut self, path: &str, body: &str) -> anyhow::Result<ClientResponse> {
        self.request(Method::PATCH, path, Some(body), &[]).await
    }

    /// Send an authenticated DELETE request.
    pub async fn delete(&mut self, path: &str) -> anyhow::Result<ClientResponse> {
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
    ) -> anyhow::Result<ClientResponse> {
        self.policy
            .ensure_raw_api_headers_allowed(&method, extra_headers)?;
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
    ) -> anyhow::Result<ClientResponse> {
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
    ) -> anyhow::Result<ClientResponse> {
        let url = self.authorize_instance_request(&method, path)?;
        let spinner = SnowflakeSpinner::start("Waiting for ServiceNow response...");
        let result = self
            .execute_with_auth_retry(false, |http, auth_headers| {
                let mut headers = HeaderMap::new();
                headers.insert(
                    http::header::ACCEPT,
                    HeaderValue::from_static("application/json"),
                );
                if body.is_some() {
                    headers.insert(
                        http::header::CONTENT_TYPE,
                        HeaderValue::from_static("application/json"),
                    );
                }
                // Extra headers replace defaults (never append duplicate Content-Type/Accept).
                for (key, value) in extra_headers {
                    let name = HeaderName::from_bytes(key.trim().as_bytes())
                        .map_err(|_| anyhow::anyhow!("Invalid HTTP header name '{key}'"))?;
                    let val = HeaderValue::from_str(value)
                        .map_err(|_| anyhow::anyhow!("Invalid HTTP header value for '{key}'"))?;
                    headers.insert(name, val);
                }
                // Apply auth headers last via replace so extra headers cannot duplicate
                // or shadow authenticator credentials.
                merge_auth_headers(&mut headers, auth_headers);

                let mut request = http.request(method.clone(), &url).headers(headers);
                if !params.is_empty() {
                    request = request.query(params);
                }
                if let Some(body) = body {
                    request = request.body(body.to_string());
                }
                Ok(request.build()?)
            })
            .await;
        drop(spinner);

        let (response, _) = result?;
        let status = response.status();

        // Check for error status codes
        if !status.is_success() {
            let status_code = status.as_u16();
            let body_text = response.text().await.ok();
            let body_len = body_text.as_ref().map_or(0, String::len);
            let api_error = ApiError::from_status(status_code, &self.base_url, body_text);

            // The structured error on stderr is the user-facing report. The raw
            // body stays out of logs; SNOW_CLI_DEBUG_HTTP is the opt-in way to see it.
            tracing::debug!(
                code = %api_error.code,
                status = status_code,
                body_len,
                "API request failed"
            );

            return Err(api_error.into());
        }

        Ok(ClientResponse { inner: response })
    }

    /// Send a request and deserialize the JSON response body.
    pub async fn get_json<T: serde::de::DeserializeOwned>(
        &mut self,
        path: &str,
    ) -> anyhow::Result<T> {
        let response = self.get(path).await?;
        let body = response.text().await?;
        tracing::debug!(body_len = body.len(), "Parsing JSON response");
        let value: T = serde_json::from_str(&body)
            .map_err(|_| super::error::TransportError::InvalidResponse)?;
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
        let value: T = serde_json::from_str(&body)
            .map_err(|_| super::error::TransportError::InvalidResponse)?;
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
        let value: T = serde_json::from_str(&resp_body)
            .map_err(|_| super::error::TransportError::InvalidResponse)?;
        Ok(value)
    }

    /// Execute a document against the fixed Now GraphQL endpoint.
    ///
    /// The request uses the shared authenticated POST path and returns only the
    /// GraphQL `data` value. A non-empty `errors` array is returned as a
    /// sanitized [`GraphqlError`], and partial data is deliberately suppressed.
    pub async fn execute_graphql(
        &mut self,
        query: &str,
        variables: &serde_json::Map<String, serde_json::Value>,
    ) -> anyhow::Result<serde_json::Value> {
        let body = serde_json::to_string(&serde_json::json!({
            "query": query,
            "variables": variables,
        }))?;
        let response = self.post("/api/now/graphql", &body).await?;
        let response_body = response.text().await?;
        tracing::debug!(
            body_len = response_body.len(),
            "Parsing GraphQL response envelope"
        );
        let envelope: serde_json::Value = serde_json::from_str(&response_body)
            .map_err(|_| super::error::TransportError::InvalidResponse)?;
        graphql_data_from_envelope(envelope)
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
        let value: T = serde_json::from_str(&resp_body)
            .map_err(|_| super::error::TransportError::InvalidResponse)?;
        Ok(value)
    }
}

pub(super) fn merge_auth_headers(target: &mut HeaderMap, auth_headers: &HeaderMap) {
    for key in auth_headers.keys() {
        target.remove(key);
    }
    for (key, value) in auth_headers.iter() {
        target.append(key.clone(), value.clone());
    }
}

pub(super) fn graphql_data_from_envelope(
    envelope: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let object = envelope
        .as_object()
        .ok_or(super::error::TransportError::InvalidEnvelope(
            "GraphQL endpoint returned a non-object response envelope",
        ))?;

    if let Some(errors) = object.get("errors") {
        let errors = errors
            .as_array()
            .ok_or(super::error::TransportError::InvalidEnvelope(
                "GraphQL endpoint returned an invalid errors field",
            ))?;
        if !errors.is_empty() {
            return Err(GraphqlError::from_errors(errors).into());
        }
    }

    object.get("data").cloned().ok_or_else(|| {
        super::error::TransportError::InvalidEnvelope(
            "GraphQL endpoint response did not contain data",
        )
        .into()
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::error::{ApiError, GraphqlError};
    use crate::client::test_support::*;
    use crate::policy::ExecutionPolicy;
    use http::Method;
    use std::sync::atomic::Ordering;
    use wiremock::matchers::{body_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// The read-only policy travels with the client: a client built with a
    /// read-only `ClientConfig` denies a write request regardless of the
    /// process-global policy state.
    #[tokio::test]
    async fn read_only_client_policy_denies_write_without_global_state() {
        let mut client = SnowClient::with_config(
            "https://test.service-now.com".to_string(),
            Box::new(MockAuth::new("token")),
            ClientConfig {
                policy: ExecutionPolicy::read_only(),
                ..ClientConfig::default()
            },
        )
        .unwrap();

        let error = client
            .post("/api/now/table/incident", "{}")
            .await
            .expect_err("read-only client must reject POST");
        let policy_error = error
            .downcast_ref::<crate::policy::PolicyError>()
            .expect("error should be a PolicyError");
        assert_eq!(policy_error.mode, crate::policy::PolicyMode::ReadOnly);
    }

    #[tokio::test]
    async fn read_only_client_denies_attachment_upload_before_network_io() {
        let mut client = SnowClient::with_config(
            "https://test.service-now.com".to_string(),
            Box::new(MockAuth::new("token")),
            ClientConfig {
                policy: ExecutionPolicy::read_only(),
                ..ClientConfig::default()
            },
        )
        .unwrap();

        let error = client
            .upload_attachment("incident", "abc123", "test.txt", b"payload".to_vec())
            .await
            .expect_err("read-only client must reject attachment upload");
        assert!(error.downcast_ref::<crate::policy::PolicyError>().is_some());
    }

    #[tokio::test]
    async fn read_only_client_denies_background_script_before_bootstrap() {
        let mut client = SnowClient::with_config(
            "https://test.service-now.com".to_string(),
            Box::new(MockAuth::new("token")),
            ClientConfig {
                policy: ExecutionPolicy::read_only(),
                ..ClientConfig::default()
            },
        )
        .unwrap();

        let error = client
            .execute_background_script(
                FORM_SCRIPT_ENDPOINT,
                "gs.info('test')",
                "global",
                BackgroundScriptOptions::default(),
            )
            .await
            .expect_err("read-only client must reject script execution");
        assert!(error.downcast_ref::<crate::policy::PolicyError>().is_some());
    }

    #[tokio::test]
    async fn read_only_client_denies_raw_get_method_override() {
        let mut client = SnowClient::with_config(
            "https://test.service-now.com".to_string(),
            Box::new(MockAuth::new("token")),
            ClientConfig {
                policy: ExecutionPolicy::read_only(),
                ..ClientConfig::default()
            },
        )
        .unwrap();
        let headers = vec![("X-HTTP-Method-Override".to_string(), "DELETE".to_string())];

        let error = client
            .request_with_headers(Method::GET, "/api/now/table/incident", None, &[], &headers)
            .await
            .expect_err("read-only client must reject method override");
        assert!(error.downcast_ref::<crate::policy::PolicyError>().is_some());
    }

    /// A full-access client (the constructor default) permits writes even when
    /// no global policy has been configured.
    #[tokio::test]
    async fn full_access_client_policy_allows_write() {
        let server = MockServer::start().await;
        Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/api/now/table/incident"))
            .respond_with(ResponseTemplate::new(201))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        client
            .post("/api/now/table/incident", "{}")
            .await
            .expect("full-access client must allow POST");
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
        assert_eq!(
            api_err.detail,
            Some("<response body redacted, 16 bytes>".to_string())
        );
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
    async fn execute_graphql_posts_exact_envelope_and_returns_data() {
        let server = MockServer::start().await;
        let query = "query Incident($number: String!) { incident(number: $number) { number } }";
        let variables = serde_json::json!({"number": "INC0010001"});

        Mock::given(method("POST"))
            .and(path("/api/now/graphql"))
            .and(header("Accept", "application/json"))
            .and(header("Content-Type", "application/json"))
            .and(header("Authorization", "Bearer graphql-token"))
            .and(body_json(serde_json::json!({
                "query": query,
                "variables": variables
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {"incident": {"number": "INC0010001"}}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("graphql-token"));
        let variables = variables.as_object().unwrap().clone();
        let data = client.execute_graphql(query, &variables).await.unwrap();

        assert_eq!(data["incident"]["number"], "INC0010001");
    }

    #[tokio::test]
    async fn execute_graphql_preserves_null_data() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/now/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": null
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let data = client
            .execute_graphql("{ nullable }", &serde_json::Map::new())
            .await
            .unwrap();
        assert!(data.is_null());
    }

    #[tokio::test]
    async fn execute_graphql_converts_response_errors_without_partial_data() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/now/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {"partialSecret": "must-not-be-retained"},
                "errors": [{
                    "message": "Field is not accessible",
                    "path": ["partialSecret"],
                    "extensions": {"debug": "raw-extension-secret"}
                }]
            })))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let error = client
            .execute_graphql("{ restricted }", &serde_json::Map::new())
            .await
            .unwrap_err()
            .downcast::<GraphqlError>()
            .unwrap();
        let detail = error.detail.as_deref().unwrap();

        assert_eq!(detail, "Field is not accessible");
        assert!(!detail.contains("must-not-be-retained"));
        assert!(!detail.contains("raw-extension-secret"));
        assert!(!detail.contains("partialSecret"));
    }

    #[tokio::test]
    async fn execute_graphql_rejects_malformed_success_envelopes() {
        let invalid_json_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/now/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not-json secret-body"))
            .mount(&invalid_json_server)
            .await;
        let mut client = test_client(&invalid_json_server.uri(), MockAuth::new("token"));
        let error = client
            .execute_graphql("{ malformed }", &serde_json::Map::new())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid JSON"));
        assert!(!error.contains("secret-body"));

        let missing_data_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/now/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&missing_data_server)
            .await;
        let mut client = test_client(&missing_data_server.uri(), MockAuth::new("token"));
        let error = client
            .execute_graphql("{ malformed }", &serde_json::Map::new())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("did not contain data"));
    }

    #[tokio::test]
    async fn execute_graphql_keeps_http_failures_as_api_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/now/graphql"))
            .respond_with(ResponseTemplate::new(403).set_body_string("forbidden"))
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        let error = client
            .execute_graphql("{ restricted }", &serde_json::Map::new())
            .await
            .unwrap_err()
            .downcast::<ApiError>()
            .unwrap();
        assert_eq!(error.code, "FORBIDDEN");
        assert_eq!(error.status, 403);
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

    #[tokio::test]
    async fn post_oauth_token_form_does_not_follow_307_or_308_redirects() {
        let origin = MockServer::start().await;
        let attacker = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/steal"))
            .respond_with(ResponseTemplate::new(200).set_body_string("stolen"))
            .expect(0)
            .mount(&attacker)
            .await;

        for (status, endpoint) in [(307, "/oauth_token_307.do"), (308, "/oauth_token_308.do")] {
            Mock::given(method("POST"))
                .and(path(endpoint))
                .respond_with(
                    ResponseTemplate::new(status)
                        .insert_header("Location", format!("{}/steal", attacker.uri())),
                )
                .expect(1)
                .mount(&origin)
                .await;

            let response = post_oauth_token_form(
                &format!("{}{endpoint}", origin.uri()),
                "grant_type=client_credentials&client_secret=super-secret",
            )
            .await
            .unwrap();
            assert_eq!(response.status, status);
            assert_eq!(response.final_url, format!("{}{endpoint}", origin.uri()));
        }
    }

    #[tokio::test]
    async fn snow_client_blocks_cross_origin_307_308_and_downgrade_redirects_pre_hop() {
        let origin = MockServer::start().await;
        let foreign = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/capture"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(0)
            .mount(&foreign)
            .await;

        for (status, endpoint) in [(307, "/api/307"), (308, "/api/308")] {
            Mock::given(method("POST"))
                .and(path(endpoint))
                .respond_with(
                    ResponseTemplate::new(status)
                        .insert_header("Location", format!("{}/capture", foreign.uri())),
                )
                .expect(1)
                .mount(&origin)
                .await;

            let mut client = test_client(&origin.uri(), MockAuth::new("secret-token"));
            let headers = vec![("X-UserToken".to_string(), "csrf-secret".to_string())];
            let err = client
                .request_with_headers(
                    Method::POST,
                    endpoint,
                    Some(r#"{"sysparm_ck":"csrf-secret"}"#),
                    &[],
                    &headers,
                )
                .await
                .expect_err("cross-origin 307/308 redirect must be rejected before hop");
            assert!(
                err.to_string().contains("error sending request")
                    || err.to_string().contains("redirect")
                    || format!("{err:?}").contains("cross-origin"),
                "unexpected error: {err:?}"
            );
        }

        // Non-loopback plaintext HTTP redirect target (HTTPS-to-HTTP / unsafe downgrade)
        Mock::given(method("GET"))
            .and(path("/api/downgrade"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", "http://127.attacker.example/steal"),
            )
            .expect(1)
            .mount(&origin)
            .await;

        let mut client = test_client(&origin.uri(), MockAuth::new("secret-token"));
        let err = client
            .get("/api/downgrade")
            .await
            .expect_err("unsafe HTTP redirect target must be rejected before hop");
        assert!(
            format!("{err:?}").contains("unsafe") || format!("{err:?}").contains("cross-origin"),
            "unexpected error: {err:?}"
        );
    }

    #[tokio::test]
    async fn fetch_skill_resource_blocks_unsafe_redirect_before_hop() {
        let origin = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/skill.md"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", "http://127.attacker.example/evil.md"),
            )
            .expect(1)
            .mount(&origin)
            .await;

        let url = Url::parse(&format!("{}/skill.md", origin.uri())).unwrap();
        let err = fetch_skill_resource(&url)
            .await
            .expect_err("redirect to non-loopback HTTP must fail before hop");
        assert!(
            format!("{err:?}").contains("unsafe URL"),
            "unexpected error: {err:?}"
        );
    }

    #[tokio::test]
    async fn observe_session_response_headers_ignores_foreign_origin() {
        let mut client = test_client("https://dev1.service-now.com", MockAuth::new("token"));
        client.session.jsessionid = Some("original-session".to_string());
        client.session.form_cookie_header = Some("JSESSIONID=original-session".to_string());

        let mut headers = http::HeaderMap::new();
        headers.insert(
            reqwest::header::SET_COOKIE,
            http::HeaderValue::from_static("JSESSIONID=attacker-session; Path=/"),
        );

        let foreign = Url::parse("https://attacker.example/callback").unwrap();
        client
            .observe_session_response_headers(&foreign, &headers)
            .await
            .unwrap();
        assert_eq!(client.jsessionid(), Some("original-session"));
        assert_eq!(
            client.session.form_cookie_header.as_deref(),
            Some("JSESSIONID=original-session")
        );

        let same = Url::parse("https://dev1.service-now.com/api/now/table/incident").unwrap();
        client
            .observe_session_response_headers(&same, &headers)
            .await
            .unwrap();
        assert_eq!(client.jsessionid(), Some("attacker-session"));
    }

    #[tokio::test]
    async fn request_with_headers_overrides_defaults_without_duplication_and_omits_get_content_type()
     {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/x/get"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/api/x/post"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;

        let mut client = test_client(&server.uri(), MockAuth::new("token"));
        client
            .request_with_headers(
                Method::GET,
                "/api/x/get",
                None,
                &[],
                &[("Accept".to_string(), "text/plain".to_string())],
            )
            .await
            .unwrap();

        client
            .request_with_headers(
                Method::POST,
                "/api/x/post",
                Some("<xml/>"),
                &[],
                &[
                    ("Content-Type".to_string(), "text/xml".to_string()),
                    ("Accept".to_string(), "application/xml".to_string()),
                ],
            )
            .await
            .unwrap();

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);

        // GET: single Accept: text/plain, no Content-Type
        let get_accepts: Vec<&str> = requests[0]
            .headers
            .get_all("accept")
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect();
        assert_eq!(get_accepts, vec!["text/plain"]);
        assert!(requests[0].headers.get("content-type").is_none());

        // POST: single Content-Type: text/xml, single Accept: application/xml
        let post_cts: Vec<&str> = requests[1]
            .headers
            .get_all("content-type")
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect();
        assert_eq!(post_cts, vec!["text/xml"]);
        let post_accepts: Vec<&str> = requests[1]
            .headers
            .get_all("accept")
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect();
        assert_eq!(post_accepts, vec!["application/xml"]);
    }

    #[tokio::test]
    async fn upload_download_and_script_retry_on_401_with_refresh() {
        let server = MockServer::start().await;

        // Upload: 401 -> refresh -> 201
        Mock::given(method("POST"))
            .and(path("/api/now/attachment/upload"))
            .respond_with(ResponseTemplate::new(401))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/now/attachment/upload"))
            .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
            .expect(1)
            .mount(&server)
            .await;

        // Download: 401 -> refresh -> 200
        Mock::given(method("GET"))
            .and(path("/api/now/attachment/att1/file"))
            .respond_with(ResponseTemplate::new(401))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/now/attachment/att1/file"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello"))
            .expect(1)
            .mount(&server)
            .await;

        // JSON script endpoint: 401 -> refresh -> 200
        Mock::given(method("POST"))
            .and(path("/api/x_custom/script"))
            .respond_with(ResponseTemplate::new(401))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/x_custom/script"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"ok":true}"#))
            .expect(1)
            .mount(&server)
            .await;

        let auth = MockAuth::new("token").with_refresh();
        let refresh_count = auth.refresh_count();
        let mut client = test_client(&server.uri(), auth);

        let upload = client
            .upload_attachment("incident", "sys1", "note.txt", b"payload".to_vec())
            .await
            .unwrap();
        assert_eq!(upload.status(), 201);

        let download = client
            .download_attachment("/api/now/attachment/att1/file")
            .await
            .unwrap();
        assert_eq!(download.status(), 200);
        assert_eq!(download.text().await.unwrap(), "hello");

        let script = client
            .execute_background_script(
                "/api/x_custom/script",
                "gs.info('hi')",
                "global",
                BackgroundScriptOptions::default(),
            )
            .await
            .unwrap();
        assert_eq!(script.status(), 200);
        assert_eq!(refresh_count.load(Ordering::SeqCst), 3);
    }
}
