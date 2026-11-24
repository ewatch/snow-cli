#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Integration tests for the encrypted credential vault (`~/.servicenow/credentials.enc`)
//! and `profile storage` commands (`status`, `set`, `migrate`).

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use base64::Engine as _;
use predicates::prelude::*;
use serde_json::Value;
use snow_cli::config::vault::vault_key_account_for;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn stdout_json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn default_keyring_mode_stores_only_one_master_key_across_multiple_profiles() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "basic"
username = "admin"

[profiles.prod]
instance = "https://prod.service-now.com"
auth_method = "api_key"
"#,
    );
    let (_kc_dir, keychain_store) = common::create_temp_keychain_store();
    let key_account = vault_key_account_for(config_path.parent().unwrap());

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args([
            "auth",
            "login",
            "--profile",
            "dev",
            "--password",
            "dev-secret",
            "--no-verify",
        ])
        .assert()
        .success();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args([
            "auth",
            "login",
            "--profile",
            "prod",
            "--token",
            "prod-token",
            "--no-verify",
        ])
        .assert()
        .success();

    // Keychain holds ONLY the single directory-scoped master key, not per-profile secrets.
    assert!(common::read_test_keychain_entry(&keychain_store, "snow-cli", &key_account).is_some());
    assert!(
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "dev:password").is_none()
    );
    assert!(
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "prod:api_token").is_none()
    );

    // Encrypted vault exists beside config.toml and contains no plaintext secrets.
    let vault_path = config_path.parent().unwrap().join("credentials.enc");
    let raw_vault = std::fs::read(&vault_path).unwrap();
    let lossy = String::from_utf8_lossy(&raw_vault);
    assert!(!lossy.contains("dev-secret"));
    assert!(!lossy.contains("prod-token"));

    // Both secrets decrypt cleanly from the single vault.
    assert_eq!(
        common::read_stored_credential(&config_path, &keychain_store, "dev", "password").as_deref(),
        Some("dev-secret")
    );
    assert_eq!(
        common::read_stored_credential(&config_path, &keychain_store, "prod", "api_token")
            .as_deref(),
        Some("prod-token")
    );
}

#[test]
fn legacy_keychain_entries_work_via_fallback_and_migrate_cleanly() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "basic"
username = "admin"

[profiles.prod]
instance = "https://prod.service-now.com"
auth_method = "basic"
username = "admin"
"#,
    );
    let (_kc_dir, keychain_store) = common::create_temp_keychain_store();
    common::write_test_keychain_entry(&keychain_store, "snow-cli", "dev:password", "legacy-pw");
    common::write_test_keychain_entry(&keychain_store, "snow-cli", "prod:password", "stale-prod");

    // Pre-populate prod in the vault so migration skips it (and must NOT delete its legacy entry).
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args([
            "auth",
            "login",
            "--profile",
            "prod",
            "--password",
            "newer-prod",
            "--no-verify",
        ])
        .assert()
        .success();

    // Pre-migration: `auth status` transparently sees dev's legacy keychain entry.
    let status = cargo_bin_cmd!("snow-cli-ro")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["auth", "status", "--profile", "dev"])
        .assert()
        .success();
    assert_eq!(
        stdout_json(status.get_output())["credentials_present"],
        true
    );

    // Migrate and delete migrated legacy per-profile entries.
    let migrate = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["profile", "storage", "migrate", "--delete-legacy"])
        .assert()
        .success();
    let report = stdout_json(migrate.get_output());
    assert_eq!(report["status"], "migrated");
    assert_eq!(report["migrated"], serde_json::json!(["dev:password"]));
    assert_eq!(report["skipped"], serde_json::json!(["prod:password"]));
    assert_eq!(
        report["deleted_legacy"],
        serde_json::json!(["dev:password"])
    );

    // Migrated legacy entry is deleted; skipped entry is preserved untouched.
    assert!(
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "dev:password").is_none()
    );
    assert_eq!(
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "prod:password").as_deref(),
        Some("stale-prod")
    );
    assert_eq!(
        common::read_stored_credential(&config_path, &keychain_store, "dev", "password").as_deref(),
        Some("legacy-pw")
    );
    assert_eq!(
        common::read_stored_credential(&config_path, &keychain_store, "prod", "password")
            .as_deref(),
        Some("newer-prod")
    );
}

