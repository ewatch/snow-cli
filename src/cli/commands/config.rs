use std::path::{Path, PathBuf};

use crate::auth::oauth2::validate_oauth_redirect_host;
use crate::cli::args::{
    CliAuthMethod, CliOAuthGrantType, ConfigArgs, ConfigCommands, OutputFormat, ProfileSdkCommands,
};
use crate::cli::output;
use crate::config::credentials;
use crate::config::now_sdk;
use crate::config::profile::{
    AppConfig, AuthMethod, OAuthGrantType, Profile, validate_instance_url,
};

/// Convert CLI auth method enum to config auth method enum.
fn to_auth_method(cli: &CliAuthMethod) -> AuthMethod {
    match cli {
        CliAuthMethod::Basic => AuthMethod::Basic,
        CliAuthMethod::Oauth2 => AuthMethod::Oauth2,
        CliAuthMethod::ApiKey => AuthMethod::ApiKey,
        CliAuthMethod::Mtls => AuthMethod::Mtls,
        CliAuthMethod::BrowserSession => AuthMethod::BrowserSession,
    }
}

/// Convert CLI OAuth grant type to config OAuth grant type.
fn to_oauth_grant_type(cli: &CliOAuthGrantType) -> OAuthGrantType {
    match cli {
        CliOAuthGrantType::ClientCredentials => OAuthGrantType::ClientCredentials,
        CliOAuthGrantType::Password => OAuthGrantType::Password,
        CliOAuthGrantType::AuthorizationCode => OAuthGrantType::AuthorizationCode,
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
            client_id,
            oauth_grant_type,
            oauth_scope,
            oauth_redirect_host,
            oauth_redirect_port,
            oauth_redirect_path,
            name,
        } => {
            handle_init_with_oauth_options(
                &config_path,
                instance,
                auth_method,
                username,
                client_id,
                oauth_grant_type,
                oauth_scope,
                oauth_redirect_host,
                oauth_redirect_port,
                oauth_redirect_path,
                name,
            )
            .await
        }
        ConfigCommands::Add {
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
        } => {
            handle_add_profile_with_oauth_options(
                &config_path,
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
        ConfigCommands::Edit {
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
        } => {
            handle_edit_profile_with_oauth_options(
                &config_path,
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
        ConfigCommands::SetProfile {
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
        } => {
            handle_set_profile_with_oauth_options(
                &config_path,
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
        ConfigCommands::ListProfiles => handle_list_profiles(&config_path, output_format).await,
        ConfigCommands::FindProfile { instance } => {
            handle_find_profile(&config_path, output_format, instance).await
        }
        ConfigCommands::Sdk(sdk_args) => match sdk_args.command {
            ProfileSdkCommands::List => handle_list_now_sdk_profiles(output_format).await,
            ProfileSdkCommands::Import {
                alias,
                all,
                set_default,
            } => handle_import_now_sdk(&config_path, output_format, alias, all, set_default).await,
            ProfileSdkCommands::Export {
                profile,
                alias,
                set_default,
            } => {
                handle_export_now_sdk(&config_path, output_format, profile, alias, set_default)
                    .await
            }
        },
        ConfigCommands::ListNowSdkProfiles => handle_list_now_sdk_profiles(output_format).await,
        ConfigCommands::ImportNowSdk {
            alias,
            all,
            set_default,
        } => handle_import_now_sdk(&config_path, output_format, alias, all, set_default).await,
        ConfigCommands::ExportNowSdk {
            profile,
            alias,
            set_default,
        } => handle_export_now_sdk(&config_path, output_format, profile, alias, set_default).await,
        ConfigCommands::UseProfile { name } => handle_default_profile(&config_path, name).await,
        ConfigCommands::Current => {
            handle_current(&config_path, active_profile, output_format).await
        }
        ConfigCommands::Show => handle_show(&config_path, active_profile, output_format).await,
        ConfigCommands::DeleteProfile {
            name,
            yes,
            new_default,
        } => handle_delete_profile(&config_path, name, yes, new_default).await,
        ConfigCommands::Output { format, reset } => {
            handle_output_default(&config_path, format, reset).await
        }
    }
}

#[derive(Debug, Clone)]
struct ProfileCredentialSnapshot {
    config: Option<Profile>,
    password: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct ImportNowSdkResult {
    status: &'static str,
    imported_count: usize,
    imported_profiles: Vec<ImportedProfileSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_profile: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct ImportedProfileSummary {
    profile: String,
    source_alias: String,
    instance: String,
    auth_method: &'static str,
}

#[derive(Debug, serde::Serialize)]
struct ExportNowSdkResult {
    status: &'static str,
    profile: String,
    alias: String,
    instance: String,
    auth_method: &'static str,
    set_default: bool,
}

#[derive(Debug, serde::Serialize)]
struct ProfileMatch {
    name: String,
    instance: String,
    auth_method: AuthMethod,
    default: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct InstanceSelector {
    host: String,
    service_now_instance: Option<String>,
}

/// `profile init` — Create initial config with a default profile.
///
/// If flags are provided (--instance, --auth-method, etc.), runs non-interactively.
/// Otherwise, returns an error with guidance on required flags (no interactive
/// prompts since this CLI is designed for both humans and coding agents).
#[cfg(test)]
async fn handle_init(
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
async fn handle_init_with_oauth_options(
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
async fn handle_set_profile(
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
async fn handle_add_profile_with_oauth_options(
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
async fn handle_edit_profile_with_oauth_options(
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
async fn handle_set_profile_with_oauth_options(
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
async fn handle_list_profiles(
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

async fn handle_find_profile(
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

fn normalize_instance_selector(input: &str) -> anyhow::Result<InstanceSelector> {
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

fn service_now_instance_name(host: &str) -> Option<String> {
    if !host.contains('.') {
        return Some(host.to_string());
    }

    host.strip_suffix(".service-now.com")
        .and_then(|without_suffix| without_suffix.split('.').next())
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
}

fn profile_matches_instance(profile_instance: &str, selector: &InstanceSelector) -> bool {
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

async fn handle_list_now_sdk_profiles(output_format: &OutputFormat) -> anyhow::Result<()> {
    let profiles = now_sdk::list_profiles()?;
    output::print_list(&profiles, output_format)
}

async fn handle_import_now_sdk(
    config_path: &Path,
    output_format: &OutputFormat,
    alias: Option<String>,
    all: bool,
    set_default: bool,
) -> anyhow::Result<()> {
    validate_now_sdk_selector(alias.as_ref(), all)?;

    if set_default && all {
        anyhow::bail!("`--set-default` is only supported when importing a single now-sdk alias.");
    }

    let store = now_sdk::load_store()?;
    let selected_aliases = selected_now_sdk_aliases(&store, alias.as_deref(), all)?;

    let mut imports = Vec::new();
    for alias_name in &selected_aliases {
        let Some(entry) = store.get(alias_name) else {
            anyhow::bail!("now-sdk alias '{}' not found.", alias_name);
        };
        let basic = entry.as_basic_profile().ok_or_else(|| {
            anyhow::anyhow!(
                "now-sdk alias '{}' uses unsupported auth type '{}'; only basic is supported.",
                alias_name,
                entry.auth_type(),
            )
        })?;
        imports.push(basic);
    }

    if imports.is_empty() {
        let result = ImportNowSdkResult {
            status: "imported",
            imported_count: 0,
            imported_profiles: Vec::new(),
            default_profile: None,
        };
        return output::print_output(&result, output_format);
    }

    let config_existed = config_path.exists();
    let mut config = AppConfig::load_from(config_path)?;
    let snapshots = snapshot_profiles_for_import(&config, &imports)?;
    let original_default = config.default_profile.clone();

    let apply_result = (|| -> anyhow::Result<ImportNowSdkResult> {
        let mut imported_profiles = Vec::new();
        for imported in &imports {
            config.profiles.insert(
                imported.alias.clone(),
                Profile {
                    instance: imported.instance.clone(),
                    auth_method: AuthMethod::Basic,
                    username: Some(imported.username.clone()),
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
            credentials::store_credential(
                &imported.alias,
                "password",
                imported.password.expose_secret(),
            )?;
            imported_profiles.push(ImportedProfileSummary {
                profile: imported.alias.clone(),
                source_alias: imported.alias.clone(),
                instance: imported.instance.clone(),
                auth_method: "basic",
            });
        }

        if config.default_profile.is_empty()
            && let Some(default_alias) = imported_default_alias(&imports, all)
        {
            config.default_profile = default_alias.to_string();
        }
        if set_default {
            config.default_profile = imports[0].alias.clone();
        }

        config.save_to(config_path)?;

        Ok(ImportNowSdkResult {
            status: "imported",
            imported_count: imported_profiles.len(),
            imported_profiles,
            default_profile: if set_default {
                Some(config.default_profile.clone())
            } else {
                None
            },
        })
    })();

    match apply_result {
        Ok(result) => output::print_output(&result, output_format),
        Err(error) => {
            restore_profiles_after_failed_import(
                &mut config,
                config_path,
                &snapshots,
                &original_default,
                config_existed,
            )?;
            Err(error)
        }
    }
}

async fn handle_export_now_sdk(
    config_path: &Path,
    output_format: &OutputFormat,
    profile_name: String,
    alias: Option<String>,
    set_default: bool,
) -> anyhow::Result<()> {
    let config = AppConfig::load_from(config_path)?;
    let profile = config
        .profiles
        .get(&profile_name)
        .ok_or_else(|| anyhow::anyhow!(config.profile_not_found_message(&profile_name)))?;

    if profile.auth_method != AuthMethod::Basic {
        anyhow::bail!(
            "Profile '{}' uses unsupported auth method '{:?}' for now-sdk export; only basic is supported.",
            profile_name,
            profile.auth_method,
        );
    }

    let username = profile.username.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Profile '{}' is missing a username required for now-sdk basic export.",
            profile_name,
        )
    })?;
    let password = credentials::get_credential(&profile_name, "password")?.ok_or_else(|| {
        anyhow::anyhow!(
            "No password stored for profile '{}'. Run `snow-cli auth login --profile {}` first.",
            profile_name,
            profile_name,
        )
    })?;

    let alias_name = alias.unwrap_or_else(|| profile_name.clone());
    let snapshot = now_sdk::snapshot_raw_store()?;
    let apply_result = (|| -> anyhow::Result<ExportNowSdkResult> {
        let mut store = now_sdk::load_store()?;
        now_sdk::upsert_basic_alias(
            &mut store,
            &alias_name,
            &profile.instance,
            username,
            &password,
            set_default,
        );
        now_sdk::save_store(&store)?;

        Ok(ExportNowSdkResult {
            status: "exported",
            profile: profile_name.clone(),
            alias: alias_name.clone(),
            instance: profile.instance.clone(),
            auth_method: "basic",
            set_default,
        })
    })();

    match apply_result {
        Ok(result) => output::print_output(&result, output_format),
        Err(error) => {
            now_sdk::restore_raw_store(snapshot.as_deref())?;
            Err(error)
        }
    }
}

fn validate_now_sdk_selector(alias: Option<&String>, all: bool) -> anyhow::Result<()> {
    match (alias.is_some(), all) {
        (true, true) => anyhow::bail!("Use either `--alias <name>` or `--all`, not both."),
        (false, false) => anyhow::bail!("Select a now-sdk alias with `--alias <name>` or `--all`."),
        _ => Ok(()),
    }
}

fn selected_now_sdk_aliases(
    store: &now_sdk::AliasStore,
    alias: Option<&str>,
    all: bool,
) -> anyhow::Result<Vec<String>> {
    if all {
        return Ok(store.keys().cloned().collect());
    }

    let alias =
        alias.ok_or_else(|| anyhow::anyhow!("alias must be present when --all is false"))?;
    if !store.contains_key(alias) {
        anyhow::bail!("now-sdk alias '{}' not found.", alias);
    }

    Ok(vec![alias.to_string()])
}

fn imported_default_alias(imports: &[now_sdk::BasicProfile], importing_all: bool) -> Option<&str> {
    if importing_all {
        imports
            .iter()
            .find(|profile| profile.is_default)
            .map(|profile| profile.alias.as_str())
            .or_else(|| imports.first().map(|profile| profile.alias.as_str()))
    } else {
        imports.first().map(|profile| profile.alias.as_str())
    }
}

fn snapshot_profiles_for_import(
    config: &AppConfig,
    imports: &[now_sdk::BasicProfile],
) -> anyhow::Result<Vec<(String, ProfileCredentialSnapshot)>> {
    let mut snapshots = Vec::new();
    for imported in imports {
        snapshots.push((
            imported.alias.clone(),
            ProfileCredentialSnapshot {
                config: config.profiles.get(&imported.alias).cloned(),
                password: credentials::snapshot_stored_credential(&imported.alias, "password")?,
            },
        ));
    }
    Ok(snapshots)
}

fn restore_profiles_after_failed_import(
    config: &mut AppConfig,
    config_path: &Path,
    snapshots: &[(String, ProfileCredentialSnapshot)],
    original_default: &str,
    config_existed: bool,
) -> anyhow::Result<()> {
    for (profile_name, snapshot) in snapshots {
        match &snapshot.config {
            Some(profile) => {
                config
                    .profiles
                    .insert(profile_name.clone(), profile.clone());
            }
            None => {
                config.profiles.remove(profile_name);
            }
        }
        credentials::restore_stored_credential(
            profile_name,
            "password",
            snapshot.password.as_deref(),
        )?;
    }

    config.default_profile = original_default.to_string();
    if !config_existed && config.profiles.is_empty() && original_default.is_empty() {
        if config_path.exists() {
            std::fs::remove_file(config_path)?;
        }
    } else {
        config.save_to(config_path)?;
    }
    Ok(())
}

/// `profile default <name>` — Set the default profile.
async fn handle_default_profile(config_path: &Path, name: String) -> anyhow::Result<()> {
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

/// `config output [FORMAT] [--reset]` — show or set the default output format.
///
/// With no arguments, prints the configured default and the effective format it
/// resolves to (json when unset). With a FORMAT, persists it. With `--reset`,
/// clears the setting so the built-in json fallback applies.
async fn handle_output_default(
    config_path: &Path,
    format: Option<OutputFormat>,
    reset: bool,
) -> anyhow::Result<()> {
    let mut config = AppConfig::load_from(config_path)?;

    if reset {
        config.default_output = None;
        config.save_to(config_path)?;
        tracing::info!("Cleared default output format");
        let result = serde_json::json!({
            "status": "updated",
            "default_output": serde_json::Value::Null,
        });
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }

    match format {
        Some(fmt) => {
            config.default_output = Some(fmt.as_str().to_string());
            config.save_to(config_path)?;
            tracing::info!("Default output format set to '{}'", fmt.as_str());
            let result = serde_json::json!({
                "status": "updated",
                "default_output": fmt.as_str(),
            });
            println!("{}", serde_json::to_string(&result)?);
        }
        None => {
            let configured = config.default_output.as_deref();
            // Effective value from config alone (ignoring env/flag): the stored
            // value if valid, else the json fallback.
            let effective = configured
                .and_then(OutputFormat::from_str_opt)
                .unwrap_or(OutputFormat::Json);
            let result = serde_json::json!({
                "default_output": configured,
                "effective": effective.as_str(),
            });
            println!("{}", serde_json::to_string(&result)?);
        }
    }

    Ok(())
}

/// `profile current` — Show the currently selected profile summary.
async fn handle_current(
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
async fn handle_delete_profile(
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

    if is_default {
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
            None,
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
        let result = handle_init(&config_path, None, None, None, None, "default".to_string()).await;
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
            None,
            "default".to_string(),
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn test_config_init_with_oauth_grant_type() {
        let (_tmp, config_path) = temp_config_path();
        handle_init(
            &config_path,
            Some("https://test.service-now.com".to_string()),
            Some(CliAuthMethod::Oauth2),
            Some("admin".to_string()),
            Some(CliOAuthGrantType::Password),
            "default".to_string(),
        )
        .await
        .unwrap();

        let config = AppConfig::load_from(&config_path).unwrap();
        let profile = config.profiles.get("default").unwrap();
        assert_eq!(profile.auth_method, AuthMethod::Oauth2);
        assert_eq!(profile.oauth_grant_type, Some(OAuthGrantType::Password));
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
            None,
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
    async fn test_config_set_profile_with_oauth_grant_type() {
        let (_tmp, config_path) = temp_config_path();
        handle_init(
            &config_path,
            Some("https://dev.service-now.com".to_string()),
            None,
            None,
            None,
            "dev".to_string(),
        )
        .await
        .unwrap();

        handle_set_profile(
            &config_path,
            "oauth-prod".to_string(),
            Some("https://prod.service-now.com".to_string()),
            Some(CliAuthMethod::Oauth2),
            Some("svc_account".to_string()),
            Some("client_abc".to_string()),
            Some(CliOAuthGrantType::Password),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let config = AppConfig::load_from(&config_path).unwrap();
        let profile = config.profiles.get("oauth-prod").unwrap();
        assert_eq!(profile.auth_method, AuthMethod::Oauth2);
        assert_eq!(profile.oauth_grant_type, Some(OAuthGrantType::Password));
        assert_eq!(profile.username, Some("svc_account".to_string()));
        assert_eq!(profile.client_id, Some("client_abc".to_string()));
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
            None,
            None,
        )
        .await
        .unwrap();

        // Switch to prod
        handle_default_profile(&config_path, "prod".to_string())
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
            None,
            "dev".to_string(),
        )
        .await
        .unwrap();

        let result = handle_default_profile(&config_path, "nonexistent".to_string()).await;
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
            None,
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
            None,
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
            None,
            None,
        )
        .await
        .unwrap();

        // Should not error
        let result = handle_list_profiles(&config_path, &OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_config_delete_profile_non_default() {
        let (_tmp, config_path) = temp_config_path();
        handle_init(
            &config_path,
            Some("https://dev.service-now.com".to_string()),
            None,
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
            None,
            None,
        )
        .await
        .unwrap();

        handle_delete_profile(&config_path, "prod".to_string(), false, None)
            .await
            .unwrap();

        let config = AppConfig::load_from(&config_path).unwrap();
        assert!(!config.profiles.contains_key("prod"));
        assert_eq!(config.default_profile, "dev");
    }

    #[tokio::test]
    async fn test_config_delete_default_requires_confirmation() {
        let (_tmp, config_path) = temp_config_path();
        handle_init(
            &config_path,
            Some("https://dev.service-now.com".to_string()),
            None,
            None,
            None,
            "dev".to_string(),
        )
        .await
        .unwrap();

        let result = handle_delete_profile(&config_path, "dev".to_string(), false, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("current default"));
    }
}
