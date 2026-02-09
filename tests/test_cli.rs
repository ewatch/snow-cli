mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn test_help_flag() {
    cargo_bin_cmd!("snow-cli")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("ServiceNow"))
        .stdout(predicate::str::contains("Common workflows"));
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
        .stdout(predicate::str::contains("Manage configuration"))
        .stdout(predicate::str::contains("Examples:"));
}

#[test]
fn test_config_init_help_mentions_non_interactive() {
    cargo_bin_cmd!("snow-cli")
        .args(["config", "init", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("non-interactive by default"));
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
fn test_config_use_profile_nonexistent_suggests_similar_name() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "basic"

[profiles.prod]
instance = "https://prod.service-now.com"
auth_method = "basic"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["config", "use-profile", "de"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Maybe you meant 'dev'"));
}

#[test]
fn test_config_delete_profile() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "basic"

[profiles.prod]
instance = "https://prod.service-now.com"
auth_method = "api_key"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["config", "delete-profile", "prod"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\":\"deleted\""));

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(!content.contains("[profiles.prod]"));
    assert!(content.contains("default_profile = \"dev\""));
}

#[test]
fn test_config_delete_default_profile_requires_yes() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "basic"

[profiles.prod]
instance = "https://prod.service-now.com"
auth_method = "api_key"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["config", "delete-profile", "dev"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("current default"));
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

// --- Table command integration tests ---

#[test]
fn test_table_get_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["table", "get", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("single record"))
        .stdout(predicate::str::contains("sys_id"));
}

#[test]
fn test_table_create_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["table", "create", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Create"));
}

#[test]
fn test_table_update_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["table", "update", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Update"));
}

#[test]
fn test_table_delete_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["table", "delete", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Delete"));
}

#[test]
fn test_table_list_missing_profile() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    // Empty config — no profiles
    std::fs::write(&config_path, "default_profile = \"default\"\n[profiles]\n").unwrap();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["table", "list", "incident"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No profiles are configured yet"))
        .stderr(predicate::str::contains("config init"));
}

#[test]
fn test_table_list_requires_table_name() {
    cargo_bin_cmd!("snow-cli")
        .args(["table", "list"])
        .assert()
        .failure();
}

#[test]
fn test_table_get_requires_sys_id() {
    cargo_bin_cmd!("snow-cli")
        .args(["table", "get", "incident"])
        .assert()
        .failure();
}

#[test]
fn test_table_delete_requires_sys_id() {
    cargo_bin_cmd!("snow-cli")
        .args(["table", "delete", "incident"])
        .assert()
        .failure();
}

#[test]
fn test_table_create_no_data_no_stdin_fails() {
    // Create a config with a basic auth profile (which will fail at auth,
    // but the --data/stdin check happens before the HTTP request).
    // Actually, build_client will fail because basic auth tries keychain.
    // But read_data check happens first in the handler — let's verify.
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "default"

[profiles.default]
instance = "https://test.service-now.com"
auth_method = "api_key"
"#,
    );

    // No --data and stdin is not a pipe => should fail
    // In test environment, stdin may not be a TTY, so we accept either error message
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["table", "create", "incident"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No data"));
}

#[test]
fn test_table_delete_no_yes_non_tty_fails() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "default"

[profiles.default]
instance = "https://test.service-now.com"
auth_method = "api_key"
"#,
    );

    // Without --yes, and stdin piped (non-TTY), should fail with confirmation error
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["table", "delete", "incident", "abc123"])
        .pipe_stdin("tests/common/mod.rs") // pipe something to make stdin non-TTY
        .unwrap()
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));
}

#[test]
fn test_attachment_commands_fail_gracefully_not_panic() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "default"

[profiles.default]
instance = "https://test.service-now.com"
auth_method = "api_key"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["attachment", "list", "incident", "abc123"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not implemented yet"));
}