#[tokio::test]
async fn switching_to_file_mode_rekeys_and_never_touches_os_keychain() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_user"))
        .and(header("Authorization", "Bearer zero-popup-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"sys_id": "6816f79cc0a8016401c5a33be04be441", "user_name": "admin"}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = common::create_temp_config(&format!(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "{}"
auth_method = "api_key"
"#,
        server.uri()
    ));
    let (_kc_dir, keychain_store) = common::create_temp_keychain_store();
    let key_account = vault_key_account_for(config_path.parent().unwrap());

    // 1. Login in default `keyring` mode.
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args([
            "auth",
            "login",
            "--token",
            "zero-popup-token",
            "--no-verify",
        ])
        .assert()
        .success();
    assert!(common::read_test_keychain_entry(&keychain_store, "snow-cli", &key_account).is_some());

    // 2. Switch to `file` mode: moves master key to credentials.key and deletes keychain key.
    let switched = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["profile", "storage", "set", "file"])
        .assert()
        .success();
    let report = stdout_json(switched.get_output());
    assert_eq!(report["previous_mode"], "keyring");
    assert_eq!(report["mode"], "file");
    assert_eq!(report["key_migrated"], true);
    assert_eq!(report["vault_rekeyed"], false);
    assert_eq!(report["old_key_removed"], true);
    assert!(common::read_test_keychain_entry(&keychain_store, "snow-cli", &key_account).is_none());
    assert!(
        config_path
            .parent()
            .unwrap()
            .join("credentials.key")
            .exists()
    );

    // 3. Prove ZERO OS keychain calls in `file` mode:
    // Setting SNOW_CLI_TEST_KEYCHAIN_STORE without ALLOW_PLAINTEXT_TEST_KEYCHAIN
    // causes any keychain access to fail loudly.
    let verified = cargo_bin_cmd!("snow-cli-ro")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env_remove("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN")
        .env_remove("SNOW_CLI_API_TOKEN")
        .args(["auth", "status", "--verify"])
        .assert()
        .success();
    let verified_json = stdout_json(verified.get_output());
    assert_eq!(verified_json["credentials_present"], true);
    assert_eq!(verified_json["verified"], true);
    assert_eq!(verified_json["verified_user"], "admin");

    // 4. Logout in `file` mode also makes zero keychain calls.
    cargo_bin_cmd!("snow-cli-ro")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env_remove("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN")
        .args(["auth", "logout"])
        .assert()
        .success();

    // 5. Switch back to `keyring` mode: removes credentials.key.
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["profile", "storage", "set", "keyring"])
        .assert()
        .success();
    assert!(
        !config_path
            .parent()
            .unwrap()
            .join("credentials.key")
            .exists()
    );
    assert!(common::read_test_keychain_entry(&keychain_store, "snow-cli", &key_account).is_some());
}

#[test]
fn switching_to_legacy_keychain_exports_entries_and_round_trips_cleanly() {
    // Regression test for H1: switching keyring -> legacy-keychain -> keyring must
    // preserve credentials and never leave an unreadable orphaned vault.
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "basic"
username = "admin"
"#,
    );
    let (_kc_dir, keychain_store) = common::create_temp_keychain_store();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["auth", "login", "--password", "keep-secret", "--no-verify"])
        .assert()
        .success();

    let to_legacy = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["profile", "storage", "set", "legacy-keychain"])
        .assert()
        .success();
    let report = stdout_json(to_legacy.get_output());
    assert_eq!(
        report["exported_to_legacy"],
        serde_json::json!(["dev:password"])
    );
    assert_eq!(
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "dev:password").as_deref(),
        Some("keep-secret")
    );
    assert!(
        !config_path
            .parent()
            .unwrap()
            .join("credentials.enc")
            .exists(),
        "vault file is removed once exported so switching back cannot hit an orphaned vault"
    );

    // Credentials remain usable in legacy-keychain mode.
    let st = cargo_bin_cmd!("snow-cli-ro")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["auth", "status"])
        .assert()
        .success();
    assert_eq!(stdout_json(st.get_output())["credentials_present"], true);

    // Switching back to keyring succeeds and subsequent login works cleanly.
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["profile", "storage", "set", "keyring"])
        .assert()
        .success();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["auth", "login", "--password", "new-secret", "--no-verify"])
        .assert()
        .success();
    assert_eq!(
        common::read_stored_credential(&config_path, &keychain_store, "dev", "password").as_deref(),
        Some("new-secret")
    );
}

