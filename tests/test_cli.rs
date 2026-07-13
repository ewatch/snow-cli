#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn test_help_flag() {
    cargo_bin_cmd!("snow-cli")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("ServiceNow"))
        .stdout(predicate::str::contains("❄️ snow-cli"))
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
fn test_profile_show_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["profile", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Manage ServiceNow connection profiles",
        ))
        .stdout(predicate::str::contains("Examples:"))
        .stdout(predicate::str::contains("sdk"));
}

#[test]
fn test_profile_sdk_list_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["profile", "sdk", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("now-sdk"));
}

#[test]
fn test_config_alias_still_works() {
    cargo_bin_cmd!("snow-cli")
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Manage ServiceNow connection profiles",
        ));
}

#[test]
fn test_profile_init_help_mentions_non_interactive() {
    cargo_bin_cmd!("snow-cli")
        .args(["profile", "init", "--help"])
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
fn test_scope_inspect_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["scope", "inspect", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Inspect scope metadata"))
        .stdout(predicate::str::contains("--details"));
}

#[test]
fn test_scope_inventory_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["scope", "inventory", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Export normalized scope artifacts",
        ));
}

#[test]
fn test_scope_list_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["scope", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "List scopes and classify them by origin",
        ))
        .stdout(predicate::str::contains("--kind"))
        .stdout(predicate::str::contains("--show-source-table"))
        .stdout(predicate::str::contains("--show-sys-id"));
}

#[test]
fn test_scope_move_file_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["scope", "move-file", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Move one application file to a different custom scope without changing sys_id",
        ))
        .stdout(predicate::str::contains("--target-scope"))
        .stdout(predicate::str::contains("--dry-run"))
        .stdout(predicate::str::contains("--yes"));
}

#[test]
fn test_data_export_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["data", "export", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Export records from a single table",
        ))
        .stdout(predicate::str::contains("--out"));
}

