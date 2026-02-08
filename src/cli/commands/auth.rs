use crate::cli::args::{AuthArgs, AuthCommands};
use crate::config::credentials;
use crate::config::profile::AppConfig;

pub async fn handle(args: AuthArgs, profile_name: &str) -> anyhow::Result<()> {
    match args.command {
        AuthCommands::Login {
            password,
            token,
            client_secret,
        } => handle_login(profile_name, password, token, client_secret).await,
        AuthCommands::Logout => handle_logout(profile_name).await,
        AuthCommands::Status => handle_status(profile_name).await,
        AuthCommands::Token => handle_token(profile_name).await,
    }
}

/// `auth login` — Store credentials for the active profile.
///
/// Credentials are read from flags (--password, --token, --client-secret)
/// or from stdin if not provided. The credential type is determined by the
/// profile's auth_method.
async fn handle_login(
    profile_name: &str,
    password: Option<String>,
    token: Option<String>,
    client_secret: Option<String>,
) -> anyhow::Result<()> {
    let config = AppConfig::load()?;
    let profile = config
        .active_profile(Some(profile_name))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Profile '{}' not found. Run `snow-cli config init` or `snow-cli config set-profile` first.",
                profile_name
            )
        })?;

    let cred_type = credentials::credential_type_for_auth(&profile.auth_method);

    // Determine the credential value based on auth method and provided flags
    let value = match &profile.auth_method {
        crate::config::profile::AuthMethod::Basic => {
            password.ok_or_else(|| {
                anyhow::anyhow!(
                    "Password required for basic auth. Use: snow-cli auth login --password <password>"
                )
            })?
        }
        crate::config::profile::AuthMethod::ApiKey => {
            token.ok_or_else(|| {
                anyhow::anyhow!(
                    "API token required. Use: snow-cli auth login --token <token>"
                )
            })?
        }
        crate::config::profile::AuthMethod::Oauth2 => {
            client_secret.ok_or_else(|| {
                anyhow::anyhow!(
                    "Client secret required for OAuth2. Use: snow-cli auth login --client-secret <secret>"
                )
            })?
        }
        other => {
            anyhow::bail!(
                "Auth method {:?} does not support `auth login`. Configure credentials manually.",
                other
            );
        }
    };

    credentials::store_credential(profile_name, cred_type, &value)?;

    tracing::info!("Credentials stored for profile '{}'", profile_name);

    let result = serde_json::json!({
        "status": "authenticated",
        "profile": profile_name,
        "auth_method": profile.auth_method,
        "credential_type": cred_type,
    });
    println!("{}", serde_json::to_string(&result)?);

    Ok(())
}

/// `auth logout` — Remove stored credentials for the active profile.
async fn handle_logout(profile_name: &str) -> anyhow::Result<()> {
    let config = AppConfig::load()?;
    let profile = config
        .active_profile(Some(profile_name))
        .ok_or_else(|| anyhow::anyhow!("Profile '{}' not found.", profile_name))?;

    let cred_type = credentials::credential_type_for_auth(&profile.auth_method);
    credentials::delete_credential(profile_name, cred_type)?;

    tracing::info!("Credentials removed for profile '{}'", profile_name);

    let result = serde_json::json!({
        "status": "logged_out",
        "profile": profile_name,
    });
    println!("{}", serde_json::to_string(&result)?);

    Ok(())
}

/// `auth status` — Check if credentials are available for the active profile.
async fn handle_status(profile_name: &str) -> anyhow::Result<()> {
    let config = AppConfig::load()?;
    let profile = config
        .active_profile(Some(profile_name))
        .ok_or_else(|| anyhow::anyhow!("Profile '{}' not found.", profile_name))?;

    let cred_type = credentials::credential_type_for_auth(&profile.auth_method);
    let has_cred = credentials::has_credential(profile_name, cred_type);

    let result = serde_json::json!({
        "profile": profile_name,
        "instance": profile.instance,
        "auth_method": profile.auth_method,
        "credential_type": cred_type,
        "authenticated": has_cred,
        "username": profile.username,
    });
    println!("{}", serde_json::to_string_pretty(&result)?);

    Ok(())
}

/// `auth token` — Print the stored credential to stdout for piping.
///
/// This is useful for integrating with other tools:
/// ```bash
/// curl -H "Authorization: Basic $(snow-cli auth token)" https://...
/// ```
async fn handle_token(profile_name: &str) -> anyhow::Result<()> {
    let config = AppConfig::load()?;
    let profile = config
        .active_profile(Some(profile_name))
        .ok_or_else(|| anyhow::anyhow!("Profile '{}' not found.", profile_name))?;

    let cred_type = credentials::credential_type_for_auth(&profile.auth_method);
    let credential = credentials::get_credential(profile_name, cred_type)?.ok_or_else(|| {
        anyhow::anyhow!(
            "No credentials stored for profile '{}'. Run `snow-cli auth login` first.",
            profile_name
        )
    })?;

    // For basic auth, output the base64-encoded "user:pass" token
    match &profile.auth_method {
        crate::config::profile::AuthMethod::Basic => {
            let username = profile.username.as_deref().unwrap_or("");
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD
                .encode(format!("{username}:{credential}"));
            print!("{encoded}");
        }
        _ => {
            // For other auth methods, output the raw credential
            print!("{credential}");
        }
    }

    Ok(())
}
