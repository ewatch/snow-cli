use super::*;

#[derive(Debug, serde::Serialize)]
pub(super) struct ProfileMatch {
    name: String,
    instance: String,
    auth_method: AuthMethod,
    default: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct InstanceSelector {
    host: String,
    service_now_instance: Option<String>,
}

/// `profile init` — Create initial config with a default profile.
///
/// If flags are provided (--instance, --auth-method, etc.), runs non-interactively.
/// Otherwise, returns an error with guidance on required flags (no interactive
/// prompts since this CLI is designed for both humans and coding agents).
#[cfg(test)]
pub(super) async fn handle_init(
    config_path: &Path,
    instance: Option<String>,
    auth_method: Option<CliAuthMethod>,
    username: Option<String>,
    oauth_grant_type: Option<CliOAuthGrantType>,
    name: String,
) -> anyhow::Result<()> {
    handle_init_with_oauth_options(
        config_path,
        instance,
        auth_method,
        username,
        None,
        oauth_grant_type,
        None,
        None,
        None,
        None,
        name,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_init_with_oauth_options(
    config_path: &Path,
    instance: Option<String>,
    auth_method: Option<CliAuthMethod>,
    username: Option<String>,
    client_id: Option<String>,
    oauth_grant_type: Option<CliOAuthGrantType>,
    oauth_scope: Option<String>,
    oauth_redirect_host: Option<String>,
    oauth_redirect_port: Option<u16>,
    oauth_redirect_path: Option<String>,
    name: String,
) -> anyhow::Result<()> {
    if config_path.exists() {
        anyhow::bail!(
            "Configuration already exists at {}. Use `snow-cli profile add <name>` to create another profile or `snow-cli profile edit <name>` to modify one.",
            config_path.display()
        );
    }

    let instance = instance.ok_or_else(|| {
        anyhow::anyhow!(
            "Instance URL is required. Use: snow-cli profile add default --instance https://mycompany.service-now.com --auth-method basic --username admin"
        )
    })?;
    let instance = validate_instance_url(&instance)?;
    if let Some(host) = oauth_redirect_host.as_deref() {
        validate_oauth_redirect_host(host)?;
    }

    let auth = auth_method
        .map(|a| to_auth_method(&a))
        .unwrap_or(AuthMethod::Basic);

    let profile = Profile {
        instance: instance.clone(),
        auth_method: auth,
        username,
        client_id,
        oauth_grant_type: oauth_grant_type.map(|g| to_oauth_grant_type(&g)),
        oauth_scope,
        oauth_redirect_host,
        oauth_redirect_port,
        oauth_redirect_path,
        cert_path: None,
        key_path: None,
        sso_login_url: None,
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

/// `profile set <name>` — Create or update a named profile (legacy upsert).
#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(super) async fn handle_set_profile(
    config_path: &Path,
    name: String,
    instance: Option<String>,
    auth_method: Option<CliAuthMethod>,
    username: Option<String>,
    client_id: Option<String>,
    oauth_grant_type: Option<CliOAuthGrantType>,
    cert_path: Option<String>,
    key_path: Option<String>,
    sso_login_url: Option<String>,
) -> anyhow::Result<()> {
    handle_set_profile_with_oauth_options(
        config_path,
        name,
        instance,
        auth_method,
        username,
        client_id,
        oauth_grant_type,
        None,
        None,
        None,
        None,
        cert_path,
        key_path,
        sso_login_url,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_add_profile_with_oauth_options(
    config_path: &Path,
    name: String,
    instance: Option<String>,
    auth_method: Option<CliAuthMethod>,
    username: Option<String>,
    client_id: Option<String>,
    oauth_grant_type: Option<CliOAuthGrantType>,
    oauth_scope: Option<String>,
    oauth_redirect_host: Option<String>,
    oauth_redirect_port: Option<u16>,
    oauth_redirect_path: Option<String>,
    cert_path: Option<String>,
    key_path: Option<String>,
    sso_login_url: Option<String>,
) -> anyhow::Result<()> {
    let config = AppConfig::load_from(config_path)?;
    if config.profiles.contains_key(&name) {
        anyhow::bail!(
            "Profile '{}' already exists. Use `snow-cli profile edit {}` to update it.",
            name,
            name
        );
    }

    handle_set_profile_with_oauth_options(
        config_path,
        name,
        instance,
        auth_method,
        username,
        client_id,
        oauth_grant_type,
        oauth_scope,
        oauth_redirect_host,
        oauth_redirect_port,
        oauth_redirect_path,
        cert_path,
        key_path,
        sso_login_url,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_edit_profile_with_oauth_options(
    config_path: &Path,
    name: String,
    instance: Option<String>,
    auth_method: Option<CliAuthMethod>,
    username: Option<String>,
    client_id: Option<String>,
    oauth_grant_type: Option<CliOAuthGrantType>,
    oauth_scope: Option<String>,
    oauth_redirect_host: Option<String>,
    oauth_redirect_port: Option<u16>,
    oauth_redirect_path: Option<String>,
    cert_path: Option<String>,
    key_path: Option<String>,
    sso_login_url: Option<String>,
) -> anyhow::Result<()> {
    let config = AppConfig::load_from(config_path)?;
    if !config.profiles.contains_key(&name) {
        anyhow::bail!(
            "{}. Use `snow-cli profile add {} --instance <url>` to create it.",
            config.profile_not_found_message(&name),
            name
        );
    }

    handle_set_profile_with_oauth_options(
        config_path,
        name,
        instance,
        auth_method,
        username,
        client_id,
        oauth_grant_type,
        oauth_scope,
        oauth_redirect_host,
        oauth_redirect_port,
        oauth_redirect_path,
        cert_path,
        key_path,
        sso_login_url,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_set_profile_with_oauth_options(
    config_path: &Path,
    name: String,
    instance: Option<String>,
    auth_method: Option<CliAuthMethod>,
    username: Option<String>,
    client_id: Option<String>,
    oauth_grant_type: Option<CliOAuthGrantType>,
    oauth_scope: Option<String>,
    oauth_redirect_host: Option<String>,
    oauth_redirect_port: Option<u16>,
    oauth_redirect_path: Option<String>,
    cert_path: Option<String>,
    key_path: Option<String>,
    sso_login_url: Option<String>,
) -> anyhow::Result<()> {
    let mut config = AppConfig::load_from(config_path)?;
    let instance = instance
        .map(|value| validate_instance_url(&value))
        .transpose()?;
    if let Some(host) = oauth_redirect_host.as_deref() {
        validate_oauth_redirect_host(host)?;
    }

    let profile = if let Some(existing) = config.profiles.get(&name) {
        // Update existing profile — merge provided fields
        Profile {
            instance: instance.unwrap_or_else(|| existing.instance.clone()),
            auth_method: auth_method
                .map(|a| to_auth_method(&a))
                .unwrap_or_else(|| existing.auth_method.clone()),
            username: username.or_else(|| existing.username.clone()),
            client_id: client_id.or_else(|| existing.client_id.clone()),
            oauth_grant_type: oauth_grant_type
                .as_ref()
                .map(to_oauth_grant_type)
                .or_else(|| existing.oauth_grant_type.clone()),
            oauth_scope: oauth_scope.or_else(|| existing.oauth_scope.clone()),
            oauth_redirect_host: oauth_redirect_host
                .or_else(|| existing.oauth_redirect_host.clone()),
            oauth_redirect_port: oauth_redirect_port.or(existing.oauth_redirect_port),
            oauth_redirect_path: oauth_redirect_path
                .or_else(|| existing.oauth_redirect_path.clone()),
            cert_path: cert_path
                .map(PathBuf::from)
                .or_else(|| existing.cert_path.clone()),
            key_path: key_path
                .map(PathBuf::from)
                .or_else(|| existing.key_path.clone()),
            sso_login_url: sso_login_url.or_else(|| existing.sso_login_url.clone()),
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
            oauth_grant_type: oauth_grant_type.map(|g| to_oauth_grant_type(&g)),
            oauth_scope,
            oauth_redirect_host,
            oauth_redirect_port,
            oauth_redirect_path,
            cert_path: cert_path.map(PathBuf::from),
            key_path: key_path.map(PathBuf::from),
            sso_login_url,
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

/// `profile list` — List all configured profiles.
pub(super) async fn handle_list_profiles(
    config_path: &Path,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let config = AppConfig::load_from(config_path)?;

    if config.profiles.is_empty() {
        anyhow::bail!(
            "No profiles configured. Run `snow-cli profile add default --instance <url>` to get started."
        );
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
        OutputFormat::Jsonl | OutputFormat::Toon | OutputFormat::Auto => {
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
            output::print_output(&profiles, output_format)?;
        }
        OutputFormat::Text => {
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
            println!("{}", serde_json::to_string_pretty(&profiles)?);
        }
    }

    Ok(())
}

pub(super) async fn handle_find_profile(
    config_path: &Path,
    output_format: &OutputFormat,
    instance: String,
) -> anyhow::Result<()> {
    let config = AppConfig::load_from(config_path)?;

    if config.profiles.is_empty() {
        anyhow::bail!(
            "No profiles configured. Run `snow-cli profile add default --instance <url>` to get started."
        );
    }

    let selector = normalize_instance_selector(&instance)?;
    let mut matches: Vec<ProfileMatch> = config
        .profiles
        .iter()
        .filter(|(_, profile)| profile_matches_instance(&profile.instance, &selector))
        .map(|(name, profile)| ProfileMatch {
            name: name.clone(),
            instance: profile.instance.clone(),
            auth_method: profile.auth_method.clone(),
            default: name == &config.default_profile,
        })
        .collect();
    matches.sort_by(|a, b| a.name.cmp(&b.name));

    if matches.is_empty() {
        anyhow::bail!(
            "No profile found for instance '{}'. Run `snow-cli profile list` to see configured profiles.",
            instance
        );
    }

    output::print_list(&matches, output_format)
}

pub(super) fn normalize_instance_selector(input: &str) -> anyhow::Result<InstanceSelector> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Instance must not be empty.");
    }

    let url_candidate = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };

    let url = url::Url::parse(&url_candidate).map_err(|_| {
        anyhow::anyhow!(
            "Invalid instance '{}'. Use an instance name, host, or URL.",
            input
        )
    })?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid instance '{}'. Missing host.", input))?
        .trim_end_matches('.')
        .to_ascii_lowercase();

    Ok(InstanceSelector {
        service_now_instance: service_now_instance_name(&host),
        host,
    })
}

pub(super) fn service_now_instance_name(host: &str) -> Option<String> {
    if !host.contains('.') {
        return Some(host.to_string());
    }

    host.strip_suffix(".service-now.com")
        .and_then(|without_suffix| without_suffix.split('.').next())
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
}

pub(super) fn profile_matches_instance(
    profile_instance: &str,
    selector: &InstanceSelector,
) -> bool {
    let Ok(profile_selector) = normalize_instance_selector(profile_instance) else {
        return false;
    };

    profile_selector.host == selector.host
        || match (
            profile_selector.service_now_instance.as_deref(),
            selector.service_now_instance.as_deref(),
        ) {
            (Some(profile_instance), Some(requested_instance)) => {
                profile_instance == requested_instance
            }
            _ => false,
        }
}

/// `profile default <name>` — Set the default profile.
pub(super) async fn handle_default_profile(config_path: &Path, name: String) -> anyhow::Result<()> {
    let mut config = AppConfig::load_from(config_path)?;

    if !config.profiles.contains_key(&name) {
        anyhow::bail!("{}", config.profile_not_found_message(&name));
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
/// `profile current` — Show the currently selected profile summary.
pub(super) async fn handle_current(
    config_path: &Path,
    active_profile: &str,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let config = AppConfig::load_from(config_path)?;
    let profile = config.active_profile(Some(active_profile));

    match output_format {
        OutputFormat::Json => {
            let output = serde_json::json!({
                "active_profile": active_profile,
                "default_profile": config.default_profile,
                "profile": profile.map(|p| serde_json::json!({
                    "name": active_profile,
                    "instance": p.instance,
                    "auth_method": p.auth_method,
                })),
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Csv => {
            let mut writer = csv::Writer::from_writer(std::io::stdout());
            writer.write_record(["key", "value"])?;
            writer.write_record(["active_profile", active_profile])?;
            writer.write_record(["default_profile", &config.default_profile])?;
            if let Some(p) = profile {
                writer.write_record(["instance", &p.instance])?;
                let auth_str = serde_json::to_string(&p.auth_method)?;
                writer.write_record(["auth_method", auth_str.trim_matches('"')])?;
            } else {
                writer.write_record(["profile_status", "not_found"])?;
            }
            writer.flush()?;
        }
        OutputFormat::Jsonl | OutputFormat::Toon | OutputFormat::Auto => {
            let output = serde_json::json!({
                "active_profile": active_profile,
                "default_profile": config.default_profile,
                "profile": profile.map(|p| serde_json::json!({
                    "name": active_profile,
                    "instance": p.instance,
                    "auth_method": p.auth_method,
                })),
            });
            output::print_output(&output, output_format)?;
        }
        OutputFormat::Text => {
            let output = serde_json::json!({
                "active_profile": active_profile,
                "default_profile": config.default_profile,
                "profile": profile.map(|p| serde_json::json!({
                    "name": active_profile,
                    "instance": p.instance,
                    "auth_method": p.auth_method,
                })),
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }

    Ok(())
}

/// `profile show` — Show the current active configuration.
pub(super) async fn handle_show(
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
        OutputFormat::Jsonl | OutputFormat::Toon | OutputFormat::Auto => {
            let output = serde_json::json!({
                "config_path": config_path.display().to_string(),
                "default_profile": config.default_profile,
                "active_profile": active_profile,
                "profile": profile,
                "total_profiles": config.profiles.len(),
            });
            output::print_output(&output, output_format)?;
        }
        OutputFormat::Text => {
            let output = serde_json::json!({
                "config_path": config_path.display().to_string(),
                "default_profile": config.default_profile,
                "active_profile": active_profile,
                "profile": profile,
                "total_profiles": config.profiles.len(),
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }

    Ok(())
}

/// `profile remove <name>` — Remove a named profile.
pub(super) async fn handle_delete_profile(
    config_path: &Path,
    name: String,
    yes: bool,
    new_default: Option<String>,
) -> anyhow::Result<()> {
    let mut config = AppConfig::load_from(config_path)?;

    if !config.profiles.contains_key(&name) {
        anyhow::bail!("{}", config.profile_not_found_message(&name));
    }

    let is_default = config.default_profile == name;
    if is_default && !yes {
        anyhow::bail!(
            "Profile '{}' is the current default. Re-run with --yes and --new-default <name>.",
            name
        );
    }

    if is_default && config.profiles.len() > 1 {
        let replacement = new_default.ok_or_else(|| {
            anyhow::anyhow!("Deleting the current default profile requires --new-default <name>.")
        })?;

        if replacement == name {
            anyhow::bail!("--new-default must be different from the profile being deleted.");
        }

        if !config.profiles.contains_key(&replacement) {
            anyhow::bail!("New default profile '{}' does not exist.", replacement);
        }

        config.default_profile = replacement;
    }

    config.profiles.remove(&name);

    if config.profiles.is_empty() {
        config.default_profile.clear();
    }

    config.save_to(config_path)?;

    // Best-effort credential cleanup for all supported types.
    for credential_type in credentials::ALL_CREDENTIAL_TYPES {
        let _ = credentials::delete_credential(&name, credential_type);
    }

    let result = serde_json::json!({
        "status": "deleted",
        "profile": name,
        "default_profile": config.default_profile,
    });
    println!("{}", serde_json::to_string(&result)?);

    Ok(())
}
