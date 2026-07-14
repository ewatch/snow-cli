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
