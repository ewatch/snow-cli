use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Return the name of the currently running binary (`snow-cli` or `snow-cli-ro`)
/// for use in user-facing hints, so suggestions reference the command the user
/// actually invoked. Falls back to `snow-cli` when the name cannot be determined.
pub(crate) fn program_name() -> String {
    std::env::args_os()
        .next()
        .map(PathBuf::from)
        .as_deref()
        .and_then(|path| path.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|name| name.starts_with("snow-cli"))
        .unwrap_or_else(|| "snow-cli".to_string())
}

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

    /// OAuth grant type (for oauth2). Defaults to client_credentials.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_grant_type: Option<OAuthGrantType>,

    /// OAuth scopes requested during authorization-code login.
    /// ServiceNow commonly uses the `useraccount` scope for user-bound tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_scope: Option<String>,

    /// Host used in the OAuth authorization-code local redirect URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_redirect_host: Option<String>,

    /// Port used in the OAuth authorization-code local redirect URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_redirect_port: Option<u16>,

    /// Path used in the OAuth authorization-code local redirect URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_redirect_path: Option<String>,

    /// Path to client certificate (for mTLS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cert_path: Option<PathBuf>,

    /// Path to client key (for mTLS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_path: Option<PathBuf>,

    /// Optional browser entry point for browser-session login.
    /// Kept for backward compatibility but not required.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sso_login_url: Option<String>,
}

/// Supported authentication methods.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    Basic,
    Oauth2,
    ApiKey,
    Mtls,
    /// Browser session token — cookie is read from the `SNOW_SESSION_COOKIE` env var at
    /// runtime and is never persisted to the profile or keychain.
    #[serde(alias = "saml")]
    BrowserSession,
}

/// OAuth 2.0 grant type for token acquisition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OAuthGrantType {
    /// Machine-to-machine: client_id + client_secret only.
    ClientCredentials,
    /// User-context: client_id + client_secret + username + password.
    Password,
    /// Browser-based user authorization with a localhost redirect callback.
    AuthorizationCode,
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
        if let Ok(path) = std::env::var("SNOW_CLI_CONFIG")
            && let Some(parent) = PathBuf::from(path).parent()
        {
            return parent.to_path_buf();
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

    /// Resolve the effective active profile name.
    pub fn resolve_active_profile_name(
        &self,
        override_name: Option<&str>,
    ) -> anyhow::Result<String> {
        if let Some(name) = override_name {
            if self.profiles.contains_key(name) {
                return Ok(name.to_string());
            }
            anyhow::bail!("{}", self.profile_not_found_message(name));
        }

        if self.default_profile.is_empty() {
            anyhow::bail!(
                "No default profile configured. Run `{} profile add default --instance <url> --auth-method <method>` first.",
                program_name()
            );
        }

        if self.profiles.is_empty() {
            anyhow::bail!(
                "No profiles are configured yet. Run `{} profile add default --instance <url> --auth-method <method>` first.",
                program_name()
            );
        }

        if !self.profiles.contains_key(&self.default_profile) {
            let available_profiles = self.available_profiles_for_message();
            anyhow::bail!(
                "Default profile '{}' not found. Available profiles: {}. \
                 Run `{} profile default <name>` to choose one.",
                self.default_profile,
                available_profiles,
                program_name()
            );
        }

        Ok(self.default_profile.clone())
    }

    /// Build a user-facing profile-not-found message with suggestions.
    pub fn profile_not_found_message(&self, requested: &str) -> String {
        if self.profiles.is_empty() {
            return format!(
                "Profile '{}' not found. No profiles are configured yet. \
                 Run `{} profile add default --instance <url> --auth-method <method>` first.",
                requested,
                program_name()
            );
        }

        if let Some(suggested) = self.suggest_profile_name(requested) {
            return format!(
                "Profile '{}' not found. Maybe you meant '{}'. \
                 Run `{} profile list` to see available profiles.",
                requested,
                suggested,
                program_name()
            );
        }

        format!(
            "Profile '{}' not found. Available profiles: {}. \
             Run `{} profile list` to see details.",
            requested,
            self.available_profiles_for_message(),
            program_name()
        )
    }

    fn available_profiles_for_message(&self) -> String {
        let mut names: Vec<&str> = self.profiles.keys().map(String::as_str).collect();
        names.sort_unstable();

        if names.is_empty() {
            return "(none)".to_string();
        }

        if names.len() <= 5 {
            return names.join(", ");
        }

        format!("{}, ...", names[..5].join(", "))
    }

    fn suggest_profile_name(&self, requested: &str) -> Option<&str> {
        let requested_lower = requested.to_ascii_lowercase();
        let mut best: Option<(&str, usize)> = None;

        for candidate in self.profiles.keys().map(String::as_str) {
            let candidate_lower = candidate.to_ascii_lowercase();
            let score = profile_similarity_score(&requested_lower, &candidate_lower);
            if score == 0 {
                continue;
            }

            match best {
                Some((_, best_score)) if score <= best_score => {}
                _ => best = Some((candidate, score)),
            }
        }

        best.map(|(name, _)| name)
    }
}

