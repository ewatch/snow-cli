use std::time::Duration;

use http::{HeaderMap, Method};

use super::*;
use crate::models::secret::Secret;

pub(crate) fn extract_jsessionid_from_headers(headers: &http::HeaderMap) -> Option<String> {
    let merged = crate::config::native_session::merge_cookie_header("", headers);
    read_cookie_value(&merged, "JSESSIONID")
}

fn headers_touch_cookie(headers: &http::HeaderMap, cookie_name: &str) -> bool {
    headers
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .any(|h| {
            h.to_str()
                .ok()
                .and_then(|raw| raw.split(';').next())
                .and_then(|pair| pair.split_once('='))
                .is_some_and(|(name, _)| name.trim() == cookie_name)
        })
}

pub(super) fn read_cookie_value(cookie_header: &str, cookie_name: &str) -> Option<String> {
    cookie_header.split(';').find_map(|cookie| {
        let (name, value) = cookie.trim().split_once('=')?;
        if name.trim() == cookie_name {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_string())
        } else {
            None
        }
    })
}

pub(super) fn parse_basic_credentials(auth_headers: &HeaderMap) -> Option<(String, String)> {
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

pub(super) fn logged_in_header_value(headers: &HeaderMap) -> Option<bool> {
    headers
        .get("x-is-logged-in")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.eq_ignore_ascii_case("true"))
}

fn is_js_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'
}

pub(crate) fn extract_g_ck_from_body(body: &str) -> Option<String> {
    let mut start = 0;

    while let Some(relative_idx) = body[start..].find("g_ck") {
        let match_start = start + relative_idx;
        let token_start = match_start + "g_ck".len();
        let prev_is_ident = body[..match_start]
            .chars()
            .next_back()
            .is_some_and(is_js_ident_char);
        let next_is_ident = body[token_start..]
            .chars()
            .next()
            .is_some_and(is_js_ident_char);
        if prev_is_ident || next_is_ident {
            start = token_start;
            continue;
        }
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
        let (value, is_quoted) = if first == '"' || first == '\'' {
            let quote = first;
            let quoted = &remainder[quote.len_utf8()..];
            (
                quoted.find(quote).map(|end| quoted[..end].to_string()),
                true,
            )
        } else {
            let end = remainder
                .find(|c: char| c == ';' || c == ',' || c.is_whitespace() || c == '<')
                .unwrap_or(remainder.len());
            (
                Some(
                    remainder[..end]
                        .trim_matches(|c| c == '"' || c == '\'' || c == '}')
                        .to_string(),
                ),
                false,
            )
        };

        if let Some(value) = value
            && !value.is_empty()
            && (is_quoted || !matches!(value.as_str(), "true" | "false" | "null" | "undefined"))
        {
            return Some(value);
        }

        start = token_start;
    }

    None
}

impl SnowClient {
    pub(super) fn is_same_origin_url(&self, url: &reqwest::Url) -> bool {
        super::core::is_safe_authenticated_url(url)
            && super::core::same_origin(&self.parsed_base_url, url)
    }

    pub(super) fn invalidate_form_session(&mut self) {
        self.session.form_session = None;
        self.session.native_form_bootstrapped = false;
    }

    pub(super) async fn observe_session_response_headers(
        &mut self,
        response_url: &reqwest::Url,
        headers: &http::HeaderMap,
    ) -> anyhow::Result<()> {
        if !self.is_same_origin_url(response_url) {
            tracing::warn!(
                url = %super::debug::redact_url_for_http_debug(response_url),
                "Ignoring session response headers from non-instance origin"
            );
            return Ok(());
        }
        self.authenticator
            .observe_response_headers(headers, Some(response_url))
            .await?;
        if self.authenticator.native_session_credentials().is_none()
            && headers.get(http::header::SET_COOKIE).is_some()
        {
            let base_cookies = self
                .session
                .form_cookie_header
                .as_ref()
                .map(|secret| secret.expose_secret().clone())
                .or_else(|| {
                    self.session
                        .jsessionid
                        .as_ref()
                        .map(|id| format!("JSESSIONID={}", id.expose_secret()))
                })
                .unwrap_or_default();
            let merged = crate::config::native_session::merge_cookie_header_for_url(
                &base_cookies,
                headers,
                Some(response_url),
            );
            if self.session.form_cookie_header.is_some() {
                self.session.form_cookie_header = Some(Secret::new(merged.clone()));
            }
            if headers_touch_cookie(headers, "JSESSIONID") {
                self.session.jsessionid = read_cookie_value(&merged, "JSESSIONID").map(Secret::new);
            }
        }
        Ok(())
    }

