//! Instance-bound native session credentials. Only the OS keychain persists secrets.
use std::sync::{Mutex, OnceLock};

use super::credentials;
use crate::models::secret::Secret;
use http::HeaderMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CREDENTIAL_TYPE: &str = "native_session";

static NATIVE_SESSION_UPDATE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

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
        let request_url = url::Url::parse(&self.instance).ok();
        let before = self.cookie_header.expose_secret().to_string();
        let merged = merge_cookie_header_for_url(&before, headers, request_url.as_ref());
        if merged == before {
            return false;
        }
        self.cookie_header = Secret::new(merged);
        true
    }
}

fn domain_matches(request_host: &str, cookie_domain: &str) -> bool {
    let host = request_host.trim().to_ascii_lowercase();
    let domain = cookie_domain
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() || domain.is_empty() {
        return false;
    }
    if host == domain {
        return true;
    }
    // IP literals never subdomain-match.
    if host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[') {
        return false;
    }
    host.ends_with(&format!(".{domain}"))
}

fn path_matches(request_path: &str, cookie_path: &str) -> bool {
    if cookie_path == "/" {
        return true;
    }
    let trimmed = cookie_path.trim_end_matches('/');
    request_path == trimmed
        || request_path == cookie_path
        || request_path.starts_with(&format!("{trimmed}/"))
}

/// Merge Set-Cookie updates into a request Cookie header, honoring RFC 6265
/// case-sensitive names, trimmed name/value pairs, and expiry rules.
pub fn merge_cookie_header(cookie_header: &str, headers: &HeaderMap) -> String {
    merge_cookie_header_for_url(cookie_header, headers, None)
}

