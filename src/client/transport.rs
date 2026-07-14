use std::time::Duration;

use http::Method;
use reqwest::{Client, Response as TransportResponse, Url};

use crate::cli::spinner::SnowflakeSpinner;
use crate::client::error::{ApiError, GraphqlError};

use super::core::{endpoint_requires_form_session, validate_external_url};
use super::session::upsert_cookie_in_header;
use super::*;

const MAX_AUTH_RETRIES: u32 = 1;

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
    let http = Client::builder()
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
        let auth_headers = self.authenticator.authenticate().await?;
        let file_part =
            reqwest::multipart::Part::bytes(file_bytes).file_name(file_name.to_string());
        let form = reqwest::multipart::Form::new()
            .text("table_name", table.to_string())
            .text("table_sys_id", table_sys_id.to_string())
            .text("file_name", file_name.to_string())
            .part("file", file_part);
        let request = self
            .http
            .post(&url)
            .query(&[
                ("table_name", table),
                ("table_sys_id", table_sys_id),
                ("file_name", file_name),
            ])
            .header("Accept", "application/json")
            .headers(auth_headers)
            .multipart(form)
            .build()?;
        log_raw_http_request(&request);
        let response = self.http.execute(request).await?;
        log_raw_http_response(&url, response.status(), response.headers());
        Ok(ClientResponse { inner: response })
    }

    /// Start a streaming attachment download through the authenticated client seam.
    pub async fn download_attachment(&mut self, path: &str) -> anyhow::Result<ClientResponse> {
        let url = self.authorize_instance_request(&Method::GET, path)?;
        let auth_headers = self.authenticator.authenticate().await?;
        let request = self
            .http
            .get(&url)
            .header("Accept", "*/*")
            .headers(auth_headers)
            .build()?;
        log_raw_http_request(&request);
        let response = self.http.execute(request).await?;
        log_raw_http_response(&url, response.status(), response.headers());
        Ok(ClientResponse { inner: response })
    }

    /// Execute a background script using the form or JSON protocol required by the endpoint.
    pub async fn execute_background_script(
        &mut self,
        endpoint: &str,
        script: &str,
        scope: &str,
        options: BackgroundScriptOptions,
    ) -> anyhow::Result<ClientResponse> {
        let url = self.authorize_instance_request(&Method::POST, endpoint)?;

        let request = if endpoint_requires_form_session(endpoint) {
            let session = self.ensure_form_session(FORM_SCRIPT_BOOTSTRAP_PATH).await?;
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

            self.http
                .post(&url)
                .header("Accept", "text/html,application/xhtml+xml")
                .header("Cookie", session.cookie_header)
                .header("X-UserToken", session.g_ck)
                .form(&form_fields)
                .build()?
        } else {
            let auth_headers = self.authenticator.authenticate().await?;
            let body = serde_json::json!({ "script": script, "scope": scope });
            self.http
                .post(&url)
                .headers(auth_headers)
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .body(serde_json::to_string(&body)?)
                .build()?
        };

        log_raw_http_request(&request);
        let response = self.http.execute(request).await?;
        log_raw_http_response(
            response.url().as_str(),
            response.status(),
            response.headers(),
        );
        Ok(ClientResponse { inner: response })
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

            let spinner = SnowflakeSpinner::start("Waiting for ServiceNow response...");
            let response = self.http.execute(request).await?;
            drop(spinner);

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
                    has_jsessionid = true,
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

            return Ok(ClientResponse { inner: response });
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
            .map_err(|error| anyhow::anyhow!("GraphQL endpoint returned invalid JSON: {error}"))?;
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
        let value: T = serde_json::from_str(&resp_body)?;
        Ok(value)
    }
}

pub(super) fn graphql_data_from_envelope(
    envelope: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let object = envelope.as_object().ok_or_else(|| {
        anyhow::anyhow!("GraphQL endpoint returned a non-object response envelope")
    })?;

    if let Some(errors) = object.get("errors") {
        let errors = errors
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("GraphQL endpoint returned an invalid errors field"))?;
        if !errors.is_empty() {
            return Err(GraphqlError::from_errors(errors).into());
        }
    }

    object
        .get("data")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("GraphQL endpoint response did not contain data"))
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
}
