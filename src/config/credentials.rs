//! Credential storage using the OS keychain.
//!
//! Credentials are stored with the service name "snow-cli" and keyed
//! by profile name and credential type.
//!
//! Key format: `<profile>:<credential_type>`
//! Example: `dev:password`, `prod:client_secret`, `staging:api_token`

const SERVICE_NAME: &str = "snow-cli";

/// Store a credential in the OS keychain.
pub fn store_credential(profile: &str, credential_type: &str, value: &str) -> anyhow::Result<()> {
    let key = format!("{profile}:{credential_type}");
    let entry = keyring::Entry::new(SERVICE_NAME, &key)?;
    entry.set_password(value)?;
    tracing::debug!("Stored credential {key} in keychain");
    Ok(())
}

/// Retrieve a credential from the OS keychain.
pub fn get_credential(profile: &str, credential_type: &str) -> anyhow::Result<Option<String>> {
    let key = format!("{profile}:{credential_type}");
    let entry = keyring::Entry::new(SERVICE_NAME, &key)?;
    match entry.get_password() {
        Ok(password) => {
            tracing::debug!("Retrieved credential {key} from keychain");
            Ok(Some(password))
        }
        Err(keyring::Error::NoEntry) => {
            tracing::debug!("No credential found for {key}");
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

/// Delete a credential from the OS keychain.
pub fn delete_credential(profile: &str, credential_type: &str) -> anyhow::Result<()> {
    let key = format!("{profile}:{credential_type}");
    let entry = keyring::Entry::new(SERVICE_NAME, &key)?;
    match entry.delete_credential() {
        Ok(()) => {
            tracing::debug!("Deleted credential {key} from keychain");
            Ok(())
        }
        Err(keyring::Error::NoEntry) => {
            tracing::debug!("No credential to delete for {key}");
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}
