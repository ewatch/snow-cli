#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! now-sdk 4.14+ encrypted credential store interop.
//!
//! `tests/fixtures/now-sdk/credentials-4.14.2.enc` was written by now-sdk
//! 4.14.2's own `credential-store.write()` with fake credentials, under the
//! base64 key in `FIXTURE_KEY` (32 bytes: "snow-cli-now-sdk-fixture-key-32b").

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::path::{Path, PathBuf};

const FIXTURE: &str = "tests/fixtures/now-sdk/credentials-4.14.2.enc";
const FIXTURE_KEY: &str = "c25vdy1jbGktbm93LXNkay1maXh0dXJlLWtleS0zMmI=";

struct Env {
    _dir: tempfile::TempDir,
    config: PathBuf,
    keychain: PathBuf,
    now_sdk_dir: PathBuf,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        let keychain = dir.path().join("keychain.json");
        let now_sdk_dir = dir.path().join("now-sdk");
        Self {
            _dir: dir,
            config,
            keychain,
            now_sdk_dir,
        }
    }

    fn with_fixture(self) -> Self {
        std::fs::create_dir_all(&self.now_sdk_dir).unwrap();
        std::fs::copy(FIXTURE, self.file()).unwrap();
        self
    }

    fn file(&self) -> PathBuf {
        self.now_sdk_dir.join("credentials.enc")
    }

    fn cmd(&self) -> assert_cmd::Command {
        let mut cmd = cargo_bin_cmd!("snow-cli");
        cmd.env("SNOW_CLI_CONFIG", &self.config)
            .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &self.keychain)
            .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
            .env("SN_SDK_CREDENTIALS_DIR", &self.now_sdk_dir)
            .env_remove("SN_SDK_KEY_SOURCE")
            .env_remove("SN_SDK_ENCRYPTION_KEY");
        cmd
    }

    fn env_key_cmd(&self) -> assert_cmd::Command {
        let mut cmd = self.cmd();
        cmd.env("SN_SDK_KEY_SOURCE", "env")
            .env("SN_SDK_ENCRYPTION_KEY", FIXTURE_KEY);
        cmd
    }
}

