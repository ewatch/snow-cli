use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::keychain;
use crate::models::secret::Secret;

mod credential_file;

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
        password: Secret<String>,
    },
    #[serde(rename = "oauth")]
    OAuth {
        #[serde(rename = "instanceUrl")]
        instance_url: String,
        access_token: Secret<String>,
        token_type: String,
        refresh_token: Secret<String>,
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
    pub password: Secret<String>,
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

/// Where now-sdk keeps its alias store on this machine.
enum Backend {
    /// The pre-4.14 JSON blob in the keychain (`ServiceNow` / `now-sdk`).
    /// Used while no encrypted store exists; now-sdk 4.14+ migrates it.
    Legacy,
    /// now-sdk 4.14+ `credentials.enc` with its key.
    Encrypted {
        key: credential_file::FileKey,
        path: std::path::PathBuf,
    },
    /// `SN_SDK_KEY_SOURCE=env` without a key and without a file: now-sdk
    /// reads this as empty and refuses to write.
    EnvKeyMissing,
}

/// Detect the store now-sdk would use, following its own rules: a key
/// selects the encrypted file; a file without its key is never touched.
fn backend() -> anyhow::Result<Backend> {
    let source = credential_file::KeySource::from_env()?;
    let key = source.key()?;
    let path = credential_file::credentials_path();
    match (key, path) {
        (Some(_), None) => Err(crate::config::error::ConfigError::new(
            "NOW_SDK_CREDENTIALS_UNREADABLE",
            "now-sdk encrypted credentials need a home directory or SN_SDK_CREDENTIALS_DIR; \
             neither is set.",
        )
        .into()),
        (Some(key), Some(path)) => Ok(Backend::Encrypted { key, path }),
        (None, Some(path)) if path.exists() => Err(crate::config::error::ConfigError::new(
            "NOW_SDK_CREDENTIALS_UNREADABLE",
            format!(
                "now-sdk encrypted credentials exist at {} but their key is unavailable ({}). \
                 snow-cli did not modify them; unlock the keychain or set the key, then retry.",
                path.display(),
                match source {
                    credential_file::KeySource::Keyring => "keychain entry missing or locked",
                    credential_file::KeySource::Env => "SN_SDK_ENCRYPTION_KEY is not set",
                }
            ),
        )
        .into()),
        (None, _) if source == credential_file::KeySource::Env => Ok(Backend::EnvKeyMissing),
        (None, _) => Ok(Backend::Legacy),
    }
}

fn parse_store(raw: &str, origin: &str) -> anyhow::Result<AliasStore> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(BTreeMap::new());
    }
    serde_json::from_str(trimmed).map_err(|error| {
        anyhow::anyhow!("Failed to parse now-sdk credentials from {origin}: {error}")
    })
}

pub fn load_store() -> anyhow::Result<AliasStore> {
    match backend()? {
        Backend::Legacy => {
            match keychain::get_password(NOW_SDK_SERVICE_NAME, NOW_SDK_ACCOUNT_NAME)? {
                Some(raw) => parse_store(
                    &raw,
                    &format!(
                        "keychain service '{NOW_SDK_SERVICE_NAME}' account '{NOW_SDK_ACCOUNT_NAME}'"
                    ),
                ),
                None => Ok(BTreeMap::new()),
            }
        }
        Backend::Encrypted { key, path } => match credential_file::read_raw(&path)? {
            Some(file) => parse_store(
                &credential_file::decrypt(&key, &file, &path)?,
                &path.display().to_string(),
            ),
            None => Ok(BTreeMap::new()),
        },
        Backend::EnvKeyMissing => Ok(BTreeMap::new()),
    }
}

pub fn save_store(store: &AliasStore) -> anyhow::Result<()> {
    match backend()? {
        Backend::Legacy => {
            if store.is_empty() {
                return keychain::delete_password(NOW_SDK_SERVICE_NAME, NOW_SDK_ACCOUNT_NAME);
            }
            let raw = serde_json::to_string(store)?;
            keychain::set_password(NOW_SDK_SERVICE_NAME, NOW_SDK_ACCOUNT_NAME, &raw)
        }
        Backend::Encrypted { key, path } => {
            let file = credential_file::encrypt(&key, &serde_json::to_string(store)?)?;
            credential_file::write_atomic(&path, &file)
        }
        Backend::EnvKeyMissing => anyhow::bail!(
            "SN_SDK_KEY_SOURCE=env but SN_SDK_ENCRYPTION_KEY is not set; set it to the base64 key \
             now-sdk uses before writing now-sdk credentials."
        ),
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
            password: Secret::new(password.to_string()),
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
                password: Secret::new("secret".to_string()),
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
                access_token: Secret::new("token".to_string()),
                token_type: "Bearer".to_string(),
                refresh_token: Secret::new("refresh".to_string()),
                expires_at: 12345,
            },
        };

        let summary = stored.summary();
        assert_eq!(summary.auth_type, "oauth");
        assert!(!summary.supported);
        assert_eq!(summary.username, None);
    }

    #[test]
    fn debug_does_not_leak_credentials() {
        let basic = NowSdkCredentials::Basic {
            instance_url: "https://dev.service-now.com".to_string(),
            username: "admin".to_string(),
            password: Secret::new("hunter2-password".to_string()),
        };
        let oauth = NowSdkCredentials::OAuth {
            instance_url: "https://prod.service-now.com".to_string(),
            access_token: Secret::new("access-token-value".to_string()),
            token_type: "Bearer".to_string(),
            refresh_token: Secret::new("refresh-token-value".to_string()),
            expires_at: 12345,
        };

        let basic_debug = format!("{basic:?}");
        assert!(!basic_debug.contains("hunter2-password"));
        assert!(basic_debug.contains("admin")); // non-secret fields still shown

        let oauth_debug = format!("{oauth:?}");
        assert!(!oauth_debug.contains("access-token-value"));
        assert!(!oauth_debug.contains("refresh-token-value"));
    }

    #[test]
    fn credentials_still_serialize_transparently() {
        // Persisted keychain payloads must be unchanged by the Secret wrapper.
        let basic = NowSdkCredentials::Basic {
            instance_url: "https://dev.service-now.com".to_string(),
            username: "admin".to_string(),
            password: Secret::new("plaintext".to_string()),
        };
        let json = serde_json::to_string(&basic).unwrap();
        assert!(json.contains(r#""password":"plaintext""#));

        let parsed: NowSdkCredentials = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, basic);
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
                    password: Secret::new("secret".to_string()),
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
                    password: Secret::new("secret".to_string()),
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
