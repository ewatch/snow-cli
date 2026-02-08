use std::path::{Path, PathBuf};

use crate::cli::args::{CliAuthMethod, ConfigArgs, ConfigCommands, OutputFormat};
use crate::config::profile::{AppConfig, AuthMethod, Profile};

/// Convert CLI auth method enum to config auth method enum.
fn to_auth_method(cli: &CliAuthMethod) -> AuthMethod {
    match cli {
        CliAuthMethod::Basic => AuthMethod::Basic,
        CliAuthMethod::Oauth2 => AuthMethod::Oauth2,
        CliAuthMethod::ApiKey => AuthMethod::ApiKey,
        CliAuthMethod::Mtls => AuthMethod::Mtls,
        CliAuthMethod::Saml => AuthMethod::Saml,
    }
}

pub async fn handle(
    args: ConfigArgs,
    active_profile: &str,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let config_path = AppConfig::config_path();
    match args.command {
        ConfigCommands::Init {
            instance,
            auth_method,
            username,
            name,
        } => handle_init(&config_path, instance, auth_method, username, name).await,
        ConfigCommands::SetProfile {
            name,
            instance,
            auth_method,
            username,
            client_id,
            cert_path,
            key_path,
        } => {
            handle_set_profile(
                &config_path,
                name,
                instance,
                auth_method,
                username,
                client_id,
                cert_path,
                key_path,
            )
            .await
        }
        ConfigCommands::ListProfiles => handle_list_profiles(&config_path, output_format).await,
        ConfigCommands::UseProfile { name } => handle_use_profile(&config_path, name).await,
        ConfigCommands::Show => handle_show(&config_path, active_profile, output_format).await,
    }
}

/// `config init` — Create initial config with a default profile.
///
/// If flags are provided (--instance, --auth-method, etc.), runs non-interactively.
/// Otherwise, returns an error with guidance on required flags (no interactive
/// prompts since this CLI is designed for both humans and coding agents).
async fn handle_init(
    config_path: &Path,
    instance: Option<String>,
    auth_method: Option<CliAuthMethod>,
    username: Option<String>,
    name: String,
) -> anyhow::Result<()> {
    if config_path.exists() {
        anyhow::bail!(
            "Configuration already exists at {}. Use `snow-cli config set-profile` to modify profiles.",
            config_path.display()
        );
    }

    let instance = instance.ok_or_else(|| {
        anyhow::anyhow!(
            "Instance URL is required for init. Use: snow-cli config init --instance https://mycompany.service-now.com --auth-method basic --username admin"
        )
    })?;

    let auth = auth_method
        .map(|a| to_auth_method(&a))
        .unwrap_or(AuthMethod::Basic);

    let profile = Profile {
        instance: instance.clone(),
        auth_method: auth,
        username,
        client_id: None,
        cert_path: None,
        key_path: None,
    };

    let mut config = AppConfig {
        default_profile: name.clone(),
        ..AppConfig::default()
    };
    config.profiles.insert(name.clone(), profile);
    config.save_to(config_path)?;

    tracing::info!("Created config at {}", config_path.display());

    let result = serde_json::json!({
        "status": "created",
        "config_path": config_path.display().to_string(),
        "profile": name,
        "instance": instance,
    });
    println!("{}", serde_json::to_string(&result)?);

    Ok(())
}

