//! Credential storage backed by an encrypted local vault (`~/.servicenow/credentials.enc`)
//! with configurable key sources (`keyring` default, `file`, `env`, or `legacy-keychain`)
//! and environment variable fallback.
//!
//! Entries are keyed by `<profile>:<credential_type>`, for example:
//! `dev:password`, `prod:client_secret`, `staging:api_token`.
//!
//! ## Storage Modes
//!
//! - `keyring` (default): AES-256-GCM encrypted vault with a single 32-byte master
//!   key in the OS keychain (`service = "snow-cli"`, `account = "credentials-key"`
//!   for `~/.servicenow` or directory-scoped for custom `SNOW_CLI_CONFIG`). Reads
//!   fall back transparently to legacy per-profile keychain entries and then
//!   environment variables.
//! - `file`: AES-256-GCM encrypted vault with an owner-only key file
//!   (`~/.servicenow/credentials.key`, mode `0600` on Unix). Never touches the OS
//!   keychain.
//! - `env`: AES-256-GCM encrypted vault keyed by `SNOW_CLI_ENCRYPTION_KEY`. Never
//!   touches the OS keychain.
//! - `legacy-keychain`: legacy per-profile OS keychain entries (`snow-cli` /
//!   `<profile>:<credential_type>`).
//!
//! ## Environment Variable Fallback
//!
//! For headless/CI environments, individual credentials can also be provided via:
//!
//! - `SNOW_CLI_PASSWORD` — password for basic auth
//! - `SNOW_CLI_API_TOKEN` — API token
//! - `SNOW_CLI_CLIENT_SECRET` — OAuth2 client secret
//! - `SNOW_CLI_OAUTH_TOKEN` — persisted OAuth2 authorization-code token JSON
//! - `SNOW_CLI_SESSION_COOKIE` — browser session cookie

use crate::config::error::ConfigError;
use crate::config::keychain;
use crate::config::profile::AppConfig;
use crate::config::vault::{self, CredentialStoreMode, VaultLookup};

pub(crate) const SERVICE_NAME: &str = "snow-cli";
pub const ALL_CREDENTIAL_TYPES: &[&str] = &[
    "password",
    "api_token",
    "client_secret",
    "oauth_token",
    "cert_passphrase",
    "saml_token",
    "session_cookie",
    "native_session",
];

/// Map credential type to its environment variable name.
fn env_var_for(credential_type: &str) -> Option<&'static str> {
    match credential_type {
        "password" => Some("SNOW_CLI_PASSWORD"),
        "api_token" => Some("SNOW_CLI_API_TOKEN"),
        "client_secret" => Some("SNOW_CLI_CLIENT_SECRET"),
        "oauth_token" => Some("SNOW_CLI_OAUTH_TOKEN"),
        "session_cookie" => Some("SNOW_CLI_SESSION_COOKIE"),
        _ => None,
    }
}

fn env_fallback(credential_type: &str) -> Option<String> {
    let env_var = env_var_for(credential_type)?;
    let value = std::env::var(env_var).ok()?;
    tracing::debug!(env_var, "Retrieved credential from environment variable");
    Some(value)
}

fn is_unreadable_vault_error(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<ConfigError>()
        .is_some_and(|cfg| cfg.code == vault::UNREADABLE_CODE)
}

/// Store a credential in the active credential store.
pub fn store_credential(profile: &str, credential_type: &str, value: &str) -> anyhow::Result<()> {
    let key = format!("{profile}:{credential_type}");
    let mode = vault::resolve_mode()?.mode;

    if mode.uses_vault() {
        let config_dir = AppConfig::config_dir();
        match vault::set_entry(mode, &config_dir, &key, value) {
            Ok(()) => {
                tracing::debug!(
                    mode = mode.as_str(),
                    entry = %key,
                    "Stored credential in vault"
                );
                Ok(())
            }
            Err(error) => {
                tracing::warn!(
                    mode = mode.as_str(),
                    %error,
                    "Failed to store credential in vault; in headless environments use credential_store='file', 'env', or environment variables"
                );
                Err(error)
            }
        }
    } else {
        match keychain::set_password(SERVICE_NAME, &key, value) {
            Ok(()) => {
                tracing::debug!(entry = %key, "Stored credential in legacy keychain");
                Ok(())
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    "Failed to store credential in keychain; in headless environments use environment variables instead"
                );
                Err(error)
            }
        }
    }
}

/// Retrieve a credential from the active credential store, falling back to
/// legacy per-profile keychain entries (in `keyring` mode) and environment variables.
pub fn get_credential(profile: &str, credential_type: &str) -> anyhow::Result<Option<String>> {
    let key = format!("{profile}:{credential_type}");
    let mode = vault::resolve_mode()?.mode;

    if mode.uses_vault() {
        let config_dir = AppConfig::config_dir();
        match vault::get_entry(mode, &config_dir, &key) {
            Ok(VaultLookup::Found(secret)) => {
                tracing::debug!(
                    mode = mode.as_str(),
                    entry = %key,
                    "Retrieved credential from vault"
                );
                return Ok(Some(secret));
            }
            Ok(VaultLookup::Missing) => {
                tracing::debug!(
                    mode = mode.as_str(),
                    entry = %key,
                    "No credential found in vault"
                );
            }
            Err(error) => {
                if is_unreadable_vault_error(&error) {
                    return Err(error);
                }
                tracing::debug!(
                    mode = mode.as_str(),
                    entry = %key,
                    %error,
                    "Vault read failed, trying fallbacks"
                );
            }
        }

        if mode == CredentialStoreMode::Keyring {
            match keychain::get_password(SERVICE_NAME, &key) {
                Ok(Some(password)) => {
                    tracing::debug!(
                        entry = %key,
                        "Retrieved credential from legacy keychain fallback"
                    );
                    return Ok(Some(password));
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::debug!(
                        entry = %key,
                        %error,
                        "Legacy keychain fallback failed, trying env var fallback"
                    );
                }
            }
        }
    } else {
        match keychain::get_password(SERVICE_NAME, &key) {
            Ok(Some(password)) => {
                tracing::debug!(entry = %key, "Retrieved credential from legacy keychain");
                return Ok(Some(password));
            }
            Ok(None) => {
                tracing::debug!(entry = %key, "No credential found in legacy keychain");
            }
            Err(error) => {
                tracing::debug!(
                    entry = %key,
                    %error,
                    "Keychain error, trying env var fallback"
                );
            }
        }
    }

    Ok(env_fallback(credential_type))
}