    pub(super) fn current_form_cookie_header(&self) -> Option<String> {
        if let Some(native) = self.authenticator.native_session_credentials() {
            return Some(native.cookie_header.expose_secret().to_string());
        }
        self.session
            .form_cookie_header
            .as_ref()
            .map(|secret| secret.expose_secret().clone())
    }

    pub(super) fn current_form_session(&self) -> Option<FormSession> {
        if self.session.native_form_bootstrapped
            && let Some(native) = self.authenticator.native_session_credentials()
        {
            return Some(FormSession {
                g_ck: native.csrf_token.clone(),
            });
        }
        self.session.form_session.clone()
    }

    pub async fn ensure_form_session(
        &mut self,
        bootstrap_path: &str,
    ) -> anyhow::Result<FormSession> {
        self.bootstrap_form_session(bootstrap_path).await?;
        match self.current_form_session() {
            Some(session) => Ok(session),
            None => anyhow::bail!("No form-session CSRF token is available"),
        }
    }

    /// Establish readiness without returning credentials with an error-bearing
    /// transport result. Request snapshots are read separately at dispatch time.
    pub(super) async fn bootstrap_form_session(
        &mut self,
        bootstrap_path: &str,
    ) -> anyhow::Result<()> {
        let url = self.authorize_instance_request(&Method::GET, bootstrap_path)?;
        if self.current_form_session().is_some() {
            return Ok(());
        }

        let redacted_url = super::debug::redact_url_str_for_http_debug(&url);
        tracing::debug!(url = %redacted_url, "Bootstrapping form session context");

        let (response, auth_headers) = self
            .execute_with_auth_retry(false, |http, auth_headers| {
                let mut headers = HeaderMap::new();
                headers.insert(
                    http::header::ACCEPT,
                    http::HeaderValue::from_static("text/html,application/xhtml+xml"),
                );
                super::transport::merge_auth_headers(&mut headers, auth_headers);
                Ok(http.get(&url).headers(headers).build()?)
            })
            .await?;

        let status = response.status();
        let response_url = response.url().clone();
        let response_headers = response.headers().clone();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(super::error::ApiError::from_status(
                status.as_u16(),
                self.base_url(),
                Some(body),
            )
            .into());
        }