/// Merge Set-Cookie updates into a request Cookie header with optional
/// `Domain`, `Path`, and `Secure` attribute enforcement against `request_url`.
pub fn merge_cookie_header_for_url(
    cookie_header: &str,
    headers: &HeaderMap,
    request_url: Option<&url::Url>,
) -> String {
    let mut cookies = cookie_header
        .split(';')
        .filter_map(|cookie| cookie.trim().split_once('='))
        .filter_map(|(name, value)| {
            let name = name.trim();
            let value = value.trim();
            (!name.is_empty()).then(|| (name.to_string(), value.to_string()))
        })
        .collect::<Vec<_>>();
    let now = std::time::SystemTime::now();
    for header in headers.get_all(http::header::SET_COOKIE) {
        let Ok(raw) = header.to_str() else { continue };
        let mut parts = raw.split(';');
        let Some((name, value)) = parts.next().and_then(|part| part.trim().split_once('=')) else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() {
            continue;
        }
        let mut max_age_expired = None;
        let mut expires = None;
        let mut attr_domain: Option<String> = None;
        let mut attr_path: Option<String> = None;
        let mut attr_secure = false;

        for attribute in parts {
            let attribute = attribute.trim();
            if attribute.is_empty() {
                continue;
            }
            if let Some((key, val)) = attribute.split_once('=') {
                let key = key.trim();
                let val = val.trim();
                if key.eq_ignore_ascii_case("max-age") {
                    let digits = val.strip_prefix('-').unwrap_or(val);
                    if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
                        max_age_expired =
                            Some(val.starts_with('-') || digits.bytes().all(|byte| byte == b'0'));
                    }
                } else if key.eq_ignore_ascii_case("expires")
                    && let Ok(date) = httpdate::parse_http_date(val)
                {
                    expires = Some(date);
                } else if key.eq_ignore_ascii_case("domain") {
                    attr_domain = Some(val.to_string());
                } else if key.eq_ignore_ascii_case("path") && val.starts_with('/') {
                    attr_path = Some(val.to_string());
                }
            } else if attribute.eq_ignore_ascii_case("secure") {
                attr_secure = true;
            }
        }

        if let Some(url) = request_url {
            if attr_secure
                && url.scheme() != "https"
                && !crate::config::profile::is_loopback_url(url)
            {
                continue;
            }
            if let Some(domain) = attr_domain.as_deref() {
                let Some(host) = url.host_str() else {
                    continue;
                };
                if !domain_matches(host, domain) {
                    continue;
                }
            }
            if let Some(path) = attr_path.as_deref()
                && !path_matches(url.path(), path)
            {
                continue;
            }
        }

        // RFC 6265 gives a valid Max-Age precedence over Expires.
        let delete = value.is_empty()
            || max_age_expired.unwrap_or_else(|| expires.is_some_and(|date| date <= now));
        // RFC 6265 cookie names are case-sensitive.
        cookies.retain(|(key, _)| key != name);
        if !delete {
            cookies.push((name.to_string(), value.to_string()));
        }
    }
    cookies
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Serialize concurrent native-session updates (in-process and across CLI
/// processes) by locking, reloading the latest persisted session, applying
/// `mutator`, and saving only when modified.
pub fn update_locked<F>(
    profile: &str,
    fallback: &NativeSession,
    mutator: F,
) -> anyhow::Result<NativeSession>
where
    F: FnOnce(&mut NativeSession) -> bool,
{
    let _in_process_guard = NATIVE_SESSION_UPDATE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _file_lock = acquire_profile_lock(profile)?;

    let mut latest = match load(profile) {
        Ok(stored) => stored,
        Err(_) => fallback.clone(),
    };
    if mutator(&mut latest) {
        save(profile, &latest)?;
    }
    Ok(latest)
}

fn acquire_profile_lock(profile: &str) -> anyhow::Result<Option<std::fs::File>> {
    let config_path = crate::config::profile::AppConfig::config_path();
    let Some(dir) = config_path.parent() else {
        return Ok(None);
    };
    let lock_dir = dir.join(".locks");
    std::fs::create_dir_all(&lock_dir)?;
    let mut hasher = Sha256::new();
    hasher.update(profile.as_bytes());
    let digest = hasher.finalize();
    let hash: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    let lock_path = lock_dir.join(format!("native-session-{hash}.lock"));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    file.lock()?;
    Ok(Some(file))
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

    #[test]
    fn cookie_names_are_case_sensitive_trimmed_and_scoped_by_domain_path_secure() {
        let url = url::Url::parse("https://dev1.service-now.com/api/now/table/incident").unwrap();
        let mut headers = HeaderMap::new();
        // Trimmed name/value and distinct case-sensitive cookie name
        headers.append(
            http::header::SET_COOKIE,
            " jsessionid = lower ; Path=/".parse().unwrap(),
        );
        // Foreign domain must be ignored
        headers.append(
            http::header::SET_COOKIE,
            "JSESSIONID=attacker; Domain=evil.example; Path=/"
                .parse()
                .unwrap(),
        );
        // Unrelated path must be ignored
        headers.append(
            http::header::SET_COOKIE,
            "JSESSIONID=other-app; Path=/other-app".parse().unwrap(),
        );
        // Matching parent domain is accepted
        headers.append(
            http::header::SET_COOKIE,
            "glide_route=node2; Domain=.service-now.com; Path=/; Secure"
                .parse()
                .unwrap(),
        );

        let merged = merge_cookie_header_for_url("JSESSIONID=upper", &headers, Some(&url));
        assert_eq!(
            merged,
            "JSESSIONID=upper; jsessionid=lower; glide_route=node2"
        );

        // Secure cookie over non-loopback HTTP is rejected
        let plain_http = url::Url::parse("http://dev1.service-now.com/api").unwrap();
        let mut secure_headers = HeaderMap::new();
        secure_headers.insert(
            http::header::SET_COOKIE,
            "JSESSIONID=should-ignore; Secure; Path=/".parse().unwrap(),
        );
        assert_eq!(
            merge_cookie_header_for_url("", &secure_headers, Some(&plain_http)),
            ""
        );
    }
}
