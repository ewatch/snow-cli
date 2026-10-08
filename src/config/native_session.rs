//! Instance-bound native session credentials. Only the OS keychain persists secrets.
use super::credentials;
use crate::models::secret::Secret;
use http::HeaderMap;
use serde::{Deserialize, Serialize};

pub const CREDENTIAL_TYPE: &str = "native_session";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeSession {
    pub instance: String,
    pub cookie_header: Secret,
    pub csrf_token: Secret,
    pub authenticated_user: Option<String>,
}

impl NativeSession {
    pub fn ensure_instance(&self, instance: &str) -> anyhow::Result<()> {
        let expected = url::Url::parse(&self.instance)?.origin();
        let actual = url::Url::parse(instance)?.origin();
        anyhow::ensure!(
            expected == actual,
            "Native session belongs to a different instance; refusing to send session credentials"
        );
        Ok(())
    }

    pub fn headers(&self) -> anyhow::Result<HeaderMap> {
        anyhow::ensure!(
            !self.cookie_header.expose_secret().is_empty(),
            "Native session has expired or has no cookies; log in or attach again"
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::COOKIE,
            self.cookie_header.expose_secret().parse()?,
        );
        if !self.csrf_token.expose_secret().is_empty() {
            headers.insert("X-UserToken", self.csrf_token.expose_secret().parse()?);
        }
        Ok(headers)
    }

    /// Apply rotations/deletions without retaining Set-Cookie attributes.
    pub fn merge_response_cookies(&mut self, headers: &HeaderMap) -> bool {
        let mut cookies = self
            .cookie_header
            .expose_secret()
            .split(';')
            .filter_map(|cookie| cookie.trim().split_once('='))
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect::<Vec<_>>();
        let before = cookies.clone();
        let now = std::time::SystemTime::now();
        for header in headers.get_all(http::header::SET_COOKIE) {
            let Ok(value) = header.to_str() else {
                continue;
            };
            let mut parts = value.split(';');
            let Some((name, value)) = parts.next().and_then(|part| part.trim().split_once('='))
            else {
                continue;
            };
            let mut max_age_expired = None;
            let mut expires = None;
            for attribute in parts {
                let Some((key, value)) = attribute.trim().split_once('=') else {
                    continue;
                };
                if key.eq_ignore_ascii_case("max-age") {
                    let value = value.trim();
                    let digits = value.strip_prefix('-').unwrap_or(value);
                    if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
                        max_age_expired =
                            Some(value.starts_with('-') || digits.bytes().all(|byte| byte == b'0'));
                    }
                } else if key.eq_ignore_ascii_case("expires")
                    && let Ok(date) = httpdate::parse_http_date(value.trim())
                {
                    expires = Some(date);
                }
            }
            // RFC 6265 gives a valid Max-Age precedence over Expires.
            let delete = value.is_empty()
                || max_age_expired.unwrap_or_else(|| expires.is_some_and(|date| date <= now));
            cookies.retain(|(key, _)| !key.eq_ignore_ascii_case(name));
            if !delete {
                cookies.push((name.to_string(), value.to_string()));
            }
        }
        if cookies != before {
            self.cookie_header = Secret::new(
                cookies
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            );
            true
        } else {
            false
        }
    }
}

pub fn load(profile: &str) -> anyhow::Result<NativeSession> {
    let secret =
        credentials::snapshot_stored_credential(profile, CREDENTIAL_TYPE)?.ok_or_else(|| {
            anyhow::anyhow!(
                "No native session stored for '{profile}'. Use session login or session attach."
            )
        })?;
    serde_json::from_str(&secret)
        .map_err(|_| anyhow::anyhow!("Stored native session is invalid; log in or attach again"))
}

pub fn save(profile: &str, session: &NativeSession) -> anyhow::Result<()> {
    credentials::store_credential(profile, CREDENTIAL_TYPE, &serde_json::to_string(session)?)
}

pub fn forget(profile: &str) -> anyhow::Result<()> {
    credentials::restore_stored_credential(profile, CREDENTIAL_TYPE, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_expiry_attributes_respect_precedence_and_invalid_values() {
        for (attributes, deleted) in [
            ("Expires=Thu, 01 Jan 1970 00:00:00 GMT", true),
            ("eXpIrEs=Thu, 01 Jan 1970 00:00:00 GMT", true),
            ("Expires=Fri, 01 Jan 2100 00:00:00 GMT", false),
            ("Expires=invalid", false),
            ("Max-Age=0", true),
            ("Max-Age=-1", true),
            ("Max-Age=3600", false),
            ("Max-Age=-0", true),
            ("Max-Age=0000", true),
            ("Max-Age=-999999999999999999999999", true),
            (
                "Max-Age=999999999999999999999999; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
                false,
            ),
            ("Max-Age=+3600; Expires=Thu, 01 Jan 1970 00:00:00 GMT", true),
            ("Max-Age=-; Expires=Fri, 01 Jan 2100 00:00:00 GMT", false),
            ("Max-Age=1x; Expires=Thu, 01 Jan 1970 00:00:00 GMT", true),
            ("Expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=3600", false),
            ("Max-Age=3600; Expires=Thu, 01 Jan 1970 00:00:00 GMT", false),
            ("Expires=Fri, 01 Jan 2100 00:00:00 GMT; Max-Age=0", true),
            (
                "Max-Age=invalid; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
                true,
            ),
            ("Max-Age=invalid; Expires=invalid", false),
        ] {
            let mut session = NativeSession {
                instance: "https://example.service-now.com".to_string(),
                cookie_header: Secret::new("route=node1; JSESSIONID=old".to_string()),
                csrf_token: Secret::new(String::new()),
                authenticated_user: None,
            };
            let mut headers = HeaderMap::new();
            headers.insert(
                http::header::SET_COOKIE,
                format!("JSESSIONID=old; {attributes}; Path=/")
                    .parse()
                    .unwrap(),
            );
            assert_eq!(
                session.merge_response_cookies(&headers),
                deleted,
                "{attributes}"
            );
            assert_eq!(
                session.cookie_header.expose_secret(),
                if deleted {
                    "route=node1"
                } else {
                    "route=node1; JSESSIONID=old"
                },
                "{attributes}"
            );
        }
    }

    #[test]
    fn expired_nonempty_cookie_is_removed() {
        let mut session = NativeSession {
            instance: "https://example.service-now.com".to_string(),
            cookie_header: Secret::new("JSESSIONID=old; route=node1".to_string()),
            csrf_token: Secret::new(String::new()),
            authenticated_user: None,
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::SET_COOKIE,
            "JSESSIONID=old; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Path=/"
                .parse()
                .unwrap(),
        );
        assert!(session.merge_response_cookies(&headers));
        assert_eq!(session.cookie_header.expose_secret(), "route=node1");
    }
}