#[test]
fn test_seed_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["seed", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Declarative test-data"))
        .stdout(predicate::str::contains("seed cleanup"));
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

#[test]
fn test_read_only_denies_mutating_command_before_config_load() {
    cargo_bin_cmd!("snow-cli")
        .args([
            "--read-only",
            "table",
            "update",
            "incident",
            "6816f79cc0a8016401c5a33be04be441",
            "--data",
            r#"{"state":"2"}"#,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("POLICY_DENIED"))
        .stderr(predicate::str::contains("table mutations"));
}

#[test]
fn test_snow_cli_ro_help_omits_mutating_commands() {
    cargo_bin_cmd!("snow-cli-ro")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("snow-cli-ro"))
        .stdout(predicate::str::contains("Raw REST API GET"))
        .stdout(predicate::str::contains("codesearch"))
        .stdout(predicate::str::contains("Script").not())
        .stdout(predicate::str::contains("ImportSet").not());
}

#[test]
fn test_snow_cli_ro_rejects_mutating_command_at_parse_time() {
    cargo_bin_cmd!("snow-cli-ro")
        .args(["table", "delete", "incident", "abc123", "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand 'delete'"));
}

#[test]
fn test_snow_cli_ro_rejects_api_post_at_parse_time() {
    cargo_bin_cmd!("snow-cli-ro")
        .args(["api", "post", "/api/x/action", "--data", "{}"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand 'post'"));
}

#[test]
fn test_snow_cli_ro_exposes_snu_read_commands() {
    cargo_bin_cmd!("snow-cli-ro")
        .args(["snu", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("check-connection"))
        .stdout(predicate::str::contains("query"))
        .stdout(predicate::str::contains("screenshot"))
        .stdout(predicate::str::contains("update-record").not())
        .stdout(predicate::str::contains("execute-bg-script").not());
}

#[test]
fn test_snow_cli_ro_rejects_snu_update_record_at_parse_time() {
    cargo_bin_cmd!("snow-cli-ro")
        .args([
            "snu",
            "update-record",
            "incident",
            "abc123",
            "--field",
            "state",
            "--content",
            "2",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unrecognized subcommand 'update-record'",
        ));
}

#[test]
fn test_read_only_api_get_denies_method_override_header() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "basic"
username = "admin"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args([
            "--read-only",
            "api",
            "get",
            "/api/x_myapp/action",
            "-H",
            "X-HTTP-Method-Override: POST",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("POLICY_DENIED"))
        .stderr(predicate::str::contains("method override"));
}

#[tokio::test]
async fn test_read_only_allows_api_get() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/x_myapp/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .mount(&server)
        .await;

    let config = format!(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "{}"
auth_method = "basic"
username = "admin"
"#,
        server.uri()
    );
    let (_dir, config_path) = common::create_temp_config(&config);
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();
    common::write_test_keychain_entry(&keychain_store, "snow-cli", "dev:password", "secret");

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["--read-only", "api", "get", "/api/x_myapp/status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\": true"));
}

// --- Config integration tests ---

#[test]
fn test_config_init_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join(".servicenow").join("config.toml");

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args([
            "profile",
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
        .args(["profile", "init"])
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
            "profile",
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
            "profile",
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
        .args(["profile", "show"])
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
        .args(["--output", "csv", "profile", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("key,value"))
        .stdout(predicate::str::contains("dev.service-now.com"));
}

#[test]
fn test_profile_current_shows_active_profile() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "basic"
username = "admin"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["profile", "current"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"active_profile\": \"dev\""))
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
        .args(["profile", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("dev"))
        .stdout(predicate::str::contains("prod"));
}

#[test]
fn test_config_find_profile_by_short_instance_name() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev123466.service-now.com"
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
        .args(["profile", "find", "--instance", "dev123466"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\":\"dev\""))
        .stdout(predicate::str::contains("dev123466.service-now.com"))
        .stdout(predicate::str::contains("prod").not())
        .stdout(predicate::str::contains("admin").not());
}

#[test]
fn test_config_find_profile_by_instance_url() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev123466.service-now.com"
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
        .args([
            "profile",
            "find",
            "--instance",
            "https://dev123466.service-now.com/nav_to.do",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\":\"dev\""))
        .stdout(predicate::str::contains("\"default\":true"));
}

#[test]
fn test_config_find_profile_no_match_errors() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev123466.service-now.com"
auth_method = "basic"
username = "admin"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["profile", "find", "--instance", "prod123466"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No profile found for instance"));
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
            "profile",
            "add",
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
        .args(["profile", "edit", "dev", "--username", "newuser"])
        .assert()
        .success();

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("newuser"));
    // Instance should be preserved
    assert!(content.contains("dev.service-now.com"));
}

#[test]
fn test_profile_add_fails_when_profile_exists() {
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
            "profile",
            "add",
            "dev",
            "--instance",
            "https://other.service-now.com",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn test_profile_edit_fails_when_profile_missing() {
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
        .args(["profile", "edit", "missing", "--username", "admin"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("profile add missing"));
}

#[test]
fn test_config_set_profile_legacy_upsert_alias_still_works() {
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
        .args(["profile", "set", "dev", "--username", "legacy-user"])
        .assert()
        .success();

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("legacy-user"));
}

#[test]
fn test_config_set_profile_stores_sso_login_url() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "saml"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args([
            "profile",
            "edit",
            "dev",
            "--sso-login-url",
            "https://dev.service-now.com/login_with_sso.do",
        ])
        .assert()
        .success();

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("sso_login_url = \"https://dev.service-now.com/login_with_sso.do\""));
}

#[test]
fn test_config_set_profile_stores_oauth_authorization_code_options() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "oauth2"
client_id = "old-client"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args([
            "profile",
            "edit",
            "dev",
            "--oauth-grant-type",
            "authorization-code",
            "--client-id",
            "new-client",
            "--oauth-scope",
            "useraccount email",
            "--oauth-redirect-host",
            "localhost",
            "--oauth-redirect-port",
            "8484",
            "--oauth-redirect-path",
            "/oauth/callback",
        ])
        .assert()
        .success();

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("client_id = \"new-client\""));
    assert!(content.contains("oauth_grant_type = \"authorization_code\""));
    assert!(content.contains("oauth_scope = \"useraccount email\""));
    assert!(content.contains("oauth_redirect_host = \"localhost\""));
    assert!(content.contains("oauth_redirect_port = 8484"));
    assert!(content.contains("oauth_redirect_path = \"/oauth/callback\""));
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
        .args(["profile", "default", "prod"])
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
        .args(["profile", "default", "nonexistent"])
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
        .args(["profile", "default", "de"])
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
        .args(["profile", "remove", "prod"])
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
        .args(["profile", "remove", "dev"])
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
        .args(["--profile", "prod", "profile", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("prod.service-now.com"));
}