/// `config set-profile <name>` — Create or update a named profile.
#[allow(clippy::too_many_arguments)]
async fn handle_set_profile(
    config_path: &Path,
    name: String,
    instance: Option<String>,
    auth_method: Option<CliAuthMethod>,
    username: Option<String>,
    client_id: Option<String>,
    cert_path: Option<String>,
    key_path: Option<String>,
) -> anyhow::Result<()> {
    let mut config = AppConfig::load_from(config_path)?;

    let profile = if let Some(existing) = config.profiles.get(&name) {
        // Update existing profile — merge provided fields
        Profile {
            instance: instance.unwrap_or_else(|| existing.instance.clone()),
            auth_method: auth_method
                .map(|a| to_auth_method(&a))
                .unwrap_or_else(|| existing.auth_method.clone()),
            username: username.or_else(|| existing.username.clone()),
            client_id: client_id.or_else(|| existing.client_id.clone()),
            cert_path: cert_path
                .map(PathBuf::from)
                .or_else(|| existing.cert_path.clone()),
            key_path: key_path
                .map(PathBuf::from)
                .or_else(|| existing.key_path.clone()),
        }
    } else {
        // New profile — instance is required
        let instance = instance.ok_or_else(|| {
            anyhow::anyhow!(
                "Instance URL is required when creating a new profile. Use: --instance https://mycompany.service-now.com"
            )
        })?;

        Profile {
            instance,
            auth_method: auth_method
                .map(|a| to_auth_method(&a))
                .unwrap_or(AuthMethod::Basic),
            username,
            client_id,
            cert_path: cert_path.map(PathBuf::from),
            key_path: key_path.map(PathBuf::from),
        }
    };

    let is_update = config.profiles.contains_key(&name);
    config.profiles.insert(name.clone(), profile);

    // If this is the first profile and no default is set, make it the default
    if config.default_profile.is_empty() || config.profiles.len() == 1 {
        config.default_profile = name.clone();
    }

    config.save_to(config_path)?;

    let action = if is_update { "updated" } else { "created" };
    tracing::info!("Profile '{}' {}", name, action);

    let result = serde_json::json!({
        "status": action,
        "profile": name,
    });
    println!("{}", serde_json::to_string(&result)?);

    Ok(())
}