fn listed(output: &[u8]) -> Vec<serde_json::Value> {
    serde_json::from_slice(output).unwrap()
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[test]
fn reads_a_file_written_by_now_sdk_with_an_env_key() {
    let env = Env::new().with_fixture();
    let output = env
        .env_key_cmd()
        .args(["--output", "json", "profile", "sdk", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let profiles = listed(&output);
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0]["alias"], "fixture-basic");
    assert_eq!(profiles[0]["username"], "fixture.user");
    assert_eq!(profiles[0]["is_default"], true);
    assert_eq!(profiles[1]["alias"], "fixture-oauth");
    assert_eq!(profiles[1]["supported"], false);
}

#[test]
fn reads_with_the_base64_keychain_key_now_sdk_stores() {
    let env = Env::new().with_fixture();
    common::write_test_keychain_entry(
        &env.keychain,
        "ServiceNow",
        "now-sdk-credentials-key",
        FIXTURE_KEY,
    );
    env.cmd()
        .args(["profile", "sdk", "import", "--alias", "fixture-basic"])
        .assert()
        .success();
    assert_eq!(
        common::read_stored_credential(&env.config, &env.keychain, "fixture-basic", "password")
            .as_deref(),
        Some("fixture-password")
    );
}

#[test]
fn export_writes_the_encrypted_file_and_leaves_the_legacy_blob_alone() {
    let env = Env::new().with_fixture();
    let legacy = r#"{"old":{"isDefault":true,"alias":"old","creds":{"type":"basic","instanceUrl":"https://old.service-now.com","username":"u","password":"p"}}}"#;
    common::write_test_keychain_entry(&env.keychain, "ServiceNow", "now-sdk", legacy);
    env.env_key_cmd()
        .args([
            "profile",
            "add",
            "dev",
            "--instance",
            "https://dev.service-now.com",
            "--auth-method",
            "basic",
            "--username",
            "admin",
        ])
        .assert()
        .success();
    common::write_test_keychain_entry(&env.keychain, "snow-cli", "dev:password", "dev-password");
    let before = read(&env.file());

    env.env_key_cmd()
        .args(["profile", "sdk", "export", "dev", "--alias", "snow-dev"])
        .assert()
        .success();

    let after = read(&env.file());
    assert_ne!(before, after, "the encrypted file was rewritten");
    assert!(!String::from_utf8_lossy(&after).contains("dev-password"));
    assert_eq!(
        common::read_test_keychain_entry(&env.keychain, "ServiceNow", "now-sdk").as_deref(),
        Some(legacy),
        "the legacy blob older now-sdk versions read is untouched"
    );
    let output = env
        .env_key_cmd()
        .args(["--output", "json", "profile", "sdk", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let aliases: Vec<String> = listed(&output)
        .iter()
        .map(|p| p["alias"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(aliases, ["fixture-basic", "fixture-oauth", "snow-dev"]);
}

#[test]
fn a_file_without_its_key_is_refused_and_left_unchanged() {
    let env = Env::new().with_fixture();
    let before = read(&env.file());
    env.cmd()
        .args(["profile", "sdk", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("NOW_SDK_CREDENTIALS_UNREADABLE"))
        .stderr(predicate::str::contains("did not modify"));
    env.cmd()
        .env("SN_SDK_KEY_SOURCE", "env")
        .env(
            "SN_SDK_ENCRYPTION_KEY",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        )
        .args(["profile", "sdk", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("could not be decrypted"));
    assert_eq!(read(&env.file()), before);
}

#[test]
fn without_any_encrypted_store_the_legacy_blob_is_still_used() {
    let env = Env::new();
    env.cmd()
        .args([
            "profile",
            "add",
            "dev",
            "--instance",
            "https://dev.service-now.com",
            "--auth-method",
            "basic",
            "--username",
            "admin",
        ])
        .assert()
        .success();
    common::write_test_keychain_entry(&env.keychain, "snow-cli", "dev:password", "dev-password");
    env.cmd()
        .args(["profile", "sdk", "export", "dev"])
        .assert()
        .success();
    assert!(!env.file().exists(), "no encrypted file is created");
    let legacy = common::read_test_keychain_entry(&env.keychain, "ServiceNow", "now-sdk").unwrap();
    assert!(legacy.contains("\"alias\":\"dev\""), "{legacy}");
}

#[test]
fn env_key_source_without_a_key_reads_empty_and_refuses_to_write() {
    let env = Env::new();
    env.cmd()
        .env("SN_SDK_KEY_SOURCE", "env")
        .args(["--output", "json", "profile", "sdk", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[]"));
    env.cmd()
        .env("SN_SDK_KEY_SOURCE", "keystore")
        .args(["profile", "sdk", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown SN_SDK_KEY_SOURCE"));
}

#[cfg(unix)]
#[test]
fn export_with_a_wrong_key_fails_without_replacing_the_file() {
    use std::os::unix::fs::MetadataExt;
    let env = Env::new().with_fixture();
    env.cmd()
        .args([
            "profile",
            "add",
            "dev",
            "--instance",
            "https://dev.service-now.com",
            "--auth-method",
            "basic",
            "--username",
            "admin",
        ])
        .assert()
        .success();
    common::write_test_keychain_entry(&env.keychain, "snow-cli", "dev:password", "dev-password");
    let before = std::fs::metadata(env.file()).unwrap();
    let bytes = read(&env.file());

    env.cmd()
        .env("SN_SDK_KEY_SOURCE", "env")
        .env(
            "SN_SDK_ENCRYPTION_KEY",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        )
        .args(["profile", "sdk", "export", "dev"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("NOW_SDK_CREDENTIALS_UNREADABLE"));

    let after = std::fs::metadata(env.file()).unwrap();
    assert_eq!(before.ino(), after.ino(), "the file was not replaced");
    assert_eq!(read(&env.file()), bytes);
}

#[test]
fn reads_with_a_raw_32_byte_keychain_key_as_now_sdk_4_14_0_wrote() {
    let env = Env::new().with_fixture();
    // The fixture key is 32 ASCII bytes, so the plaintext test keychain can
    // hold it raw, as now-sdk 4.14.0 stored keys.
    common::write_test_keychain_entry(
        &env.keychain,
        "ServiceNow",
        "now-sdk-credentials-key",
        "snow-cli-now-sdk-fixture-key-32b",
    );
    env.cmd()
        .args(["--output", "json", "profile", "sdk", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("fixture-basic"));
}