#[test]
fn test_config_list_now_sdk_profiles() {
    let (_dir, keychain_store) = common::create_temp_keychain_store();
    let now_sdk_blob = json!({
        "dev": {
            "alias": "dev",
            "isDefault": true,
            "creds": {
                "type": "basic",
                "instanceUrl": "https://dev.service-now.com",
                "username": "admin",
                "password": "secret"
            }
        },
        "prod": {
            "alias": "prod",
            "isDefault": false,
            "creds": {
                "type": "oauth",
                "instanceUrl": "https://prod.service-now.com",
                "access_token": "token",
                "token_type": "Bearer",
                "refresh_token": "refresh",
                "expires_at": 1700000000
            }
        }
    });
    common::write_test_keychain_entry(
        &keychain_store,
        "ServiceNow",
        "now-sdk",
        &now_sdk_blob.to_string(),
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["profile", "sdk", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"alias\":\"dev\""))
        .stdout(predicate::str::contains("\"supported\":true"))
        .stdout(predicate::str::contains("\"alias\":\"prod\""))
        .stdout(predicate::str::contains("\"supported\":false"));
}

#[test]
fn test_config_import_now_sdk_alias_creates_profile_and_password() {
    let config_dir = tempfile::tempdir().unwrap();
    let config_path = config_dir.path().join("config.toml");
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();
    let now_sdk_blob = json!({
        "dev": {
            "alias": "dev",
            "isDefault": true,
            "creds": {
                "type": "basic",
                "instanceUrl": "https://dev.service-now.com",
                "username": "admin",
                "password": "secret"
            }
        }
    });
    common::write_test_keychain_entry(
        &keychain_store,
        "ServiceNow",
        "now-sdk",
        &now_sdk_blob.to_string(),
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args([
            "profile",
            "sdk",
            "import",
            "--alias",
            "dev",
            "--set-default",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"imported_count\":1"))
        .stdout(predicate::str::contains("\"default_profile\":\"dev\""));

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("default_profile = \"dev\""));
    assert!(content.contains("instance = \"https://dev.service-now.com\""));
    assert!(content.contains("username = \"admin\""));

    let stored_password =
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "dev:password").unwrap();
    assert_eq!(stored_password, "secret");
}

#[test]
fn test_config_import_now_sdk_overwrites_existing_profile() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://old.service-now.com"
auth_method = "basic"
username = "olduser"
"#,
    );
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();
    common::write_test_keychain_entry(&keychain_store, "snow-cli", "dev:password", "old-secret");
    let now_sdk_blob = json!({
        "dev": {
            "alias": "dev",
            "isDefault": false,
            "creds": {
                "type": "basic",
                "instanceUrl": "https://new.service-now.com",
                "username": "newuser",
                "password": "new-secret"
            }
        }
    });
    common::write_test_keychain_entry(
        &keychain_store,
        "ServiceNow",
        "now-sdk",
        &now_sdk_blob.to_string(),
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["profile", "sdk", "import", "--alias", "dev"])
        .assert()
        .success();

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("https://new.service-now.com"));
    assert!(content.contains("newuser"));
    assert_eq!(
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "dev:password").unwrap(),
        "new-secret"
    );
}

