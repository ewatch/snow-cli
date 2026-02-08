/// Shared test helpers for integration tests.
///
/// Provides utilities for setting up mock ServiceNow servers,
/// creating temporary config files, and building CLI commands.
use std::path::PathBuf;

/// Create a temporary config file with the given content.
/// Returns the path to the temp directory (which contains config.toml).
#[allow(dead_code)]
pub fn create_temp_config(content: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    std::fs::write(&config_path, content).unwrap();
    (dir, config_path)
}
