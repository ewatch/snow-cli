use std::time::Duration;

use http::{HeaderMap, Method};

use super::*;

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

pub(super) fn upsert_cookie_in_header(
    cookie_header: &str,
    cookie_name: &str,
    cookie_value: &str,
) -> String {
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
            if let Some((name, _)) = cookie.split_once('=')
                && name.trim().eq_ignore_ascii_case(cookie_name)
            {
                found = true;
                return format!("{cookie_name}={cookie_value}");
            }

            cookie
        })
        .collect::<Vec<_>>();

    if !found {
        cookies.push(format!("{cookie_name}={cookie_value}"));
    }

    cookies.join("; ")
}

pub(super) fn read_cookie_value(cookie_header: &str, cookie_name: &str) -> Option<String> {
    cookie_header.split(';').find_map(|cookie| {
        let (name, value) = cookie.trim().split_once('=')?;
        if name.trim().eq_ignore_ascii_case(cookie_name) {
            Some(value.trim().to_string())
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

        if let Some(value) = value
            && !value.is_empty()
        {
            return Some(value);
        }

        start = token_start;
    }

    None
}

impl SnowClient {
    pub async fn ensure_form_session(
        &mut self,
        bootstrap_path: &str,
    ) -> anyhow::Result<FormSession> {
        let url = self.authorize_instance_request(&Method::GET, bootstrap_path)?;
        if let Some(session) = self.session.form_session.clone() {
            return Ok(session);
        }

        let auth_headers = self.authenticator.authenticate().await?;

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

        let g_ck = super::extract_g_ck_from_body(&body).ok_or_else(|| {
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

        let cookie_header = super::extract_cookie_header_from_headers(&response_headers)
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
        let login_url = reqwest::Url::parse(&self.url("/login.do")?)?;

        tracing::debug!(url = %login_url, "Performing login.do form login");

        let no_redirect_client = reqwest::Client::builder()
            .user_agent(format!("snow-cli/{}", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        let mut current_url = login_url.clone();
        let mut cookie_header = String::new();

        for _ in 0..5 {
            let is_login_submission = current_url == login_url;
            let mut request_builder = if is_login_submission {
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
            let request_url = request.url().to_string();
            log_raw_http_request(&request);

            let response = no_redirect_client.execute(request).await?;
            let status = response.status();
            log_raw_http_response(&request_url, status, response.headers());

            if let Some(from_response) =
                super::extract_cookie_header_from_headers(response.headers())
            {
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

        let g_ck = super::extract_g_ck_from_body(&body).ok_or_else(|| {
            anyhow::anyhow!(
                "Could not extract g_ck token from {} response. Verify the profile user can access Script Background UI.",
                url
            )
        })?;

        let mut cookie_header = login_cookie_header.to_string();
        if let Some(from_bootstrap) = super::extract_cookie_header_from_headers(&response_headers) {
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
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::test_support::*;
    use http::HeaderMap;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn form_session_debug_redacts_secret_values() {
        let session = FormSession {
            jsessionid: "jsession-secret-value".to_string(),
            g_ck: "gck-secret-value".to_string(),
            cookie_header: "JSESSIONID=jsession-secret-value; glide_user_route=route-secret"
                .to_string(),
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
}