/// Retrieve a credential directly from the backing store without env fallback.
pub fn snapshot_stored_credential(
    profile: &str,
    credential_type: &str,
) -> anyhow::Result<Option<String>> {
    let key = format!("{profile}:{credential_type}");
    let mode = vault::resolve_mode()?.mode;

    if mode.uses_vault() {
        let config_dir = AppConfig::config_dir();
        match vault::get_entry(mode, &config_dir, &key)? {
            VaultLookup::Found(secret) => Ok(Some(secret)),
            VaultLookup::Missing => {
                if mode == CredentialStoreMode::Keyring {
                    keychain::get_password(SERVICE_NAME, &key)
                } else {
                    Ok(None)
                }
            }
        }
    } else {
        keychain::get_password(SERVICE_NAME, &key)
    }
}

/// Restore a raw stored credential value without consulting env vars.
pub fn restore_stored_credential(
    profile: &str,
    credential_type: &str,
    value: Option<&str>,
) -> anyhow::Result<()> {
    match value {
        Some(secret) => store_credential(profile, credential_type, secret),
        None => delete_credential(profile, credential_type),
    }
}

/// Delete a single credential from the active store (and legacy keychain in `keyring` mode).
pub fn delete_credential(profile: &str, credential_type: &str) -> anyhow::Result<()> {
    delete_credentials(profile, &[credential_type])
}

/// Delete multiple credential types for `profile` in a single atomic vault update.
///
/// In `keyring` mode, legacy per-profile keychain entries are also deleted even if
/// the vault update fails, and any legacy deletion error (other than `NoEntry`, which
/// maps to `Ok(())`) is propagated so logout never reports success while a fallback
/// legacy secret remains readable.
pub fn delete_credentials(profile: &str, credential_types: &[&str]) -> anyhow::Result<()> {
    if credential_types.is_empty() {
        return Ok(());
    }
    let keys: Vec<String> = credential_types
        .iter()
        .map(|cred_type| format!("{profile}:{cred_type}"))
        .collect();
    let mode = vault::resolve_mode()?.mode;

    if mode.uses_vault() {
        let config_dir = AppConfig::config_dir();
        let vault_result = vault::delete_entries(mode, &config_dir, &keys);
        let mut legacy_error: Option<anyhow::Error> = None;

        if mode == CredentialStoreMode::Keyring {
            for key in &keys {
                if let Err(error) = keychain::delete_password(SERVICE_NAME, key) {
                    tracing::warn!(
                        entry = %key,
                        %error,
                        "Failed to delete legacy keychain credential"
                    );
                    if legacy_error.is_none() {
                        legacy_error = Some(error);
                    }
                }
            }
        }

        vault_result?;
        if let Some(error) = legacy_error {
            return Err(error);
        }
        Ok(())
    } else {
        for key in &keys {
            match keychain::delete_password(SERVICE_NAME, key) {
                Ok(()) => {
                    tracing::debug!(entry = %key, "Deleted credential from legacy keychain");
                }
                Err(error) => {
                    tracing::warn!(entry = %key, %error, "Failed to delete credential from keychain");
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}

/// Delete all known credential types for `profile` (used by `profile remove`).
pub fn delete_profile_credentials(profile: &str) -> anyhow::Result<()> {
    delete_credentials(profile, ALL_CREDENTIAL_TYPES)
}

/// Check if a credential exists (in the active store or environment), propagating
/// configuration and unreadable-vault errors instead of hiding them as `false`.
pub fn check_credential_present(profile: &str, credential_type: &str) -> anyhow::Result<bool> {
    Ok(get_credential(profile, credential_type)?.is_some())
}

/// Check if a credential exists (in the active store or environment).
pub fn has_credential(profile: &str, credential_type: &str) -> bool {
    matches!(get_credential(profile, credential_type), Ok(Some(_)))
}

/// Return the credential type expected for a given auth method.
pub fn credential_type_for_auth(auth_method: &crate::config::profile::AuthMethod) -> &'static str {
    match auth_method {
        crate::config::profile::AuthMethod::Basic => "password",
        crate::config::profile::AuthMethod::NativeSession => "native_session",
        crate::config::profile::AuthMethod::Oauth2 => "client_secret",
        crate::config::profile::AuthMethod::ApiKey => "api_token",
        crate::config::profile::AuthMethod::Mtls => "cert_passphrase",
        crate::config::profile::AuthMethod::BrowserSession => "session_cookie",
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
        assert_eq!(env_var_for("oauth_token"), Some("SNOW_CLI_OAUTH_TOKEN"));
        assert_eq!(
            env_var_for("session_cookie"),
            Some("SNOW_CLI_SESSION_COOKIE")
        );
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