fn profile_similarity_score(requested: &str, candidate: &str) -> usize {
    if requested == candidate {
        return 1000;
    }
    if candidate.starts_with(requested) || requested.starts_with(candidate) {
        return 800;
    }
    if candidate.contains(requested) || requested.contains(candidate) {
        return 600;
    }

    let shared_prefix = requested
        .chars()
        .zip(candidate.chars())
        .take_while(|(a, b)| a == b)
        .count();
    if shared_prefix >= 2 {
        return shared_prefix;
    }

    if requested.chars().count() > 2 && candidate.chars().count() > 2 {
        let requested_without_last: String = requested
            .chars()
            .take(requested.chars().count() - 1)
            .collect();
        let candidate_without_last: String = candidate
            .chars()
            .take(candidate.chars().count() - 1)
            .collect();
        if requested_without_last == candidate_without_last {
            return 500;
        }
    }

    0
}

pub fn validate_instance_url(input: &str) -> anyhow::Result<String> {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        anyhow::bail!("Instance URL must not be empty.");
    }

    let url = reqwest::Url::parse(trimmed)
        .map_err(|error| anyhow::anyhow!("Invalid instance URL '{}': {}", input, error))?;

    if url.host_str().is_none() {
        anyhow::bail!("Invalid instance URL '{}': missing host.", input);
    }
    if !url.username().is_empty() || url.password().is_some() {
        anyhow::bail!(
            "Invalid instance URL '{}': embedded credentials are not allowed.",
            input
        );
    }
    if !url.path().is_empty() && url.path() != "/" {
        anyhow::bail!(
            "Invalid instance URL '{}': paths are not allowed; provide only the instance origin.",
            input
        );
    }
    if url.query().is_some() || url.fragment().is_some() {
        anyhow::bail!(
            "Invalid instance URL '{}': query strings and fragments are not allowed.",
            input
        );
    }

    match url.scheme() {
        "https" => Ok(trimmed.to_string()),
        "http" if url.host_str().map(is_loopback_host).unwrap_or(false) => Ok(trimmed.to_string()),
        "http" => anyhow::bail!(
            "Invalid instance URL '{}': HTTP is only allowed for loopback test instances.",
            input
        ),
        scheme => anyhow::bail!(
            "Invalid instance URL '{}': unsupported scheme '{}'; use HTTPS.",
            input,
            scheme
        ),
    }
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
        || host.starts_with("127.")
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
    fn test_validate_instance_url_accepts_https_and_loopback_http() {
        assert_eq!(
            validate_instance_url("https://dev.service-now.com/").unwrap(),
            "https://dev.service-now.com"
        );
        assert_eq!(
            validate_instance_url("http://127.0.0.1:8080").unwrap(),
            "http://127.0.0.1:8080"
        );
    }

    #[test]
    fn test_validate_instance_url_rejects_unsafe_forms() {
        assert!(validate_instance_url("http://dev.service-now.com").is_err());
        assert!(validate_instance_url("https://user:pass@dev.service-now.com").is_err());
        assert!(validate_instance_url("https://dev.service-now.com/nav_to.do").is_err());
        assert!(validate_instance_url("https://dev.service-now.com?x=1").is_err());
        assert!(validate_instance_url("file:///tmp/foo").is_err());
    }

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
                oauth_grant_type: None,
                oauth_scope: None,
                oauth_redirect_host: None,
                oauth_redirect_port: None,
                oauth_redirect_path: None,
                cert_path: None,
                key_path: None,
                sso_login_url: None,
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
                oauth_grant_type: None,
                oauth_scope: None,
                oauth_redirect_host: None,
                oauth_redirect_port: None,
                oauth_redirect_path: None,
                cert_path: None,
                key_path: None,
                sso_login_url: None,
            },
        );
        config.profiles.insert(
            "prod".to_string(),
            Profile {
                instance: "https://prod.service-now.com".to_string(),
                auth_method: AuthMethod::Oauth2,
                username: None,
                client_id: Some("client123".to_string()),
                oauth_grant_type: None,
                oauth_scope: None,
                oauth_redirect_host: None,
                oauth_redirect_port: None,
                oauth_redirect_path: None,
                cert_path: None,
                key_path: None,
                sso_login_url: None,
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

    #[test]
    fn test_resolve_active_profile_name_uses_default() {
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
                oauth_grant_type: None,
                oauth_scope: None,
                oauth_redirect_host: None,
                oauth_redirect_port: None,
                oauth_redirect_path: None,
                cert_path: None,
                key_path: None,
                sso_login_url: None,
            },
        );

        let resolved = config.resolve_active_profile_name(None).unwrap();
        assert_eq!(resolved, "dev");
    }

    #[test]
    fn test_resolve_active_profile_name_fails_for_unknown_override() {
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
                oauth_grant_type: None,
                oauth_scope: None,
                oauth_redirect_host: None,
                oauth_redirect_port: None,
                oauth_redirect_path: None,
                cert_path: None,
                key_path: None,
                sso_login_url: None,
            },
        );

        let err = config
            .resolve_active_profile_name(Some("prod"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("Profile 'prod' not found"));
    }

    #[test]
    fn test_profile_not_found_message_includes_suggestion() {
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
                oauth_grant_type: None,
                oauth_scope: None,
                oauth_redirect_host: None,
                oauth_redirect_port: None,
                oauth_redirect_path: None,
                cert_path: None,
                key_path: None,
                sso_login_url: None,
            },
        );

        let message = config.profile_not_found_message("de");
        assert!(message.contains("Maybe you meant 'dev'"));
    }

    #[test]
    fn test_profile_not_found_message_for_empty_config() {
        let config = AppConfig::default();
        let message = config.profile_not_found_message("dev");
        assert!(message.contains("No profiles are configured yet"));
        assert!(message.contains("profile add"));
    }
}
