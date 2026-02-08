use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Top-level application configuration, loaded from ~/.servicenow/config.toml.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    /// Name of the default profile to use.
    pub default_profile: String,

    /// Map of profile name to profile configuration.
    #[serde(default)]
    pub profiles: HashMap<String, Profile>,
}

/// A single ServiceNow instance profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Instance base URL (e.g., https://mycompany.service-now.com).
    pub instance: String,

    /// Authentication method for this profile.
    pub auth_method: AuthMethod,

    /// Username (for basic auth).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// OAuth client ID (for oauth2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Path to client certificate (for mTLS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cert_path: Option<PathBuf>,

    /// Path to client key (for mTLS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_path: Option<PathBuf>,
}

/// Supported authentication methods.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    Basic,
    Oauth2,
    ApiKey,
    Mtls,
    Saml,
}

impl AppConfig {
    /// Returns the path to the config file.
    ///
    /// Respects the `SNOW_CLI_CONFIG` environment variable if set,
    /// otherwise defaults to `~/.servicenow/config.toml`.
    pub fn config_path() -> PathBuf {
        if let Ok(path) = std::env::var("SNOW_CLI_CONFIG") {
            return PathBuf::from(path);
        }
        dirs_config_path().join("config.toml")
    }

    /// Returns the path to the config directory.
    pub fn config_dir() -> PathBuf {
        if let Ok(path) = std::env::var("SNOW_CLI_CONFIG") {
            if let Some(parent) = PathBuf::from(path).parent() {
                return parent.to_path_buf();
            }
        }
        dirs_config_path()
    }

    /// Load config from the default path. Returns default config if file does not exist.
    pub fn load() -> anyhow::Result<Self> {
        Self::load_from(&Self::config_path())
    }

    /// Load config from a specific path. Returns default config if file does not exist.
    pub fn load_from(path: &std::path::Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Save config to the default path.
    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to(&Self::config_path())
    }

    /// Save config to a specific path.
    pub fn save_to(&self, path: &std::path::Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get a profile by name, falling back to the default profile.
    pub fn get_profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.get(name)
    }

    /// Get the active profile (default or specified).
    pub fn active_profile(&self, override_name: Option<&str>) -> Option<&Profile> {
        let name = override_name.unwrap_or(&self.default_profile);
        self.get_profile(name)
    }
}

fn dirs_config_path() -> PathBuf {
    if let Some(home) = home_dir() {
        home.join(".servicenow")
    } else {
        PathBuf::from(".servicenow")
    }
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE").ok().map(PathBuf::from)
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert!(config.profiles.is_empty());
        assert_eq!(config.default_profile, "");
    }

    #[test]
    fn test_toml_round_trip() {
        let mut config = AppConfig {
            default_profile: "dev".to_string(),
            profiles: HashMap::new(),
        };
        config.profiles.insert(
            "dev".to_string(),
            Profile {
                instance: "https://dev.service-now.com".to_string(),
                auth_method: AuthMethod::Basic,
                username: Some("admin".to_string()),
                client_id: None,
                cert_path: None,
                key_path: None,
            },
        );

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let loaded: AppConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(loaded.default_profile, "dev");
        let profile = loaded.profiles.get("dev").unwrap();
        assert_eq!(profile.instance, "https://dev.service-now.com");
        assert_eq!(profile.auth_method, AuthMethod::Basic);
        assert_eq!(profile.username, Some("admin".to_string()));
    }

    #[test]
    fn test_active_profile_with_override() {
        let mut config = AppConfig {
            default_profile: "dev".to_string(),
            profiles: HashMap::new(),
        };
        config.profiles.insert(
            "dev".to_string(),
            Profile {
                instance: "https://dev.service-now.com".to_string(),
                auth_method: AuthMethod::Basic,
                username: None,
                client_id: None,
                cert_path: None,
                key_path: None,
            },
        );
        config.profiles.insert(
            "prod".to_string(),
            Profile {
                instance: "https://prod.service-now.com".to_string(),
                auth_method: AuthMethod::Oauth2,
                username: None,
                client_id: Some("client123".to_string()),
                cert_path: None,
                key_path: None,
            },
        );

        // Default profile
        let profile = config.active_profile(None).unwrap();
        assert_eq!(profile.instance, "https://dev.service-now.com");

        // Override profile
        let profile = config.active_profile(Some("prod")).unwrap();
        assert_eq!(profile.instance, "https://prod.service-now.com");

        // Missing profile
        assert!(config.active_profile(Some("nonexistent")).is_none());
    }

    #[test]
    fn test_parse_auth_methods() {
        let toml_str = r#"
            default_profile = "test"

            [profiles.test]
            instance = "https://test.service-now.com"
            auth_method = "oauth2"
            client_id = "abc"
        "#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        let profile = config.profiles.get("test").unwrap();
        assert_eq!(profile.auth_method, AuthMethod::Oauth2);
        assert_eq!(profile.client_id, Some("abc".to_string()));
    }
}