#[test]
fn separate_config_directories_do_not_clobber_each_others_keychain_keys() {
    // Regression test for H2: switching storage mode in config dir B must never
    // delete or overwrite the master key used by config dir A.
    let (_dir_a, config_a) = common::create_temp_config(
        r#"
default_profile = "dev"
[profiles.dev]
instance = "https://a.service-now.com"
auth_method = "api_key"
"#,
    );
    let (_dir_b, config_b) = common::create_temp_config(
        r#"
default_profile = "dev"
[profiles.dev]
instance = "https://b.service-now.com"
auth_method = "api_key"
"#,
    );
    let (_kc_dir, keychain_store) = common::create_temp_keychain_store();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_a)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["auth", "login", "--token", "secret-a", "--no-verify"])
        .assert()
        .success();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_b)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["auth", "login", "--token", "secret-b", "--no-verify"])
        .assert()
        .success();

    // Switch dir B to file mode (which deletes dir B's keychain key).
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_b)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["profile", "storage", "set", "file"])
        .assert()
        .success();

    // Dir A's vault must remain completely intact and readable.
    let st_a = cargo_bin_cmd!("snow-cli-ro")
        .env("SNOW_CLI_CONFIG", &config_a)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["auth", "status"])
        .assert()
        .success();
    assert_eq!(stdout_json(st_a.get_output())["credentials_present"], true);
    assert_eq!(
        common::read_stored_credential(&config_a, &keychain_store, "dev", "api_token").as_deref(),
        Some("secret-a")
    );
}

#[tokio::test]
async fn confidential_oauth_authorization_code_refreshes_across_upgrade_from_legacy_keychain() {
    // Regression test for H3: a confidential authorization-code profile whose
    // legacy keychain holds both `oauth_token` and `client_secret` must keep
    // sending `client_secret` on the 1st AND 2nd refreshes after `oauth_token`
    // is written to the vault.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth_token.do"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains(
            "client_secret=legacy-confidential-secret",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "refreshed-access-token",
            "refresh_token": "refreshed-refresh-token",
            "token_type": "Bearer",
            "expires_in": 1
        })))
        .expect(2)
        .mount(&server)
        .await;

    let (_dir, config_path) = common::create_temp_config(&format!(
        r#"
default_profile = "corp"

[profiles.corp]
instance = "{}"
auth_method = "oauth2"
client_id = "corp-client"
oauth_grant_type = "authorization_code"
"#,
        server.uri()
    ));
    let (_kc_dir, keychain_store) = common::create_temp_keychain_store();

    // Seed legacy per-profile keychain entries with an already-expired access token.
    common::write_test_keychain_entry(
        &keychain_store,
        "snow-cli",
        "corp:client_secret",
        "legacy-confidential-secret",
    );
    common::write_test_keychain_entry(
        &keychain_store,
        "snow-cli",
        "corp:oauth_token",
        r#"{"access_token":"old-token","refresh_token":"rt-1","expires_at":1}"#,
    );

    // First refresh reads both from legacy keychain and writes the new `oauth_token`
    // (with expires_in=1, already within the 30s refresh skew window) into the vault.
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .env_remove("SNOW_CLI_CLIENT_SECRET")
        .env_remove("SNOW_CLI_OAUTH_TOKEN")
        .args(["auth", "token"])
        .assert()
        .success()
        .stdout("refreshed-access-token");

    // Second refresh reads `oauth_token` from the vault AND must STILL find
    // `client_secret` in the legacy keychain fallback!
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .env_remove("SNOW_CLI_CLIENT_SECRET")
        .env_remove("SNOW_CLI_OAUTH_TOKEN")
        .args(["auth", "token"])
        .assert()
        .success()
        .stdout("refreshed-access-token");
}