#[test]
fn test_config_import_now_sdk_all_fails_when_oauth_present() {
    let config_dir = tempfile::tempdir().unwrap();
    let config_path = config_dir.path().join("config.toml");
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();
    let now_sdk_blob = json!({
        "dev": {
            "alias": "dev",
            "isDefault": true,
            "creds": {
                "type": "basic",
                "instanceUrl": "https://dev.service-now.com",
                "username": "admin",
                "password": "secret"
            }
        },
        "prod": {
            "alias": "prod",
            "isDefault": false,
            "creds": {
                "type": "oauth",
                "instanceUrl": "https://prod.service-now.com",
                "access_token": "token",
                "token_type": "Bearer",
                "refresh_token": "refresh",
                "expires_at": 1700000000
            }
        }
    });
    common::write_test_keychain_entry(
        &keychain_store,
        "ServiceNow",
        "now-sdk",
        &now_sdk_blob.to_string(),
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["profile", "sdk", "import", "--all"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unsupported auth type 'oauth'"));

    assert!(!config_path.exists());
    assert!(
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "dev:password").is_none()
    );
}

#[test]
fn test_config_import_now_sdk_all_preserves_now_sdk_default_alias() {
    let config_dir = tempfile::tempdir().unwrap();
    let config_path = config_dir.path().join("config.toml");
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();
    let now_sdk_blob = json!({
        "dev": {
            "alias": "dev",
            "isDefault": false,
            "creds": {
                "type": "basic",
                "instanceUrl": "https://dev.service-now.com",
                "username": "admin",
                "password": "secret"
            }
        },
        "prod": {
            "alias": "prod",
            "isDefault": true,
            "creds": {
                "type": "basic",
                "instanceUrl": "https://prod.service-now.com",
                "username": "admin",
                "password": "secret"
            }
        }
    });
    common::write_test_keychain_entry(
        &keychain_store,
        "ServiceNow",
        "now-sdk",
        &now_sdk_blob.to_string(),
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["profile", "sdk", "import", "--all"])
        .assert()
        .success();

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("default_profile = \"prod\""));
}

#[test]
fn test_config_export_now_sdk_writes_alias() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "basic"
username = "admin"
"#,
    );
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();
    common::write_test_keychain_entry(&keychain_store, "snow-cli", "dev:password", "secret");

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args([
            "profile",
            "sdk",
            "export",
            "dev",
            "--alias",
            "sdk-dev",
            "--set-default",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"alias\":\"sdk-dev\""));

    let raw = common::read_test_keychain_entry(&keychain_store, "ServiceNow", "now-sdk").unwrap();
    let blob: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(blob["sdk-dev"]["creds"]["type"], "basic");
    assert_eq!(blob["sdk-dev"]["creds"]["username"], "admin");
    assert_eq!(blob["sdk-dev"]["creds"]["password"], "secret");
    assert_eq!(blob["sdk-dev"]["isDefault"], true);
}

#[test]
fn test_auth_login_also_now_sdk_dual_writes_basic_password() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "basic"
username = "admin"
"#,
    );
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args([
            "auth",
            "login",
            "--password",
            "secret",
            "--also-now-sdk",
            "--now-sdk-alias",
            "sdk-dev",
            "--set-now-sdk-default",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"profile\":\"dev\""))
        .stdout(predicate::str::contains("\"alias\":\"sdk-dev\""));

    assert_eq!(
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "dev:password").unwrap(),
        "secret"
    );

    let raw = common::read_test_keychain_entry(&keychain_store, "ServiceNow", "now-sdk").unwrap();
    let blob: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(blob["sdk-dev"]["creds"]["password"], "secret");
    assert_eq!(blob["sdk-dev"]["isDefault"], true);
}

#[test]
fn test_auth_login_token_stdin_stores_api_token() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "default"

[profiles.default]
instance = "https://dev.service-now.com"
auth_method = "api_key"
"#,
    );
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["auth", "login", "--token-stdin"])
        .write_stdin("stdin-token\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("api_token"));

    assert_eq!(
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "default:api_token").unwrap(),
        "stdin-token"
    );
}

#[test]
fn test_auth_login_also_now_sdk_rejects_non_basic_profiles() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "default"

[profiles.default]
instance = "https://dev.service-now.com"
auth_method = "api_key"
"#,
    );
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["auth", "login", "--token", "abc123", "--also-now-sdk"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "only supported for basic auth profiles",
        ));
}

#[test]
fn test_plaintext_test_keychain_requires_explicit_unsafe_opt_in() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "default"

