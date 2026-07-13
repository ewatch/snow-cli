#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

#[test]
fn skill_install_local_bundle_installs_selected_pack_only() {
    let temp = tempdir().unwrap();
    let bundle = temp.path().join("bundle");
    let target = temp.path().join("target");
    fs::create_dir_all(bundle.join("packs/table-api")).unwrap();
    fs::create_dir_all(bundle.join("packs/snu-browser-helper")).unwrap();
    fs::write(
        bundle.join("skill.toml"),
        r#"
name = "snow/snow-cli"
version = "1.0.0"

[[packs]]
name = "table-api"

[[packs]]
name = "snu-browser-helper"
"#,
    )
    .unwrap();
    fs::write(bundle.join("SKILL.md"), "# snow-cli\n").unwrap();
    fs::write(
        bundle.join("packs/table-api/SKILL.md"),
        "# Table API pack\n",
    )
    .unwrap();
    fs::write(
        bundle.join("packs/snu-browser-helper/SKILL.md"),
        "# SN-Utils pack\n",
    )
    .unwrap();

    let output = cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            bundle.to_str().unwrap(),
            "--target-dir",
            target.to_str().unwrap(),
            "--pack",
            "table-api",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["name"], "snow/snow-cli");
    assert_eq!(json["packs"], serde_json::json!(["table-api"]));

    let install_dir = target.join("snow-cli");
    assert!(install_dir.join("SKILL.md").exists());
    assert!(install_dir.join("packs/table-api/SKILL.md").exists());
    assert!(
        !install_dir
            .join("packs/snu-browser-helper/SKILL.md")
            .exists()
    );
    assert!(install_dir.join("snow-cli-skill.lock.toml").exists());
}

#[test]
fn skill_install_known_target_uses_agent_skill_root() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("home");
    let bundle = temp.path().join("bundle");
    fs::create_dir_all(&bundle).unwrap();
    fs::write(
        bundle.join("skill.toml"),
        r#"
name = "snow/snow-cli"
"#,
    )
    .unwrap();
    fs::write(bundle.join("SKILL.md"), "# snow-cli\n").unwrap();

    cargo_bin_cmd!("snow-cli")
        .env("HOME", &home)
        .args([
            "skill",
            "install",
            bundle.to_str().unwrap(),
            "--target",
            "codex",
        ])
        .assert()
        .success();

    assert!(home.join(".codex/skills/snow-cli/SKILL.md").exists());
    assert!(
        home.join(".codex/skills/snow-cli/snow-cli-skill.lock.toml")
            .exists()
    );
}

#[tokio::test]
async fn skill_install_url_manifest_fetches_declared_files() {
    let server = MockServer::start().await;
    let target = tempdir().unwrap();
    let skill = b"# snow-cli\n";
    let pack = b"# Table API pack\n";
    let manifest = format!(
        r#"
name = "snow/snow-cli"
version = "1.0.0"

[[packs]]
name = "table-api"

[[files]]
path = "SKILL.md"
sha256 = "{}"

[[files]]
path = "packs/table-api/SKILL.md"
sha256 = "{}"
"#,
        sha256_hex(skill),
        sha256_hex(pack)
    );

    Mock::given(method("GET"))
        .and(path("/skill.toml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(manifest))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/SKILL.md"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(skill.as_slice()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/packs/table-api/SKILL.md"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(pack.as_slice()))
        .mount(&server)
        .await;

    cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            &format!("{}/skill.toml", server.uri()),
            "--target-dir",
            target.path().to_str().unwrap(),
            "--pack",
            "table-api",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\":\"success\""));

    let install_dir = target.path().join("snow-cli");
    assert_eq!(fs::read(install_dir.join("SKILL.md")).unwrap(), skill);
    assert_eq!(
        fs::read(install_dir.join("packs/table-api/SKILL.md")).unwrap(),
        pack
    );
}

#[test]
fn skill_install_rejects_unsafe_manifest_paths() {
    let temp = tempdir().unwrap();
    let bundle = temp.path().join("bundle");
    let target = temp.path().join("target");
    fs::create_dir_all(&bundle).unwrap();
    fs::write(
        bundle.join("skill.toml"),
        r#"
name = "snow/snow-cli"

[[files]]
path = "../evil.md"
"#,
    )
    .unwrap();

    cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            bundle.to_str().unwrap(),
            "--target-dir",
            target.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unsafe skill file path"));
}

#[tokio::test]
async fn skill_install_rejects_remote_digest_mismatch() {
    let server = MockServer::start().await;
    let target = tempdir().unwrap();
    let skill = b"# snow-cli\n";
    let manifest = r#"
name = "snow/snow-cli"

[[files]]
path = "SKILL.md"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
"#;

    Mock::given(method("GET"))
        .and(path("/skill.toml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(manifest))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/SKILL.md"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(skill.as_slice()))
        .mount(&server)
        .await;

    cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            &format!("{}/skill.toml", server.uri()),
            "--target-dir",
            target.path().to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Digest mismatch for SKILL.md"));
}