#[test]
fn storage_set_refuses_when_overridden_by_env_var() {
    // Regression test for M1.
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"
[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "api_key"
"#,
    );
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_CREDENTIAL_STORE", "keyring")
        .args(["profile", "storage", "set", "file"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("CONFIG_INVALID"))
        .stderr(predicate::str::contains("SNOW_CLI_CREDENTIAL_STORE"));
}

#[test]
fn auth_status_surfaces_unreadable_vault_error_instead_of_false() {
    // Regression test for M3.
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"
credential_store = "file"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "api_key"
"#,
    );
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["auth", "login", "--token", "secret", "--no-verify"])
        .assert()
        .success();

    // Remove the key file so `credentials.enc` becomes unreadable.
    std::fs::remove_file(config_path.parent().unwrap().join("credentials.key")).unwrap();

    cargo_bin_cmd!("snow-cli-ro")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env_remove("SNOW_CLI_API_TOKEN")
        .args(["auth", "status"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("CREDENTIALS_UNREADABLE"));
}

#[test]
fn env_mode_works_without_touching_keychain_and_keeps_old_key_on_switch() {
    // Covers env mode + M4 (switching to env keeps previous persistent key).
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "ci"

[profiles.ci]
instance = "https://ci.service-now.com"
auth_method = "api_key"
"#,
    );
    let (_kc_dir, keychain_store) = common::create_temp_keychain_store();
    let key_account = vault_key_account_for(config_path.parent().unwrap());
    let key = base64::engine::general_purpose::STANDARD.encode([42u8; 32]);
    let wrong_key = base64::engine::general_purpose::STANDARD.encode([99u8; 32]);

    // First store in keyring mode, then switch to env mode and verify old keychain key is kept.
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["auth", "login", "--token", "ci-secret", "--no-verify"])
        .assert()
        .success();

    let switched = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .env("SNOW_CLI_ENCRYPTION_KEY", &key)
        .args(["profile", "storage", "set", "env"])
        .assert()
        .success();
    let report = stdout_json(switched.get_output());
    assert_eq!(report["vault_rekeyed"], true);
    assert_eq!(report["old_key_removed"], false);
    assert!(common::read_test_keychain_entry(&keychain_store, "snow-cli", &key_account).is_some());

    // Now in env mode, reading works with poisoned keychain.
    let status = cargo_bin_cmd!("snow-cli-ro")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_ENCRYPTION_KEY", &key)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", "/must/not/be/touched")
        .env_remove("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN")
        .args(["auth", "status"])
        .assert()
        .success();
    assert_eq!(
        stdout_json(status.get_output())["credentials_present"],
        true
    );

    let vault_path = config_path.parent().unwrap().join("credentials.enc");
    let before = std::fs::read(&vault_path).unwrap();

    // Wrong key fails closed with CREDENTIALS_UNREADABLE and never modifies the file.
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_ENCRYPTION_KEY", &wrong_key)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", "/must/not/be/touched")
        .env_remove("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN")
        .args(["auth", "login", "--token", "overwrite", "--no-verify"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("CREDENTIALS_UNREADABLE"))
        .stderr(predicate::str::contains("did not modify"));

    assert_eq!(std::fs::read(&vault_path).unwrap(), before);
}

#[test]
fn read_only_policy_allows_storage_status_and_denies_set_and_migrate() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "https://dev.service-now.com"
auth_method = "api_key"
"#,
    );

    // `profile storage status` succeeds in snow-cli-ro and never touches keychain.
    let status = cargo_bin_cmd!("snow-cli-ro")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", "/must/not/be/touched")
        .env_remove("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN")
        .args(["profile", "storage", "status"])
        .assert()
        .success();
    let json = stdout_json(status.get_output());
    assert_eq!(json["mode"], "keyring");
    assert_eq!(json["source"], "default");
    assert_eq!(json["vault_exists"], false);

    // `profile storage set` and `migrate` are denied with POLICY_DENIED (exit 7).
    cargo_bin_cmd!("snow-cli-ro")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["profile", "storage", "set", "file"])
        .assert()
        .code(7)
        .stderr(predicate::str::contains("POLICY_DENIED"));

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args(["--read-only", "profile", "storage", "migrate"])
        .assert()
        .code(7)
        .stderr(predicate::str::contains("POLICY_DENIED"));
}
