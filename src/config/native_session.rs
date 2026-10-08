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
        for header in headers.get_all(http::header::SET_COOKIE) {
            let Ok(value) = header.to_str() else {
                continue;
            };
            let mut parts = value.split(';');
            let Some((name, value)) = parts.next().and_then(|part| part.trim().split_once('='))
            else {
                continue;
            };
            let delete = value.is_empty()
                || parts.any(|attribute| {
                    attribute
                        .trim()
                        .split_once('=')
                        .is_some_and(|(key, value)| {
                            key.eq_ignore_ascii_case("max-age")
                                && value.trim().parse::<i64>().is_ok_and(|age| age <= 0)
                        })
                });
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
