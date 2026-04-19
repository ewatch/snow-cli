use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::keychain;

pub const NOW_SDK_SERVICE_NAME: &str = "ServiceNow";
pub const NOW_SDK_ACCOUNT_NAME: &str = "now-sdk";

pub type AliasStore = BTreeMap<String, StoredAlias>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredAlias {
    #[serde(rename = "isDefault")]
    pub is_default: bool,
    pub alias: String,
    pub creds: NowSdkCredentials,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum NowSdkCredentials {
    #[serde(rename = "basic")]
    Basic {
        #[serde(rename = "instanceUrl")]
        instance_url: String,
        username: String,
        password: String,
    },
    #[serde(rename = "oauth")]
    OAuth {
        #[serde(rename = "instanceUrl")]
        instance_url: String,
        access_token: String,
        token_type: String,
        refresh_token: String,
        expires_at: i64,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProfileSummary {
    pub alias: String,
    pub instance: String,
    pub auth_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    pub is_default: bool,
    pub supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicProfile {
    pub alias: String,
    pub instance: String,
    pub username: String,
    pub password: String,
    pub is_default: bool,
}

impl StoredAlias {
    pub fn auth_type(&self) -> &'static str {
        match self.creds {
            NowSdkCredentials::Basic { .. } => "basic",
            NowSdkCredentials::OAuth { .. } => "oauth",
        }
    }

    pub fn summary(&self) -> ProfileSummary {
        match &self.creds {
            NowSdkCredentials::Basic {
                instance_url,
                username,
                ..
            } => ProfileSummary {
                alias: self.alias.clone(),
                instance: instance_url.clone(),
                auth_type: self.auth_type().to_string(),
                username: Some(username.clone()),
                is_default: self.is_default,
                supported: true,
            },
            NowSdkCredentials::OAuth { instance_url, .. } => ProfileSummary {
                alias: self.alias.clone(),
                instance: instance_url.clone(),
                auth_type: self.auth_type().to_string(),
                username: None,
                is_default: self.is_default,
                supported: false,
            },
        }
    }

    pub fn as_basic_profile(&self) -> Option<BasicProfile> {
        match &self.creds {
            NowSdkCredentials::Basic {
                instance_url,
                username,
                password,
            } => Some(BasicProfile {
                alias: self.alias.clone(),
                instance: instance_url.clone(),
                username: username.clone(),
                password: password.clone(),
                is_default: self.is_default,
            }),
            NowSdkCredentials::OAuth { .. } => None,
        }
    }
}

pub fn load_store() -> anyhow::Result<AliasStore> {
    let Some(raw) = keychain::get_password(NOW_SDK_SERVICE_NAME, NOW_SDK_ACCOUNT_NAME)? else {
        return Ok(BTreeMap::new());
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(BTreeMap::new());
    }

    serde_json::from_str(trimmed).map_err(|error| {
        anyhow::anyhow!(
            "Failed to parse now-sdk keychain entry for service '{}' account '{}': {error}",
            NOW_SDK_SERVICE_NAME,
            NOW_SDK_ACCOUNT_NAME,
        )
    })
}

pub fn save_store(store: &AliasStore) -> anyhow::Result<()> {
    if store.is_empty() {
        return keychain::delete_password(NOW_SDK_SERVICE_NAME, NOW_SDK_ACCOUNT_NAME);
    }

    let raw = serde_json::to_string(store)?;
    keychain::set_password(NOW_SDK_SERVICE_NAME, NOW_SDK_ACCOUNT_NAME, &raw)
}

pub fn snapshot_raw_store() -> anyhow::Result<Option<String>> {
    keychain::get_password(NOW_SDK_SERVICE_NAME, NOW_SDK_ACCOUNT_NAME)
}

pub fn restore_raw_store(snapshot: Option<&str>) -> anyhow::Result<()> {
    match snapshot {
        Some(raw) => keychain::set_password(NOW_SDK_SERVICE_NAME, NOW_SDK_ACCOUNT_NAME, raw),
        None => keychain::delete_password(NOW_SDK_SERVICE_NAME, NOW_SDK_ACCOUNT_NAME),
    }
}

pub fn list_profiles() -> anyhow::Result<Vec<ProfileSummary>> {
    let store = load_store()?;
    let mut profiles: Vec<_> = store.values().map(StoredAlias::summary).collect();
    profiles.sort_by(|left, right| left.alias.cmp(&right.alias));
    Ok(profiles)
}

pub fn upsert_basic_alias(
    store: &mut AliasStore,
    alias: &str,
    instance: &str,
    username: &str,
    password: &str,
    set_default: bool,
) {
    let existing_is_default = store
        .get(alias)
        .map(|entry| entry.is_default)
        .unwrap_or(false);

    let entry = StoredAlias {
        is_default: if set_default {
            true
        } else {
            existing_is_default
        },
        alias: alias.to_string(),
        creds: NowSdkCredentials::Basic {
            instance_url: instance.to_string(),
            username: username.to_string(),
            password: password.to_string(),
        },
    };
    store.insert(alias.to_string(), entry);

    if set_default {
        for (name, stored) in store.iter_mut() {
            stored.is_default = name == alias;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_alias_summary_marks_supported() {
        let stored = StoredAlias {
            is_default: true,
            alias: "dev".to_string(),
            creds: NowSdkCredentials::Basic {
                instance_url: "https://dev.service-now.com".to_string(),
                username: "admin".to_string(),
                password: "secret".to_string(),
            },
        };

        let summary = stored.summary();
        assert_eq!(summary.alias, "dev");
        assert_eq!(summary.instance, "https://dev.service-now.com");
        assert_eq!(summary.auth_type, "basic");
        assert_eq!(summary.username.as_deref(), Some("admin"));
        assert!(summary.is_default);
        assert!(summary.supported);
    }

    #[test]
    fn test_oauth_alias_summary_marks_unsupported() {
        let stored = StoredAlias {
            is_default: false,
            alias: "prod".to_string(),
            creds: NowSdkCredentials::OAuth {
                instance_url: "https://prod.service-now.com".to_string(),
                access_token: "token".to_string(),
                token_type: "Bearer".to_string(),
                refresh_token: "refresh".to_string(),
                expires_at: 12345,
            },
        };

        let summary = stored.summary();
        assert_eq!(summary.auth_type, "oauth");
        assert!(!summary.supported);
        assert_eq!(summary.username, None);
    }

    #[test]
    fn test_upsert_basic_alias_preserves_default_without_flag() {
        let mut store = BTreeMap::from([(
            "default".to_string(),
            StoredAlias {
                is_default: true,
                alias: "default".to_string(),
                creds: NowSdkCredentials::Basic {
                    instance_url: "https://default.service-now.com".to_string(),
                    username: "admin".to_string(),
                    password: "secret".to_string(),
                },
            },
        )]);

        upsert_basic_alias(
            &mut store,
            "dev",
            "https://dev.service-now.com",
            "devuser",
            "password",
            false,
        );

        assert!(store.get("default").unwrap().is_default);
        assert!(!store.get("dev").unwrap().is_default);
    }

    #[test]
    fn test_upsert_basic_alias_sets_default_when_requested() {
        let mut store = BTreeMap::from([(
            "default".to_string(),
            StoredAlias {
                is_default: true,
                alias: "default".to_string(),
                creds: NowSdkCredentials::Basic {
                    instance_url: "https://default.service-now.com".to_string(),
                    username: "admin".to_string(),
                    password: "secret".to_string(),
                },
            },
        )]);

        upsert_basic_alias(
            &mut store,
            "dev",
            "https://dev.service-now.com",
            "devuser",
            "password",
            true,
        );

        assert!(!store.get("default").unwrap().is_default);
        assert!(store.get("dev").unwrap().is_default);
    }
}