/// `config list-profiles` — List all configured profiles.
async fn handle_list_profiles(
    config_path: &Path,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let config = AppConfig::load_from(config_path)?;

    if config.profiles.is_empty() {
        anyhow::bail!("No profiles configured. Run `snow-cli config init` to get started.");
    }

    match output_format {
        OutputFormat::Json => {
            let profiles: Vec<serde_json::Value> = config
                .profiles
                .iter()
                .map(|(name, profile)| {
                    serde_json::json!({
                        "name": name,
                        "instance": profile.instance,
                        "auth_method": profile.auth_method,
                        "default": name == &config.default_profile,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string(&profiles)?);
        }
        OutputFormat::Csv => {
            let mut writer = csv::Writer::from_writer(std::io::stdout());
            writer.write_record(["name", "instance", "auth_method", "default"])?;
            for (name, profile) in &config.profiles {
                let auth_str = serde_json::to_string(&profile.auth_method)?;
                let auth_str = auth_str.trim_matches('"');
                let is_default = if name == &config.default_profile {
                    "true"
                } else {
                    "false"
                };
                writer.write_record([name.as_str(), &profile.instance, auth_str, is_default])?;
            }
            writer.flush()?;
        }
    }

    Ok(())
}

/// `config use-profile <name>` — Set the default profile.
async fn handle_use_profile(config_path: &Path, name: String) -> anyhow::Result<()> {
    let mut config = AppConfig::load_from(config_path)?;

    if !config.profiles.contains_key(&name) {
        let available: Vec<&String> = config.profiles.keys().collect();
        anyhow::bail!(
            "Profile '{}' not found. Available profiles: {:?}",
            name,
            available
        );
    }

    config.default_profile = name.clone();
    config.save_to(config_path)?;

    tracing::info!("Default profile set to '{}'", name);

    let result = serde_json::json!({
        "status": "updated",
        "default_profile": name,
    });
    println!("{}", serde_json::to_string(&result)?);

    Ok(())
}

/// `config show` — Show the current active configuration.
async fn handle_show(
    config_path: &Path,
    active_profile: &str,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let config = AppConfig::load_from(config_path)?;

    let profile = config.active_profile(Some(active_profile));

    match output_format {
        OutputFormat::Json => {
            let output = serde_json::json!({
                "config_path": config_path.display().to_string(),
                "default_profile": config.default_profile,
                "active_profile": active_profile,
                "profile": profile,
                "total_profiles": config.profiles.len(),
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Csv => {
            // CSV doesn't make sense for nested config, output key-value pairs
            let mut writer = csv::Writer::from_writer(std::io::stdout());
            writer.write_record(["key", "value"])?;
            writer.write_record(["config_path", &config_path.display().to_string()])?;
            writer.write_record(["default_profile", &config.default_profile])?;
            writer.write_record(["active_profile", active_profile])?;
            if let Some(p) = profile {
                writer.write_record(["instance", &p.instance])?;
                let auth_str = serde_json::to_string(&p.auth_method)?;
                writer.write_record(["auth_method", auth_str.trim_matches('"')])?;
                if let Some(ref u) = p.username {
                    writer.write_record(["username", u])?;
                }
            } else {
                writer.write_record(["profile_status", "not_found"])?;
            }
            writer.flush()?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper: create a temp dir and return the config file path within it.
    fn temp_config_path() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join(".servicenow").join("config.toml");
        (tmp, config_path)
    }

    #[tokio::test]
    async fn test_config_init_creates_config() {
        let (_tmp, config_path) = temp_config_path();
        let result = handle_init(
            &config_path,
            Some("https://test.service-now.com".to_string()),
            Some(CliAuthMethod::Basic),
            Some("admin".to_string()),
            "default".to_string(),
        )
        .await;
        assert!(result.is_ok());

        let config = AppConfig::load_from(&config_path).unwrap();
        assert_eq!(config.default_profile, "default");
        let profile = config.profiles.get("default").unwrap();
        assert_eq!(profile.instance, "https://test.service-now.com");
        assert_eq!(profile.auth_method, AuthMethod::Basic);
        assert_eq!(profile.username, Some("admin".to_string()));
    }

    #[tokio::test]
    async fn test_config_init_defaults_auth_to_basic() {
        let (_tmp, config_path) = temp_config_path();
        handle_init(
            &config_path,
            Some("https://test.service-now.com".to_string()),
            None,
            None,
            "default".to_string(),
        )
        .await
        .unwrap();

        let config = AppConfig::load_from(&config_path).unwrap();
        let profile = config.profiles.get("default").unwrap();
        assert_eq!(profile.auth_method, AuthMethod::Basic);
    }

    #[tokio::test]
    async fn test_config_init_fails_without_instance() {
        let (_tmp, config_path) = temp_config_path();
        let result = handle_init(&config_path, None, None, None, "default".to_string()).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Instance URL is required")
        );
    }

    #[tokio::test]
    async fn test_config_init_fails_if_exists() {
        let (_tmp, config_path) = temp_config_path();
        // Create config first
        handle_init(
            &config_path,
            Some("https://test.service-now.com".to_string()),
            None,
            None,
            "default".to_string(),
        )
        .await
        .unwrap();

        // Try init again — should fail
        let result = handle_init(
            &config_path,
            Some("https://test2.service-now.com".to_string()),
            None,
            None,
            "default".to_string(),
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn test_config_set_profile_creates_new() {
        let (_tmp, config_path) = temp_config_path();
        // Init first
        handle_init(
            &config_path,
            Some("https://dev.service-now.com".to_string()),
            None,
            None,
            "dev".to_string(),
        )
        .await
        .unwrap();

        // Create a second profile
        handle_set_profile(
            &config_path,
            "prod".to_string(),
            Some("https://prod.service-now.com".to_string()),
            Some(CliAuthMethod::Oauth2),
            None,
            Some("client123".to_string()),
            None,
            None,
        )
        .await
        .unwrap();

        let config = AppConfig::load_from(&config_path).unwrap();
        assert_eq!(config.profiles.len(), 2);
        let prod = config.profiles.get("prod").unwrap();
        assert_eq!(prod.instance, "https://prod.service-now.com");
        assert_eq!(prod.auth_method, AuthMethod::Oauth2);
        assert_eq!(prod.client_id, Some("client123".to_string()));
    }

    #[tokio::test]
    async fn test_config_set_profile_updates_existing() {
        let (_tmp, config_path) = temp_config_path();
        // Init
        handle_init(
            &config_path,
            Some("https://dev.service-now.com".to_string()),
            Some(CliAuthMethod::Basic),
            Some("admin".to_string()),
            "dev".to_string(),
        )
        .await
        .unwrap();

        // Update instance URL only — should preserve other fields
        handle_set_profile(
            &config_path,
            "dev".to_string(),
            Some("https://dev2.service-now.com".to_string()),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let config = AppConfig::load_from(&config_path).unwrap();
        let dev = config.profiles.get("dev").unwrap();
        assert_eq!(dev.instance, "https://dev2.service-now.com");
        // Auth method and username should be preserved
        assert_eq!(dev.auth_method, AuthMethod::Basic);
        assert_eq!(dev.username, Some("admin".to_string()));
    }

    #[tokio::test]
    async fn test_config_set_profile_requires_instance_for_new() {
        let (_tmp, config_path) = temp_config_path();
        // Init
        handle_init(
            &config_path,
            Some("https://dev.service-now.com".to_string()),
            None,
            None,
            "dev".to_string(),
        )
        .await
        .unwrap();

        // Try to create new profile without instance
        let result = handle_set_profile(
            &config_path,
            "prod".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Instance URL is required")
        );
    }

    #[tokio::test]
    async fn test_config_use_profile() {
        let (_tmp, config_path) = temp_config_path();
        // Init with dev
        handle_init(
            &config_path,
            Some("https://dev.service-now.com".to_string()),
            None,
            None,
            "dev".to_string(),
        )
        .await
        .unwrap();

        // Add prod
        handle_set_profile(
            &config_path,
            "prod".to_string(),
            Some("https://prod.service-now.com".to_string()),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        // Switch to prod
        handle_use_profile(&config_path, "prod".to_string())
            .await
            .unwrap();

        let config = AppConfig::load_from(&config_path).unwrap();
        assert_eq!(config.default_profile, "prod");
    }

    #[tokio::test]
    async fn test_config_use_profile_fails_for_nonexistent() {
        let (_tmp, config_path) = temp_config_path();
        handle_init(
            &config_path,
            Some("https://dev.service-now.com".to_string()),
            None,
            None,
            "dev".to_string(),
        )
        .await
        .unwrap();

        let result = handle_use_profile(&config_path, "nonexistent".to_string()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_config_show_json() {
        let (_tmp, config_path) = temp_config_path();
        handle_init(
            &config_path,
            Some("https://dev.service-now.com".to_string()),
            Some(CliAuthMethod::Basic),
            Some("admin".to_string()),
            "dev".to_string(),
        )
        .await
        .unwrap();

        // Should not error
        let result = handle_show(&config_path, "dev", &OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_config_show_csv() {
        let (_tmp, config_path) = temp_config_path();
        handle_init(
            &config_path,
            Some("https://dev.service-now.com".to_string()),
            Some(CliAuthMethod::Basic),
            Some("admin".to_string()),
            "dev".to_string(),
        )
        .await
        .unwrap();

        let result = handle_show(&config_path, "dev", &OutputFormat::Csv).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_config_list_profiles_empty() {
        let (_tmp, config_path) = temp_config_path();
        let result = handle_list_profiles(&config_path, &OutputFormat::Json).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No profiles"));
    }

    #[tokio::test]
    async fn test_config_list_profiles() {
        let (_tmp, config_path) = temp_config_path();
        handle_init(
            &config_path,
            Some("https://dev.service-now.com".to_string()),
            None,
            None,
            "dev".to_string(),
        )
        .await
        .unwrap();

        handle_set_profile(
            &config_path,
            "prod".to_string(),
            Some("https://prod.service-now.com".to_string()),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        // Should not error
        let result = handle_list_profiles(&config_path, &OutputFormat::Json).await;
        assert!(result.is_ok());
    }
}
