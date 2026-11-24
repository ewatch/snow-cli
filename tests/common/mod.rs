/// Shared test helpers for integration tests.
///
/// Provides utilities for setting up mock ServiceNow servers,
/// creating temporary config files, and building CLI commands.
use std::path::PathBuf;

use serde_json::Value;

/// Create a temporary config file with the given content.
/// Returns the path to the temp directory (which contains config.toml).
#[allow(
    dead_code,
    reason = "shared helpers are compiled separately by each integration test crate"
)]
pub fn create_temp_config(content: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    std::fs::write(&config_path, content).unwrap();
    (dir, config_path)
}

/// Create an isolated fake keychain backing file for CLI tests.
#[allow(
    dead_code,
    reason = "shared helpers are compiled separately by each integration test crate"
)]
pub fn create_temp_keychain_store() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("keychain-store.json");
    (dir, store_path)
}

/// Write a service/account entry into the fake keychain store.
#[allow(
    dead_code,
    reason = "shared helpers are compiled separately by each integration test crate"
)]
pub fn write_test_keychain_entry(
    store_path: &std::path::Path,
    service: &str,
    account: &str,
    value: &str,
) {
    let mut store = read_store(store_path);
    let service_entry = store
        .as_object_mut()
        .unwrap()
        .entry(service.to_string())
        .or_insert_with(|| Value::Object(Default::default()));
    service_entry
        .as_object_mut()
        .unwrap()
        .insert(account.to_string(), Value::String(value.to_string()));
    std::fs::write(store_path, serde_json::to_string_pretty(&store).unwrap()).unwrap();
}

/// Read a service/account entry from the fake keychain store.
#[allow(
    dead_code,
    reason = "shared helpers are compiled separately by each integration test crate"
)]
pub fn read_test_keychain_entry(
    store_path: &std::path::Path,
    service: &str,
    account: &str,
) -> Option<String> {
    let store = read_store(store_path);
    store
        .get(service)
        .and_then(|accounts| accounts.get(account))
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
}

fn read_store(store_path: &std::path::Path) -> Value {
    if !store_path.exists() {
        return Value::Object(Default::default());
    }

    let content = std::fs::read_to_string(store_path).unwrap();
    if content.trim().is_empty() {
        return Value::Object(Default::default());
    }

    serde_json::from_str(&content).unwrap()
}

/// Read a stored credential from the encrypted vault beside `config_path`
/// (using the master key in `keychain_store` or `credentials.key`), falling
/// back to a legacy per-profile entry in `keychain_store` if no vault exists.
#[allow(
    dead_code,
    reason = "shared helpers are compiled separately by each integration test crate"
)]
pub fn read_stored_credential(
    config_path: &std::path::Path,
    keychain_store: &std::path::Path,
    profile: &str,
    credential_type: &str,
) -> Option<String> {
    use base64::Engine as _;
    use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};

    let entry_key = format!("{profile}:{credential_type}");
    let config_dir = config_path.parent().unwrap();
    let vault_path = config_dir.join("credentials.enc");
    if vault_path.exists() {
        let key_account = snow_cli::config::vault::vault_key_account_for(config_dir);
        let key_b64 = read_test_keychain_entry(keychain_store, "snow-cli", &key_account)
            .or_else(|| {
                std::fs::read_to_string(config_dir.join("credentials.key"))
                    .ok()
                    .map(|s| s.trim().to_string())
            })
            .unwrap();
        let key_bytes = base64::engine::general_purpose::STANDARD
            .decode(key_b64)
            .unwrap();
        let file = std::fs::read(&vault_path).unwrap();
        let cbor: ciborium::Value = ciborium::from_reader(file.as_slice()).unwrap();
        let map = cbor.as_map().unwrap();
        let iv = map
            .iter()
            .find(|(k, _)| k.as_text() == Some("iv"))
            .unwrap()
            .1
            .as_bytes()
            .unwrap();
        let mut data = map
            .iter()
            .find(|(k, _)| k.as_text() == Some("data"))
            .unwrap()
            .1
            .as_bytes()
            .unwrap()
            .clone();
        let cipher = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &key_bytes).unwrap());
        let plaintext = cipher
            .open_in_place(
                Nonce::try_assume_unique_for_key(iv).unwrap(),
                Aad::empty(),
                &mut data,
            )
            .unwrap();
        let payload: Value = serde_json::from_slice(plaintext).unwrap();
        if let Some(value) = payload["entries"].get(&entry_key).and_then(|v| v.as_str()) {
            return Some(value.to_string());
        }
    }

    read_test_keychain_entry(keychain_store, "snow-cli", &entry_key)
}

/// now-sdk 4.14+ credential directory beside a temp keychain store, so tests
/// never read or write the developer's real `~/.now-sdk/credentials.enc`.
#[allow(
    dead_code,
    reason = "shared helpers are compiled separately by each integration test crate"
)]
pub fn now_sdk_credentials_dir(keychain_store: &std::path::Path) -> PathBuf {
    keychain_store.with_file_name("now-sdk")
}
