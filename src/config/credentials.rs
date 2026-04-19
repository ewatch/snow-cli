//! Credential storage using the OS keychain with environment variable fallback.
//!
//! Credentials are stored with the service name "snow-cli" and keyed
//! by profile name and credential type.
//!
//! Key format: `<profile>:<credential_type>`
//! Example: `dev:password`, `prod:client_secret`, `staging:api_token`
//!
//! ## Environment Variable Fallback
//!
//! For headless/CI environments where no keychain is available, credentials
//! can be provided via environment variables:
//!
//! - `SNOW_CLI_PASSWORD` — password for basic auth
//! - `SNOW_CLI_API_TOKEN` — API token
//! - `SNOW_CLI_CLIENT_SECRET` — OAuth2 client secret
//!
//! The keychain is tried first; env vars are used as fallback.

use crate::config::keychain;

pub(crate) const SERVICE_NAME: &str = "snow-cli";
pub const ALL_CREDENTIAL_TYPES: &[&str] = &[
    "password",
    "api_token",
    "client_secret",
    "cert_passphrase",
    "saml_token",
];

/// Map credential type to its environment variable name.
fn env_var_for(credential_type: &str) -> Option<&'static str> {
    match credential_type {
        "password" => Some("SNOW_CLI_PASSWORD"),
        "api_token" => Some("SNOW_CLI_API_TOKEN"),
        "client_secret" => Some("SNOW_CLI_CLIENT_SECRET"),
        _ => None,
    }
}

/// Store a credential in the OS keychain.
pub fn store_credential(profile: &str, credential_type: &str, value: &str) -> anyhow::Result<()> {
    let key = format!("{profile}:{credential_type}");
    match keychain::set_password(SERVICE_NAME, &key, value) {
        Ok(()) => {
            tracing::debug!("Stored credential {key} in keychain");
            Ok(())
        }
        Err(e) => {
            tracing::warn!(
                "Failed to store credential in keychain: {e}. \
                 In headless environments, use environment variables instead."
            );
            Err(e)
        }
    }
}

/// Retrieve a credential from the OS keychain, falling back to environment variables.
pub fn get_credential(profile: &str, credential_type: &str) -> anyhow::Result<Option<String>> {
    let key = format!("{profile}:{credential_type}");
    match keychain::get_password(SERVICE_NAME, &key) {
        Ok(Some(password)) => {
            tracing::debug!("Retrieved credential {key} from keychain");
            return Ok(Some(password));
        }
        Ok(None) => {
            tracing::debug!("No credential found for {key} in keychain");
        }
        Err(e) => {
            tracing::debug!("Keychain error for {key}: {e}, trying env var fallback");
        }
    }

    // Fallback: check environment variable
    if let Some(env_var) = env_var_for(credential_type)
        && let Ok(value) = std::env::var(env_var)
    {
        tracing::debug!("Retrieved credential from env var {env_var}");
        return Ok(Some(value));
    }

    Ok(None)
}

/// Retrieve a credential directly from the backing keychain store without env fallback.
pub fn snapshot_stored_credential(
    profile: &str,
    credential_type: &str,
) -> anyhow::Result<Option<String>> {
    let key = format!("{profile}:{credential_type}");
    keychain::get_password(SERVICE_NAME, &key)
}

/// Restore a raw keychain-backed credential value without consulting env vars.
pub fn restore_stored_credential(
    profile: &str,
    credential_type: &str,
    value: Option<&str>,
) -> anyhow::Result<()> {
    let key = format!("{profile}:{credential_type}");
    match value {
        Some(secret) => keychain::set_password(SERVICE_NAME, &key, secret),
        None => keychain::delete_password(SERVICE_NAME, &key),
    }
}

/// Delete a credential from the OS keychain.
pub fn delete_credential(profile: &str, credential_type: &str) -> anyhow::Result<()> {
    let key = format!("{profile}:{credential_type}");
    match keychain::delete_password(SERVICE_NAME, &key) {
        Ok(()) => {
            tracing::debug!("Deleted credential {key} from keychain");
            Ok(())
        }
        Err(e) => {
            tracing::warn!("Failed to delete credential from keychain: {e}");
            Err(e)
        }
    }
}

/// Check if a credential exists (in keychain or environment).
pub fn has_credential(profile: &str, credential_type: &str) -> bool {
    matches!(get_credential(profile, credential_type), Ok(Some(_)))
}

/// Return the credential type expected for a given auth method.
pub fn credential_type_for_auth(auth_method: &crate::config::profile::AuthMethod) -> &'static str {
    match auth_method {
        crate::config::profile::AuthMethod::Basic => "password",
        crate::config::profile::AuthMethod::Oauth2 => "client_secret",
        crate::config::profile::AuthMethod::ApiKey => "api_token",
        crate::config::profile::AuthMethod::Mtls => "cert_passphrase",
        crate::config::profile::AuthMethod::Saml => "saml_token",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_var_mapping() {
        assert_eq!(env_var_for("password"), Some("SNOW_CLI_PASSWORD"));
        assert_eq!(env_var_for("api_token"), Some("SNOW_CLI_API_TOKEN"));
        assert_eq!(env_var_for("client_secret"), Some("SNOW_CLI_CLIENT_SECRET"));
        assert_eq!(env_var_for("unknown"), None);
    }

    #[test]
    fn test_credential_type_for_auth() {
        use crate::config::profile::AuthMethod;
        assert_eq!(credential_type_for_auth(&AuthMethod::Basic), "password");
        assert_eq!(
            credential_type_for_auth(&AuthMethod::Oauth2),
            "client_secret"
        );
        assert_eq!(credential_type_for_auth(&AuthMethod::ApiKey), "api_token");
    }
}
