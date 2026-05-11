use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type TestStore = BTreeMap<String, BTreeMap<String, String>>;

const TEST_STORE_ENV: &str = "SNOW_CLI_TEST_KEYCHAIN_STORE";
const ALLOW_PLAINTEXT_TEST_STORE_ENV: &str = "SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN";

pub fn get_password(service: &str, account: &str) -> anyhow::Result<Option<String>> {
    if let Some(path) = test_store_path()? {
        let store = load_test_store(&path)?;
        return Ok(store
            .get(service)
            .and_then(|accounts| accounts.get(account))
            .cloned());
    }

    let entry = keyring::Entry::new(service, account)?;
    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn set_password(service: &str, account: &str, value: &str) -> anyhow::Result<()> {
    if let Some(path) = test_store_path()? {
        let mut store = load_test_store(&path)?;
        store
            .entry(service.to_string())
            .or_default()
            .insert(account.to_string(), value.to_string());
        return save_test_store(&path, &store);
    }

    let entry = keyring::Entry::new(service, account)?;
    entry.set_password(value)?;
    Ok(())
}

pub fn delete_password(service: &str, account: &str) -> anyhow::Result<()> {
    if let Some(path) = test_store_path()? {
        let mut store = load_test_store(&path)?;
        if let Some(accounts) = store.get_mut(service) {
            accounts.remove(account);
            if accounts.is_empty() {
                store.remove(service);
            }
        }
        return save_test_store(&path, &store);
    }

    let entry = keyring::Entry::new(service, account)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn test_store_path() -> anyhow::Result<Option<PathBuf>> {
    let Some(path) = std::env::var(TEST_STORE_ENV).ok().map(PathBuf::from) else {
        return Ok(None);
    };

    let allowed = std::env::var(ALLOW_PLAINTEXT_TEST_STORE_ENV)
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            !(normalized.is_empty() || matches!(normalized.as_str(), "0" | "false" | "off" | "no"))
        })
        .unwrap_or(false);

    if !allowed {
        anyhow::bail!(
            "{} is a plaintext test credential store and requires {}=1. Do not use it for real credentials.",
            TEST_STORE_ENV,
            ALLOW_PLAINTEXT_TEST_STORE_ENV
        );
    }

    Ok(Some(path))
}

fn load_test_store(path: &Path) -> anyhow::Result<TestStore> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let contents = std::fs::read_to_string(path)?;
    if contents.trim().is_empty() {
        return Ok(BTreeMap::new());
    }

    Ok(serde_json::from_str(&contents)?)
}

fn save_test_store(path: &Path, store: &TestStore) -> anyhow::Result<()> {
    if store.is_empty() {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let contents = serde_json::to_string_pretty(store)?;

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(contents.as_bytes())?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        std::fs::write(path, contents)?;
        Ok(())
    }
}
