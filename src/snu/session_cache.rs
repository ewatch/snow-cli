use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::config::credentials;
use crate::snu::protocol::SnuInstance;

const CACHE_PROFILE: &str = "__snutils_bridge__";
const CACHE_CREDENTIAL_TYPE: &str = "browser_session";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CachedSnuSession {
    pub instance: SnuInstance,
    pub saved_at_unix_secs: u64,
}

impl CachedSnuSession {
    fn new(instance: &SnuInstance) -> anyhow::Result<Self> {
        let saved_at_unix_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before UNIX epoch")?
            .as_secs();

        Ok(Self {
            instance: instance.clone(),
            saved_at_unix_secs,
        })
    }
}

pub fn store_session(instance: &SnuInstance) -> anyhow::Result<()> {
    if !has_g_ck(instance) {
        return Ok(());
    }

    let cached = CachedSnuSession::new(instance)?;
    let value = serde_json::to_string(&cached)?;
    credentials::store_credential(CACHE_PROFILE, CACHE_CREDENTIAL_TYPE, &value)
        .context("failed to store SN-Utils browser session token in OS keychain")
}

pub fn load_session() -> anyhow::Result<Option<CachedSnuSession>> {
    let Some(raw) = credentials::get_credential(CACHE_PROFILE, CACHE_CREDENTIAL_TYPE)
        .context("failed to read SN-Utils browser session token from OS keychain")?
    else {
        return Ok(None);
    };

    let cached: CachedSnuSession = serde_json::from_str(&raw)
        .context("failed to parse cached SN-Utils browser session token")?;
    if has_g_ck(&cached.instance) {
        Ok(Some(cached))
    } else {
        Ok(None)
    }
}

pub fn load_session_for_url(instance_url: &str) -> anyhow::Result<Option<CachedSnuSession>> {
    let Some(cached) = load_session()? else {
        return Ok(None);
    };

    if same_origin_urls(&cached.instance.url, instance_url) {
        Ok(Some(cached))
    } else {
        Ok(None)
    }
}

pub fn delete_session() -> anyhow::Result<()> {
    credentials::delete_credential(CACHE_PROFILE, CACHE_CREDENTIAL_TYPE)
        .context("failed to delete cached SN-Utils browser session token")
}

fn has_g_ck(instance: &SnuInstance) -> bool {
    instance
        .g_ck
        .as_deref()
        .is_some_and(|token| !token.trim().is_empty())
}

fn same_origin_urls(left: &str, right: &str) -> bool {
    let Ok(left) = reqwest::Url::parse(left) else {
        return false;
    };
    let Ok(right) = reqwest::Url::parse(right) else {
        return false;
    };

    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(url: &str, g_ck: Option<&str>) -> SnuInstance {
        SnuInstance {
            name: "dev".into(),
            url: url.into(),
            g_ck: g_ck.map(str::to_string),
            scope: Some("global".into()),
        }
    }

    #[test]
    fn cache_payload_round_trips_without_losing_token() {
        let cached = CachedSnuSession {
            instance: instance("https://dev.service-now.com", Some("gck-secret")),
            saved_at_unix_secs: 123,
        };

        let raw = serde_json::to_string(&cached).unwrap();
        let parsed: CachedSnuSession = serde_json::from_str(&raw).unwrap();

        assert_eq!(parsed, cached);
    }

    #[test]
    fn same_origin_matches_default_ports() {
        assert!(same_origin_urls(
            "https://dev.service-now.com",
            "https://dev.service-now.com/"
        ));
        assert!(same_origin_urls(
            "https://dev.service-now.com:443",
            "https://dev.service-now.com/now/table/incident"
        ));
        assert!(!same_origin_urls(
            "https://dev.service-now.com",
            "https://other.service-now.com"
        ));
    }

    #[test]
    fn has_g_ck_rejects_missing_or_blank_tokens() {
        assert!(has_g_ck(&instance(
            "https://dev.service-now.com",
            Some("token")
        )));
        assert!(!has_g_ck(&instance(
            "https://dev.service-now.com",
            Some("  ")
        )));
        assert!(!has_g_ck(&instance("https://dev.service-now.com", None)));
    }
}
