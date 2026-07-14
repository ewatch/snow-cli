use super::*;

#[derive(Debug, Clone)]
pub(super) struct ProfileCredentialSnapshot {
    config: Option<Profile>,
    password: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct ImportNowSdkResult {
    status: &'static str,
    imported_count: usize,
    imported_profiles: Vec<ImportedProfileSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_profile: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct ImportedProfileSummary {
    profile: String,
    source_alias: String,
    instance: String,
    auth_method: &'static str,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct ExportNowSdkResult {
    status: &'static str,
    profile: String,
    alias: String,
    instance: String,
    auth_method: &'static str,
    set_default: bool,
}

pub(super) async fn handle_list_now_sdk_profiles(
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let profiles = now_sdk::list_profiles()?;
    output::print_list(&profiles, output_format)
}

pub(super) async fn handle_import_now_sdk(
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

pub(super) async fn handle_export_now_sdk(
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

pub(super) fn validate_now_sdk_selector(alias: Option<&String>, all: bool) -> anyhow::Result<()> {
    match (alias.is_some(), all) {
        (true, true) => anyhow::bail!("Use either `--alias <name>` or `--all`, not both."),
        (false, false) => anyhow::bail!("Select a now-sdk alias with `--alias <name>` or `--all`."),
        _ => Ok(()),
    }
}

pub(super) fn selected_now_sdk_aliases(
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

pub(super) fn imported_default_alias(
    imports: &[now_sdk::BasicProfile],
    importing_all: bool,
) -> Option<&str> {
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

pub(super) fn snapshot_profiles_for_import(
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

pub(super) fn restore_profiles_after_failed_import(
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
