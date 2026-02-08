use crate::cli::args::{AuthArgs, AuthCommands};
use crate::config::credentials;
use crate::config::profile::{AppConfig, OAuthGrantType};

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
///
/// For OAuth2 password grant, both `--client-secret` and `--password` are required
/// (two separate keychain entries).
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

    match &profile.auth_method {
        crate::config::profile::AuthMethod::Basic => {
            let pw = password.ok_or_else(|| {
                anyhow::anyhow!(
                    "Password required for basic auth. Use: snow-cli auth login --password <password>"
                )
            })?;
            credentials::store_credential(profile_name, "password", &pw)?;

            let result = serde_json::json!({
                "status": "authenticated",
                "profile": profile_name,
                "auth_method": profile.auth_method,
                "credential_type": "password",
            });
            println!("{}", serde_json::to_string(&result)?);
        }
        crate::config::profile::AuthMethod::ApiKey => {
            let tok = token.ok_or_else(|| {
                anyhow::anyhow!("API token required. Use: snow-cli auth login --token <token>")
            })?;
            credentials::store_credential(profile_name, "api_token", &tok)?;

            let result = serde_json::json!({
                "status": "authenticated",
                "profile": profile_name,
                "auth_method": profile.auth_method,
                "credential_type": "api_token",
            });
            println!("{}", serde_json::to_string(&result)?);
        }
        crate::config::profile::AuthMethod::Oauth2 => {
            let secret = client_secret.ok_or_else(|| {
                anyhow::anyhow!(
                    "Client secret required for OAuth2. Use: snow-cli auth login --client-secret <secret>"
                )
            })?;
            credentials::store_credential(profile_name, "client_secret", &secret)?;

            let grant_type = profile
                .oauth_grant_type
                .clone()
                .unwrap_or(OAuthGrantType::ClientCredentials);

            // For password grant, also store the user's password
            if grant_type == OAuthGrantType::Password {
                let pw = password.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Password required for OAuth2 password grant. \
                         Use: snow-cli auth login --client-secret <secret> --password <password>"
                    )
                })?;
                credentials::store_credential(profile_name, "password", &pw)?;
            }

            let result = serde_json::json!({
                "status": "authenticated",
                "profile": profile_name,
                "auth_method": profile.auth_method,
                "oauth_grant_type": grant_type,
                "credential_types": if grant_type == OAuthGrantType::Password {
                    vec!["client_secret", "password"]
                } else {
                    vec!["client_secret"]
                },
            });
            println!("{}", serde_json::to_string(&result)?);
        }
        other => {
            anyhow::bail!(
                "Auth method {:?} does not support `auth login`. Configure credentials manually.",
                other
            );
        }
    }

    tracing::info!("Credentials stored for profile '{}'", profile_name);

    Ok(())
}

/// `auth logout` — Remove stored credentials for the active profile.
///
/// Removes all credential types associated with the profile's auth method.
async fn handle_logout(profile_name: &str) -> anyhow::Result<()> {
    let config = AppConfig::load()?;
    let profile = config
        .active_profile(Some(profile_name))
        .ok_or_else(|| anyhow::anyhow!("Profile '{}' not found.", profile_name))?;

    // Delete all credential types for this auth method
    let cred_types = credential_types_for_auth(profile);
    for cred_type in &cred_types {
        credentials::delete_credential(profile_name, cred_type)?;
    }

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

    let cred_types = credential_types_for_auth(profile);
    let authenticated = cred_types
        .iter()
        .all(|ct| credentials::has_credential(profile_name, ct));

    let result = serde_json::json!({
        "profile": profile_name,
        "instance": profile.instance,
        "auth_method": profile.auth_method,
        "credential_types": cred_types,
        "authenticated": authenticated,
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

    let primary_cred_type = credentials::credential_type_for_auth(&profile.auth_method);
    let credential =
        credentials::get_credential(profile_name, primary_cred_type)?.ok_or_else(|| {
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

/// Return the list of credential types needed for the profile's auth method.
///
/// OAuth2 password grant requires both `client_secret` and `password`.
fn credential_types_for_auth(profile: &crate::config::profile::Profile) -> Vec<&'static str> {
    match &profile.auth_method {
        crate::config::profile::AuthMethod::Basic => vec!["password"],
        crate::config::profile::AuthMethod::ApiKey => vec!["api_token"],
        crate::config::profile::AuthMethod::Oauth2 => {
            let grant_type = profile
                .oauth_grant_type
                .as_ref()
                .cloned()
                .unwrap_or(OAuthGrantType::ClientCredentials);
            if grant_type == OAuthGrantType::Password {
                vec!["client_secret", "password"]
            } else {
                vec!["client_secret"]
            }
        }
        crate::config::profile::AuthMethod::Mtls => vec!["cert_passphrase"],
        crate::config::profile::AuthMethod::Saml => vec!["saml_token"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::profile::{AuthMethod, Profile};

    fn make_profile(auth_method: AuthMethod, grant_type: Option<OAuthGrantType>) -> Profile {
        Profile {
            instance: "https://test.service-now.com".to_string(),
            auth_method,
            username: Some("admin".to_string()),
            client_id: Some("client123".to_string()),
            oauth_grant_type: grant_type,
            cert_path: None,
            key_path: None,
        }
    }

    #[test]
    fn test_credential_types_basic() {
        let profile = make_profile(AuthMethod::Basic, None);
        assert_eq!(credential_types_for_auth(&profile), vec!["password"]);
    }

    #[test]
    fn test_credential_types_api_key() {
        let profile = make_profile(AuthMethod::ApiKey, None);
        assert_eq!(credential_types_for_auth(&profile), vec!["api_token"]);
    }

    #[test]
    fn test_credential_types_oauth2_client_credentials() {
        let profile = make_profile(AuthMethod::Oauth2, Some(OAuthGrantType::ClientCredentials));
        assert_eq!(credential_types_for_auth(&profile), vec!["client_secret"]);
    }

    #[test]
    fn test_credential_types_oauth2_password() {
        let profile = make_profile(AuthMethod::Oauth2, Some(OAuthGrantType::Password));
        assert_eq!(
            credential_types_for_auth(&profile),
            vec!["client_secret", "password"]
        );
    }

    #[test]
    fn test_credential_types_oauth2_default_grant() {
        // No grant type set — should default to client_credentials
        let profile = make_profile(AuthMethod::Oauth2, None);
        assert_eq!(credential_types_for_auth(&profile), vec!["client_secret"]);
    }

    #[test]
    fn test_credential_types_mtls() {
        let profile = make_profile(AuthMethod::Mtls, None);
        assert_eq!(credential_types_for_auth(&profile), vec!["cert_passphrase"]);
    }

    #[test]
    fn test_credential_types_saml() {
        let profile = make_profile(AuthMethod::Saml, None);
        assert_eq!(credential_types_for_auth(&profile), vec!["saml_token"]);
    }
}