[profiles.default]
instance = "https://dev.service-now.com"
auth_method = "api_key"
"#,
    );
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .args(["auth", "login", "--token", "test-api-token"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN=1",
        ));
}

#[tokio::test]
async fn test_auth_token_oauth_client_credentials_prints_access_token_not_client_secret() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth_token.do"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "short-lived-access-token",
            "token_type": "Bearer",
            "expires_in": 3600
        })))
        .expect(1)
        .mount(&server)
        .await;

    let config = format!(
        r#"
default_profile = "default"

[profiles.default]
instance = "{}"
auth_method = "oauth2"
client_id = "client-id"
oauth_grant_type = "client_credentials"
"#,
        server.uri()
    );
    let (_dir, config_path) = common::create_temp_config(&config);
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();
    common::write_test_keychain_entry(
        &keychain_store,
        "snow-cli",
        "default:client_secret",
        "super-secret-client-secret",
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["auth", "token"])
        .assert()
        .success()
        .stdout(predicate::str::contains("short-lived-access-token"))
        .stdout(predicate::str::contains("super-secret-client-secret").not());
}

#[test]
fn test_auth_login_browser_session_validates_cookie_and_prints_export_hint() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "default"

[profiles.default]
instance = "https://dev.service-now.com"
auth_method = "browser_session"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args([
            "auth",
            "login",
            "--session-cookie",
            "JSESSIONID=session123; glide_user_route=route456",
        ])
        .assert()
        .success()
        // Token is NOT stored — output shows export hint instead
        .stdout(predicate::str::contains("\"stored\":false"))
        .stdout(predicate::str::contains("export_hint"))
        .stdout(predicate::str::contains("SNOW_SESSION_COOKIE"))
        .stdout(predicate::str::contains(
            "\"credential_type\":\"session_cookie\"",
        ));
}

#[test]
fn test_auth_login_browser_session_accepts_legacy_saml_method_name() {
    // Old profiles with auth_method = "saml" must still be loadable (backward compat)
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "default"

[profiles.default]
instance = "https://dev.service-now.com"
auth_method = "saml"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args([
            "auth",
            "login",
            "--session-cookie",
            "JSESSIONID=session123; glide_user_route=route456",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"stored\":false"))
        .stdout(predicate::str::contains("SNOW_SESSION_COOKIE"));
}

#[test]
fn test_auth_login_browser_session_rejects_cookie_without_jsessionid() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "default"

[profiles.default]
instance = "https://dev.service-now.com"
auth_method = "browser_session"
"#,
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args([
            "auth",
            "login",
            "--session-cookie",
            "glide_user_route=route456",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("JSESSIONID"));
}

#[test]
fn test_auth_login_help_mentions_browser_session_export_hint() {
    cargo_bin_cmd!("snow-cli")
        .args(["auth", "login", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("SNOW_SESSION_COOKIE"))
        .stdout(predicate::str::contains("browser-session"));
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
        .stderr(predicate::str::contains("profile add"));
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
        .args([
            "table",
            "delete",
            "incident",
            "6816f79cc0a8016401c5a33be04be441",
        ])
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
        .args([
            "attachment",
            "list",
            "incident",
            "6816f79cc0a8016401c5a33be04be441",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No API token found"));
}

#[test]
fn test_import_set_load_fails_gracefully_not_panic() {
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
        .args([
            "import-set",
            "load",
            "u_import_table",
            "--data",
            r#"{"name":"x"}"#,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No API token found"))
        .stderr(predicate::str::contains("panicked").not());
}

#[test]
fn test_import_set_transform_fails_gracefully_not_panic() {
    // Use an isolated config with a default profile so the command reaches the
    // "not implemented yet" branch deterministically. Without this, a clean
    // environment (e.g. CI) fails earlier at profile resolution with a
    // different error, and the assertions below never match.
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
        .args([
            "import-set",
            "transform",
            "6816f79cc0a8016401c5a33be04be441",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("import-set transform"))
        .stderr(predicate::str::contains("not implemented yet"))
        .stderr(predicate::str::contains("panicked").not());
}
