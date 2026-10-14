use crate::cli::{
    args::{OutputFormat, SessionArgs, SessionCommands},
    output,
};
use crate::client::SnowClient;
use crate::config::{
    AppConfig, AuthMethod, Profile, credentials, error::ConfigError, native_session,
};
use crate::models::{secret::Secret, validation::ValidationError};

pub async fn handle(
    args: SessionArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    match args.command {
        SessionCommands::Login {
            name,
            password_stdin,
        } => {
            let mut config = AppConfig::load()?;
            let source = config.active_profile(Some(profile)).ok_or_else(|| {
                ConfigError::new(
                    "PROFILE_NOT_FOUND",
                    config.profile_not_found_message(profile),
                )
            })?;
            anyhow::ensure!(
                source.auth_method == AuthMethod::Basic,
                ValidationError::new(
                    "Native login requires a Basic profile. For SSO/MFA or another authentication method, use session attach --cookie-stdin with an authenticated cookie session."
                )
            );
            let username = source
                .username
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Basic profile has no username"))?;
            let target_instance = instance.unwrap_or(&source.instance).to_string();
            validate_target(&config, profile, &name, &target_instance)?;
            let password = if password_stdin {
                read_secret_stdin("password")?
            } else {
                Secret::new(
                    credentials::get_credential(profile, "password")?.ok_or_else(|| {
                        anyhow::anyhow!("No stored password; use --password-stdin or auth login")
                    })?,
                )
            };
            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
            let session = client.native_login(&username, &password).await?;
            persist(&mut config, &name, &session)?;
            output::print_output(
                &serde_json::json!({"status":"ready", "profile":name, "instance":session.instance, "auth_method":"native_session", "authenticated_user":session.authenticated_user, "stored":true}),
                format,
            )
        }
        SessionCommands::Attach {
            name,
            cookie_stdin: _,
        } => {
            let mut config = AppConfig::load()?;
            let source = config.active_profile(Some(profile)).ok_or_else(|| {
                ConfigError::new(
                    "PROFILE_NOT_FOUND",
                    config.profile_not_found_message(profile),
                )
            })?;
            let target_instance = crate::config::profile::validate_instance_url(
                instance.unwrap_or(&source.instance),
            )?;
            validate_target(&config, profile, &name, &target_instance)?;
            let session = native_session::NativeSession {
                instance: target_instance.clone(),
                cookie_header: read_secret_stdin("cookie header")?,
                csrf_token: Secret::new(String::new()),
                authenticated_user: None,
            };
            // Cookie-only validation; the source profile's authentication is never consulted.
            let mut client = SnowClient::with_config(
                target_instance,
                Box::new(crate::auth::native_session::NativeSessionAuth::transient(
                    session,
                )),
                crate::client::ClientConfig {
                    timeout_secs: timeout_secs.unwrap_or(90),
                    policy: crate::policy::ExecutionPolicy::read_only(),
                },
            )?;
            let inspection = client.inspect_native_session().await?;
            let session = client.native_credentials().await?;
            persist(&mut config, &name, &session)?;
            output::print_output(
                &serde_json::json!({"status":"ready", "profile":name, "instance":session.instance, "auth_method":"native_session", "effective_user":inspection.effective_user, "authenticated_user":null, "stored":true}),
                format,
            )
        }
        SessionCommands::Forget => {
            require_native_profile(profile)?;
            native_session::forget(profile)?;
            output::print_output(
                &serde_json::json!({"profile":profile,"status":"forgotten","remote_logout":false}),
                format,
            )
        }
        command => {
            require_native_profile(profile)?;
            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
            match command {
                SessionCommands::Show => {
                    output::print_output(&client.inspect_native_session().await?, format)
                }
                SessionCommands::Options { kind } => {
                    output::print_list(&client.native_session_options(kind).await?, format)
                }
                SessionCommands::Switch { kind, value } => {
                    if matches!(format, OutputFormat::Csv) {
                        return Err(crate::models::validation::ValidationError::new(
                            "session switch does not support --output csv; use json, jsonl, toon, text, or auto",
                        ).into());
                    }
                    let (before, after) = client.switch_native_session(kind, &value).await?;
                    let changed = before.selection(kind).map(|context| &context.id)
                        != after.selection(kind).map(|context| &context.id);
                    output::print_output(
                        &serde_json::json!({"verified":true,"changed":changed,"switched":kind.endpoint(),"before":before,"after":after}),
                        format,
                    )
                }
                _ => anyhow::bail!("Unexpected session lifecycle command"),
            }
        }
    }
}

fn require_native_profile(profile: &str) -> anyhow::Result<()> {
    let config = AppConfig::load()?;
    anyhow::ensure!(
        config
            .active_profile(Some(profile))
            .is_some_and(|profile| profile.auth_method == AuthMethod::NativeSession),
        ValidationError::new(
            "Select a native_session profile created by session login/attach; ordinary Basic/OAuth profiles are separate sessions"
        )
    );
    Ok(())
}

fn read_secret_stdin(label: &str) -> anyhow::Result<Secret> {
    use std::io::IsTerminal;
    anyhow::ensure!(
        !std::io::stdin().is_terminal(),
        ValidationError::new(format!("Pipe the {label} to stdin"))
    );
    let value = crate::cli::io::read_to_string_limited(
        std::io::stdin().lock(),
        64 * 1024,
        "session credential",
    )?;
    let value = value.trim_end_matches(['\r', '\n']).to_string();
    anyhow::ensure!(
        !value.is_empty() && !value.contains(['\r', '\n']),
        ValidationError::new(format!("Expected one nonempty line for the {label}"))
    );
    Ok(Secret::new(value))
}

fn validate_target(
    config: &AppConfig,
    source: &str,
    name: &str,
    instance: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !name.is_empty()
            && name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')),
        ValidationError::new(
            "Session profile names may contain letters, digits, dot, dash, or underscore"
        )
    );
    anyhow::ensure!(
        source != name,
        ValidationError::new(
            "Use a separate session profile name so the source credential profile is preserved"
        )
    );
    if let Some(existing) = config.profiles.get(name) {
        anyhow::ensure!(
            existing.auth_method == AuthMethod::NativeSession
                && url::Url::parse(&existing.instance)?.origin()
                    == url::Url::parse(instance)?.origin(),
            ValidationError::new(
                "Target profile already exists with different authentication or instance; choose another name"
            )
        );
    }
    Ok(())
}

fn persist(
    config: &mut AppConfig,
    name: &str,
    session: &native_session::NativeSession,
) -> anyhow::Result<()> {
    let previous = credentials::snapshot_stored_credential(name, native_session::CREDENTIAL_TYPE)?;
    native_session::save(name, session)?;
    let profile: Profile = serde_json::from_value(
        serde_json::json!({"instance":session.instance,"auth_method":"native_session"}),
    )?;
    config.profiles.insert(name.to_string(), profile);
    if let Err(error) = config.save() {
        credentials::restore_stored_credential(
            name,
            native_session::CREDENTIAL_TYPE,
            previous.as_deref(),
        )?;
        return Err(error);
    }
    Ok(())
}