        if matches!(logged_in_header_value(&response_headers), Some(false)) {
            if let Some((username, password)) = parse_basic_credentials(&auth_headers) {
                tracing::info!(url = %redacted_url, "Form bootstrap returned x-is-logged-in=false; attempting explicit login.do form login");

                let cookie_header = match self.form_login_cookie_header(&username, &password).await
                {
                    Ok(cookies) => cookies,
                    Err(error) => return Err(error),
                };
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

        let g_ck = super::extract_g_ck_from_body(&body).ok_or_else(|| {
            anyhow::anyhow!(
                "Could not extract g_ck token from {} response. Verify the profile user can access Script Background UI.",
                url
            )
        })?;

        let native = self.authenticator.native_session_credentials();
        // Seed non-native cookie merge from any Cookie header supplied by the
        // authenticator (e.g. BrowserSessionAuth) or prior form cookies so
        // cookies not re-emitted in Set-Cookie are preserved.
        let seeded_cookies = auth_headers
            .get(http::header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .or_else(|| {
                self.session
                    .form_cookie_header
                    .as_ref()
                    .map(|secret| secret.expose_secret().as_str())
            })
            .unwrap_or("");
        let merged_cookies = crate::config::native_session::merge_cookie_header_for_url(
            seeded_cookies,
            &response_headers,
            Some(&response_url),
        );
        let jsessionid = match native.as_ref() {
            Some(session) => read_cookie_value(session.cookie_header.expose_secret(), "JSESSIONID"),
            None => read_cookie_value(&merged_cookies, "JSESSIONID").or_else(|| {
                self.session
                    .jsessionid
                    .as_ref()
                    .map(|secret| secret.expose_secret().clone())
            }),
        };
        let Some(jsessionid) = jsessionid else {
            anyhow::bail!(
                "Could not determine JSESSIONID for form session. Ensure the profile is authenticated before running form-based commands."
            );
        };

        let cookie_header = native
            .as_ref()
            .map(|session| session.cookie_header.expose_secret().to_string())
            .unwrap_or_else(|| {
                if merged_cookies.is_empty() {
                    format!("JSESSIONID={jsessionid}")
                } else {
                    merged_cookies
                }
            });
        if native.is_none() {
            self.session.form_cookie_header = Some(Secret::new(cookie_header));
            self.session.jsessionid = Some(Secret::new(jsessionid));
        }
        self.authenticator.update_native_csrf(&g_ck).await?;

        let session = FormSession {
            g_ck: Secret::new(g_ck),
        };
        if native.is_some() {
            self.session.native_form_bootstrapped = true;
        } else {
            self.session.form_session = Some(session);
        }

        Ok(())
    }

    pub(super) async fn form_login_cookie_header(
        &self,
        username: &str,
        password: &str,
    ) -> anyhow::Result<String> {
        let login_url = reqwest::Url::parse(&self.url("/login.do")?)?;

        tracing::debug!(
            url = %super::debug::redact_url_for_http_debug(&login_url),
            "Performing login.do form login"
        );

        let no_redirect_client = reqwest::Client::builder()
            .user_agent(format!("snow-cli/{}", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(self.timeout_secs))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        let mut current_url = login_url.clone();
        let mut cookie_header = String::new();
        let mut completed = false;

        for hop in 0..5 {
            // Submit credentials strictly on the initial POST (hop 0).
            let mut request_builder = if hop == 0 {
                no_redirect_client
                    .post(current_url.clone())
                    .header("Accept", "text/html,application/xhtml+xml")
                    .form(&[
                        ("user_name", username),
                        ("sys_action", "sysverb_login"),
                        ("user_password", password),
                    ])
            } else {
                no_redirect_client
                    .get(current_url.clone())
                    .header("Accept", "text/html,application/xhtml+xml")
            };

            if !cookie_header.is_empty() {
                request_builder = request_builder.header("Cookie", cookie_header.clone());
            }

            let request = request_builder.build()?;
            log_raw_http_request(&request);

            let response = no_redirect_client.execute(request).await?;
            let status = response.status();
            let response_url = response.url().clone();
            log_raw_http_response(response_url.as_str(), status, response.headers());

            cookie_header = crate::config::native_session::merge_cookie_header_for_url(
                &cookie_header,
                response.headers(),
                Some(&response_url),
            );

            if status.is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| anyhow::anyhow!("login.do redirect missing Location header"))?;
                let next = current_url.join(location)?;
                self.authenticated_url(next.as_str())?;
                if next.path() == "/login.do" {
                    anyhow::bail!(
                        "login.do redirected back to /login.do; authentication failed or session was rejected."
                    );
                }
                current_url = next;
                continue;
            }

            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(super::error::ApiError::from_status(
                    status.as_u16(),
                    self.base_url(),
                    Some(body),
                )
                .into());
            }

            completed = true;
            break;
        }

        if !completed {
            anyhow::bail!("login.do form login exceeded the redirect limit.");
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
    ) -> anyhow::Result<()> {
        let url = self.url(bootstrap_path)?;
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
            return Err(super::error::ApiError::from_status(
                status.as_u16(),
                self.base_url(),
                Some(body),
            )
            .into());
        }

        if matches!(logged_in_header_value(&response_headers), Some(false)) {
            anyhow::bail!(
                "Bootstrap at {} still returned x-is-logged-in=false after login.do.",
                url
            );
        }

        let g_ck = super::extract_g_ck_from_body(&body).ok_or_else(|| {
            anyhow::anyhow!(
                "Could not extract g_ck token from {} response. Verify the profile user can access Script Background UI.",
                url
            )
        })?;

        let bootstrap_url = url::Url::parse(&url).ok();
        let cookie_header = crate::config::native_session::merge_cookie_header_for_url(
            login_cookie_header,
            &response_headers,
            bootstrap_url.as_ref(),
        );

        let Some(jsessionid) = read_cookie_value(&cookie_header, "JSESSIONID") else {
            anyhow::bail!("Could not determine JSESSIONID from login/bootstrap cookies.");
        };

        self.session.jsessionid = Some(Secret::new(jsessionid));
        self.session.form_cookie_header = Some(Secret::new(cookie_header));
        let session = FormSession {
            g_ck: Secret::new(g_ck),
        };
        self.session.form_session = Some(session);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::test_support::*;
    use crate::models::secret::Secret;
    use http::HeaderMap;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn form_session_http_failures_are_typed_and_redacted() {
        let server = MockServer::start().await;
        for (verb, endpoint) in [("GET", "/sys.scripts.modern.do"), ("POST", "/login.do")] {
            Mock::given(method(verb))
                .and(path(endpoint))
                .respond_with(
                    ResponseTemplate::new(403)
                        .set_body_string("<html>private login page; g_ck='session-secret'</html>"),
                )
                .mount(&server)
                .await;
        }
        let mut client = test_client(&server.uri(), MockAuth::new("test-token"));
        // Ephemeral input is used only against the mock; no credential is embedded
        // in the test binary or used with a real instance.
        let password = uuid::Uuid::new_v4().to_string();
        let errors = [
            client
                .ensure_form_session("/sys.scripts.modern.do")
                .await
                .unwrap_err(),
            client
                .ensure_form_session_with_cookie("/sys.scripts.modern.do", "JSESSIONID=test")
                .await
                .unwrap_err(),
            client
                .form_login_cookie_header("user", &password)
                .await
                .unwrap_err(),
        ];
        for error in errors {
            let api = error
                .downcast_ref::<super::super::error::ApiError>()
                .unwrap();
            assert_eq!(api.code, "FORBIDDEN");
            assert_eq!(api.status, 403);
            let rendered = serde_json::to_string(api).unwrap();
            assert!(!rendered.contains("session-secret"));
            assert!(!rendered.contains("private login page"));
            assert!(rendered.contains("response body redacted"));
        }
    }

    #[test]
    fn form_session_debug_redacts_secret_values() {
        let session = FormSession {
            g_ck: Secret::new("gck-secret-value".to_string()),
        };

        let debug = format!("{session:?}");

        assert!(!debug.contains("jsession-secret-value"));
        assert!(!debug.contains("gck-secret-value"));
        assert!(!debug.contains("route-secret"));
        assert!(debug.contains("<redacted>"));
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

        assert_eq!(first.g_ck.expose_secret(), "form-gck-456");
        assert_eq!(
            client.current_form_cookie_header().unwrap(),
            "JSESSIONID=form-session-123"
        );
        assert_eq!(first, second);
        assert_eq!(client.jsessionid(), Some("form-session-123"));
        assert_eq!(
            client.form_session(),
            Some(&FormSession {
                g_ck: Secret::new("form-gck-456".to_string()),
            })
        );
    }

    #[tokio::test]
    async fn native_form_bootstrap_reuses_stored_cookies_without_new_set_cookie() {
        use crate::auth::native_session::NativeSessionAuth;
        use crate::config::native_session::NativeSession;
        use crate::models::secret::Secret;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sys.scripts.modern.do"))
            .and(header("Cookie", "JSESSIONID=existing; route=node1"))
            .respond_with(ResponseTemplate::new(200).set_body_string("g_ck='new-csrf';"))
            .expect(1)
            .mount(&server)
            .await;
        let auth = NativeSessionAuth::transient(NativeSession {
            instance: server.uri(),
            cookie_header: Secret::new("JSESSIONID=existing; route=node1".to_string()),
            csrf_token: Secret::new("old-csrf".to_string()),
            authenticated_user: None,
        });
        let mut client = SnowClient::new(server.uri(), Box::new(auth)).unwrap();
        let form = client
            .ensure_form_session("/sys.scripts.modern.do")
            .await
            .unwrap();
        assert_eq!(form.g_ck.expose_secret(), "new-csrf");
        assert_eq!(
            client.current_form_cookie_header().unwrap(),
            "JSESSIONID=existing; route=node1"
        );
        assert_eq!(
            client
                .native_credentials()
                .await
                .unwrap()
                .csrf_token
                .expose_secret(),
            "new-csrf"
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
    async fn deleted_jsessionid_is_cleared_not_recaptured() {
        let mut client = test_client("https://dev1.service-now.com", MockAuth::new("token"));
        client.session.jsessionid = Some(Secret::new("old-session".to_string()));
        client.session.form_cookie_header = Some(Secret::new(
            "JSESSIONID=old-session; route=node1".to_string(),
        ));

        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::SET_COOKIE,
            http::HeaderValue::from_static("JSESSIONID=old-session; Max-Age=0; Path=/"),
        );

        let url =
            reqwest::Url::parse("https://dev1.service-now.com/api/now/table/incident").unwrap();
        client
            .observe_session_response_headers(&url, &headers)
            .await
            .unwrap();

        assert_eq!(client.jsessionid(), None);
        assert_eq!(
            client.current_form_cookie_header().as_deref(),
            Some("route=node1")
        );
        assert_eq!(extract_jsessionid_from_headers(&headers), None);
    }

    #[tokio::test]
    async fn browser_session_form_bootstrap_preserves_existing_cookies_without_set_cookie() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sys.scripts.modern.do"))
            .and(header(
                "Cookie",
                "JSESSIONID=browser-123; glide_user_route=node7",
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("<script>var g_ck = 'browser-gck-999';</script>"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let auth = crate::auth::browser_session::BrowserSessionAuth::new_with_cookie(
            "JSESSIONID=browser-123; glide_user_route=node7".to_string(),
        );
        let mut client = SnowClient::new(server.uri(), Box::new(auth)).unwrap();
        let session = client
            .ensure_form_session("/sys.scripts.modern.do")
            .await
            .unwrap();
        assert_eq!(session.g_ck.expose_secret(), "browser-gck-999");
        assert_eq!(client.jsessionid(), Some("browser-123"));
        assert_eq!(
            client.current_form_cookie_header().as_deref(),
            Some("JSESSIONID=browser-123; glide_user_route=node7")
        );
    }

    #[tokio::test]
    async fn form_login_fails_on_self_redirect_and_exhausted_redirects() {
        // Case 1: /login.do redirecting back to /login.do fails immediately after 1 POST.
        let self_redirect_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/login.do"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Set-Cookie", "JSESSIONID=rejected; Path=/")
                    .insert_header("Location", "/login.do"),
            )
            .expect(1)
            .mount(&self_redirect_server)
            .await;

        let client = test_client(&self_redirect_server.uri(), MockAuth::new("token"));
        let test_pw = uuid::Uuid::new_v4().to_string();
        let err = client
            .form_login_cookie_header("user", &test_pw)
            .await
            .expect_err("self-redirect to /login.do must fail immediately");
        assert!(err.to_string().contains("/login.do"), "{err}");

        // Case 2: 5 redirects without a 2xx response must error instead of returning Ok.
        let loop_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/login.do"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Set-Cookie", "JSESSIONID=step0; Path=/")
                    .insert_header("Location", "/step1"),
            )
            .expect(1)
            .mount(&loop_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/step1"))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", "/step1"))
            .expect(4)
            .mount(&loop_server)
            .await;

        let client = test_client(&loop_server.uri(), MockAuth::new("token"));
        let err = client
            .form_login_cookie_header("user", &test_pw)
            .await
            .expect_err("exhausted redirects must error");
        assert!(err.to_string().contains("redirect limit"), "{err}");
    }

    #[test]
    fn extract_g_ck_respects_identifier_boundaries_and_ignores_booleans() {
        assert_eq!(extract_g_ck_from_body("var flag_ck = 'nope';"), None);
        assert_eq!(extract_g_ck_from_body("var g_ck_enabled = true;"), None);
        assert_eq!(extract_g_ck_from_body("var g_ck = null;"), None);
        assert_eq!(
            extract_g_ck_from_body(
                "var flag_ck = 'bad'; var g_ck_enabled = true; window.g_ck = 'real-token';"
            ),
            Some("real-token".to_string())
        );
    }
}
