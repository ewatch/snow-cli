mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn test_help_flag() {
    cargo_bin_cmd!("snow-cli")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("ServiceNow"));
}

#[test]
fn test_version_flag() {
    cargo_bin_cmd!("snow-cli")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("snow-cli"));
}

#[test]
fn test_no_args_shows_help() {
    cargo_bin_cmd!("snow-cli")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage:"));
}

#[test]
fn test_config_show_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Manage configuration"));
}

#[test]
fn test_table_list_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["table", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("List records"));
}

#[test]
fn test_incident_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["incident", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Incident"));
}

#[test]
fn test_completions_bash() {
    cargo_bin_cmd!("snow-cli")
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("snow-cli"));
}

#[test]
fn test_invalid_subcommand() {
    cargo_bin_cmd!("snow-cli")
        .arg("nonexistent")
        .assert()
        .failure();
}

// --- Config integration tests ---

#[test]
fn test_config_init_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join(".servicenow").join("config.toml");

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args([
            "config",
            "init",
            "--instance",
            "https://test.service-now.com",
            "--username",
            "admin",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\":\"created\""));

    // Verify the file was created with correct content
    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("https://test.service-now.com"));
    assert!(content.contains("admin"));
    assert!(content.contains("basic"));
}

#[test]
fn test_config_init_fails_without_instance() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["config", "init"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Instance URL is required"));
}

#[test]
fn test_config_init_fails_if_exists() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://existing.service-now.com"
auth_method = "basic"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args([
            "config",
            "init",
            "--instance",
            "https://test.service-now.com",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn test_config_init_with_custom_profile_name() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args([
            "config",
            "init",
            "--instance",
            "https://dev.service-now.com",
            "--name",
            "mydev",
            "--auth-method",
            "oauth2",
        ])
        .assert()
        .success();

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("mydev"));
    assert!(content.contains("oauth2"));
}

#[test]
fn test_config_show_json_output() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "default"

[profiles.default]
instance = "https://dev.service-now.com"
auth_method = "basic"
username = "admin"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("config_path"))
        .stdout(predicate::str::contains("dev.service-now.com"));
}

#[test]
fn test_config_show_csv_output() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "default"

[profiles.default]
instance = "https://dev.service-now.com"
auth_method = "basic"
username = "admin"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["--output", "csv", "config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("key,value"))
        .stdout(predicate::str::contains("dev.service-now.com"));
}

#[test]
fn test_config_list_profiles() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "basic"
username = "admin"

[profiles.prod]
instance = "https://prod.service-now.com"
auth_method = "oauth2"
client_id = "abc123"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["config", "list-profiles"])
        .assert()
        .success()
        .stdout(predicate::str::contains("dev"))
        .stdout(predicate::str::contains("prod"));
}

#[test]
fn test_config_set_profile_creates_new() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "basic"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args([
            "config",
            "set-profile",
            "staging",
            "--instance",
            "https://staging.service-now.com",
            "--auth-method",
            "basic",
            "--username",
            "staginguser",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("staging"));

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("staging.service-now.com"));
    assert!(content.contains("staginguser"));
}

#[test]
fn test_config_set_profile_updates_existing() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "basic"
username = "olduser"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["config", "set-profile", "dev", "--username", "newuser"])
        .assert()
        .success();

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("newuser"));
    // Instance should be preserved
    assert!(content.contains("dev.service-now.com"));
}

#[test]
fn test_config_use_profile() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "basic"

[profiles.prod]
instance = "https://prod.service-now.com"
auth_method = "oauth2"
client_id = "abc"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["config", "use-profile", "prod"])
        .assert()
        .success()
        .stdout(predicate::str::contains("prod"));

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("default_profile = \"prod\""));
}

#[test]
fn test_config_use_profile_nonexistent() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "basic"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["config", "use-profile", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_config_profile_flag_overrides_default() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "basic"
username = "devuser"

[profiles.prod]
instance = "https://prod.service-now.com"
auth_method = "oauth2"
client_id = "abc"
"#,
    );

    // Using --profile should show the prod profile info
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["--profile", "prod", "config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("prod.service-now.com"));
}
