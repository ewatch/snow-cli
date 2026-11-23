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
    fs::write(bundle.join("packs/table-api/PACK.md"), "# Table API pack\n").unwrap();
    fs::write(
        bundle.join("packs/snu-browser-helper/PACK.md"),
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
    assert!(install_dir.join("packs/table-api/PACK.md").exists());
    assert!(
        !install_dir
            .join("packs/snu-browser-helper/PACK.md")
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
path = "packs/table-api/PACK.md"
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
        .and(path("/packs/table-api/PACK.md"))
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
        fs::read(install_dir.join("packs/table-api/PACK.md")).unwrap(),
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

fn bundled_skill_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills/snow-cli")
}

#[test]
fn bundled_skill_installs_core_and_references_without_packs() {
    let target = tempdir().unwrap();
    let skill_dir = bundled_skill_dir();

    let output = cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            skill_dir.to_str().unwrap(),
            "--target-dir",
            target.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["name"], "snow/snow-cli");
    assert_eq!(json["packs"], serde_json::json!([]));

    let install_dir = target.path().join("snow-cli");
    assert!(install_dir.join("SKILL.md").exists());
    assert!(install_dir.join("skill.toml").exists());
    assert!(install_dir.join("snow-cli-skill.lock.toml").exists());

    let mut reference_count = 0usize;
    for entry in fs::read_dir(skill_dir.join("references")).unwrap() {
        let entry = entry.unwrap();
        let file_name = entry.file_name();
        if entry.path().extension().and_then(|s| s.to_str()) == Some("md") {
            reference_count += 1;
            assert!(
                install_dir.join("references").join(&file_name).exists(),
                "expected bundled reference {} to be installed",
                file_name.to_string_lossy()
            );
        }
    }
    assert!(
        reference_count >= 10,
        "expected bundled references to be present"
    );
    assert!(
        !install_dir.join("packs").exists(),
        "packs must not be installed unless explicitly requested"
    );
}

#[test]
fn bundled_skill_installs_selected_and_all_packs() {
    let skill_dir = bundled_skill_dir();

    let single_target = tempdir().unwrap();
    cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            skill_dir.to_str().unwrap(),
            "--target-dir",
            single_target.path().to_str().unwrap(),
            "--pack",
            "itsm",
        ])
        .assert()
        .success();

    let single_install = single_target.path().join("snow-cli");
    assert!(single_install.join("packs/itsm/PACK.md").exists());
    assert!(!single_install.join("packs/cmdb/PACK.md").exists());

    let all_target = tempdir().unwrap();
    let output = cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            skill_dir.to_str().unwrap(),
            "--target-dir",
            all_target.path().to_str().unwrap(),
            "--all-packs",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    let installed_packs: Vec<&str> = json["packs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        installed_packs,
        vec![
            "catalog",
            "cmdb",
            "csm",
            "hrsd",
            "itsm",
            "knowledge",
            "plugins",
            "reporting",
            "secops",
            "snu",
            "workflows",
        ]
    );

    let all_install = all_target.path().join("snow-cli");
    for pack in installed_packs {
        assert!(all_install.join(format!("packs/{pack}/PACK.md")).exists());
    }
}

fn collect_markdown_files(root: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

fn extract_bash_commands(markdown: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut in_bash = false;
    let mut current = String::new();

    for line in markdown.lines() {
        let trimmed = line.trim();
        if let Some(lang) = trimmed.strip_prefix("```") {
            if in_bash {
                if !current.trim().is_empty() {
                    commands.push(std::mem::take(&mut current));
                }
                in_bash = false;
            } else if lang.trim() == "bash" {
                in_bash = true;
            }
            continue;
        }
        if !in_bash {
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            if !current.trim().is_empty() {
                commands.push(std::mem::take(&mut current));
            }
            continue;
        }
        if let Some(continued) = trimmed.strip_suffix('\\') {
            current.push_str(continued);
            current.push(' ');
        } else {
            current.push_str(trimmed);
            commands.push(std::mem::take(&mut current));
        }
    }

    commands
}

#[test]
fn bundled_skill_links_resolve_and_cli_examples_parse() {
    use clap::Parser;
    use regex::Regex;
    use snow_cli::cli::args::Cli;
    use snow_cli::cli::readonly_args::ReadOnlyCli;
    use std::collections::BTreeSet;

    let skill_dir = bundled_skill_dir();

    // 1. Every reference linked from SKILL.md exists, and every references/*.md file is linked.
    let skill_md = fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
    let link_re = Regex::new(r"\((references/[a-zA-Z0-9._-]+\.md)\)").unwrap();
    let linked_refs: BTreeSet<String> = link_re
        .captures_iter(&skill_md)
        .map(|c| c[1].to_string())
        .collect();
    let disk_refs: BTreeSet<String> = fs::read_dir(skill_dir.join("references"))
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) == Some("md") {
                Some(format!(
                    "references/{}",
                    path.file_name().unwrap().to_string_lossy()
                ))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        linked_refs, disk_refs,
        "SKILL.md reference links and references/*.md on disk must match exactly"
    );

    // 2. skill.toml version matches CARGO_PKG_VERSION and [[packs]] matches packs/* bidirectionally.
    let manifest: toml::Value =
        toml::from_str(&fs::read_to_string(skill_dir.join("skill.toml")).unwrap()).unwrap();
    assert_eq!(
        manifest["version"].as_str(),
        Some(env!("CARGO_PKG_VERSION")),
        "skills/snow-cli/skill.toml version must match Cargo.toml version"
    );
    let declared_packs: BTreeSet<String> = manifest["packs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect();
    let disk_packs: BTreeSet<String> = fs::read_dir(skill_dir.join("packs"))
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            if path.is_dir() {
                Some(path.file_name().unwrap().to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        declared_packs, disk_packs,
        "skill.toml [[packs]] and packs/* directories must match exactly"
    );
    for name in &declared_packs {
        assert!(
            skill_dir.join(format!("packs/{name}/PACK.md")).exists(),
            "missing packs/{name}/PACK.md"
        );
        assert!(
            !skill_dir.join(format!("packs/{name}/SKILL.md")).exists(),
            "packs/{name}/SKILL.md should be named PACK.md to avoid nested skill discovery"
        );
        assert!(
            skill_md.contains(&format!("packs/{name}/PACK.md")),
            "SKILL.md should route to packs/{name}/PACK.md"
        );
    }

    // 3. Every snow-cli / snow-cli-ro command in bash blocks parses cleanly.
    //    `allowed-tools` only pre-approves snow-cli-ro, so an example written as
    //    plain snow-cli must be one the read-only binary rejects.
    let placeholder_re = Regex::new(r"<[^>]+>").unwrap();
    let substitute_placeholders = |token: &str| -> String {
        placeholder_re
            .replace_all(token, |caps: &regex::Captures| {
                let inner = caps[0].to_ascii_lowercase();
                if inner.contains("sys_id") {
                    "0123456789abcdef0123456789abcdef"
                } else {
                    "placeholder"
                }
            })
            .into_owned()
    };

    let mut md_files = Vec::new();
    collect_markdown_files(&skill_dir, &mut md_files);
    md_files.sort();

    let mut parsed_count = 0usize;
    let mut ro_verified_count = 0usize;
    for file in md_files {
        let content = fs::read_to_string(&file).unwrap();
        for raw_cmd in extract_bash_commands(&content) {
            let Some(raw_tokens) = shlex::split(&raw_cmd) else {
                panic!(
                    "failed to shell-tokenize command in {}: {raw_cmd}",
                    file.display()
                );
            };
            // Drop trailing inline `# ...` comments (shlex keeps `#` as its own token when unquoted).
            let tokens: Vec<String> = raw_tokens
                .into_iter()
                .take_while(|t| !t.starts_with('#'))
                .map(|t| substitute_placeholders(&t))
                .collect();
            let Some(bin) = tokens.first().map(String::as_str) else {
                continue;
            };
            match bin {
                "snow-cli" => {
                    if let Err(err) = Cli::try_parse_from(&tokens) {
                        panic!(
                            "failed to parse snow-cli example in {}:\n  command: {raw_cmd}\n  error: {err}",
                            file.display()
                        );
                    }
                    let mut ro_tokens = tokens.clone();
                    ro_tokens[0] = "snow-cli-ro".to_string();
                    assert!(
                        ReadOnlyCli::try_parse_from(&ro_tokens).is_err(),
                        "snow-cli-ro accepts this example in {}; write it as `snow-cli-ro`:\n  command: {raw_cmd}",
                        file.display()
                    );
                    parsed_count += 1;
                }
                "snow-cli-ro" => {
                    if let Err(err) = ReadOnlyCli::try_parse_from(&tokens) {
                        panic!(
                            "failed to parse snow-cli-ro example in {}:\n  command: {raw_cmd}\n  error: {err}",
                            file.display()
                        );
                    }
                    parsed_count += 1;
                    ro_verified_count += 1;
                }
                _ => {}
            }
        }
    }

    assert!(
        parsed_count >= 100,
        "expected to parse at least 100 CLI examples across skill docs, got {parsed_count}"
    );
    assert!(
        ro_verified_count >= 80,
        "expected at least 80 read-only CLI examples verified against ReadOnlyCli, got {ro_verified_count}"
    );
}

#[test]
fn bundled_flag_installs_embedded_skill_and_reports_dry_run_actions() {
    let target = tempdir().unwrap();

    // 1. Dry-run on empty target writes nothing and plans `create` for every file.
    let dry_out = cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            "--bundled",
            "--dry-run",
            "--pack",
            "knowledge",
            "--target-dir",
            target.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let dry_json: Value = serde_json::from_slice(&dry_out).unwrap();
    assert_eq!(dry_json["status"], "dry_run");
    assert_eq!(dry_json["dry_run"], true);
    assert_eq!(dry_json["source"], "bundled");
    assert_eq!(dry_json["packs"], serde_json::json!(["knowledge"]));
    let files = dry_json["files"].as_array().unwrap();
    assert!(files.iter().all(|f| f["action"] == "create"));
    assert!(
        !target.path().join("snow-cli").exists(),
        "--dry-run must not create install directory"
    );

    // 2. Real install writes files and lockfile.
    let real_out = cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            "--bundled",
            "--pack",
            "knowledge",
            "--target-dir",
            target.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let real_json: Value = serde_json::from_slice(&real_out).unwrap();
    assert_eq!(real_json["status"], "success");
    assert_eq!(real_json["dry_run"], false);

    let install_dir = target.path().join("snow-cli");
    assert!(install_dir.join("SKILL.md").exists());
    assert!(install_dir.join("packs/knowledge/PACK.md").exists());
    assert!(install_dir.join("references/acls.md").exists());
    assert!(install_dir.join("snow-cli-skill.lock.toml").exists());

    // 3. Re-running --dry-run after a local edit reports `conflict` without --force,
    //    and real install refuses (exit 2 / VALIDATION_ERROR) until --force is passed.
    fs::write(install_dir.join("SKILL.md"), "# locally modified\n").unwrap();
    let second_dry = cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            "--bundled",
            "--dry-run",
            "--pack",
            "knowledge",
            "--target-dir",
            target.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let second_json: Value = serde_json::from_slice(&second_dry).unwrap();
    let second_files = second_json["files"].as_array().unwrap();
    let skill_entry = second_files
        .iter()
        .find(|f| f["path"] == "SKILL.md")
        .unwrap();
    assert_eq!(skill_entry["action"], "conflict");
    let acls_entry = second_files
        .iter()
        .find(|f| f["path"] == "references/acls.md")
        .unwrap();
    assert_eq!(acls_entry["action"], "unchanged");

    cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            "--bundled",
            "--pack",
            "knowledge",
            "--target-dir",
            target.path().to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("VALIDATION_ERROR"))
        .stderr(predicate::str::contains("SKILL.md"));
    assert_eq!(
        fs::read_to_string(install_dir.join("SKILL.md")).unwrap(),
        "# locally modified\n"
    );

    // 4. With --force, dry-run shows `overwrite` and real install restores the file.
    let force_dry = cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            "--bundled",
            "--dry-run",
            "--force",
            "--pack",
            "knowledge",
            "--target-dir",
            target.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let force_json: Value = serde_json::from_slice(&force_dry).unwrap();
    let skill_force_entry = force_json["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == "SKILL.md")
        .unwrap();
    assert_eq!(skill_force_entry["action"], "overwrite");

    cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            "--bundled",
            "--force",
            "--pack",
            "knowledge",
            "--target-dir",
            target.path().to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(
        fs::read_to_string(install_dir.join("SKILL.md"))
            .unwrap()
            .contains("# snow-cli")
    );
}

#[test]
fn reinstall_with_fewer_packs_prunes_unmodified_and_preserves_modified_packs() {
    let target = tempdir().unwrap();

    // Install two packs first.
    cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            "--bundled",
            "--pack",
            "itsm",
            "--pack",
            "cmdb",
            "--pack",
            "knowledge",
            "--target-dir",
            target.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    let install_dir = target.path().join("snow-cli");
    assert!(install_dir.join("packs/itsm/PACK.md").exists());
    assert!(install_dir.join("packs/cmdb/PACK.md").exists());
    assert!(install_dir.join("packs/knowledge/PACK.md").exists());

    // Locally edit one of the packs we are about to drop (knowledge).
    fs::write(
        install_dir.join("packs/knowledge/PACK.md"),
        "# custom local notes\n",
    )
    .unwrap();

    // Re-install with only --pack itsm: cmdb should be pruned, knowledge preserved as keep_modified.
    let reinstall_out = cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            "--bundled",
            "--pack",
            "itsm",
            "--target-dir",
            target.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&reinstall_out).unwrap();
    let files = json["files"].as_array().unwrap();
    let cmdb_entry = files
        .iter()
        .find(|f| f["path"] == "packs/cmdb/PACK.md")
        .unwrap();
    assert_eq!(cmdb_entry["action"], "remove");
    let kb_entry = files
        .iter()
        .find(|f| f["path"] == "packs/knowledge/PACK.md")
        .unwrap();
    assert_eq!(kb_entry["action"], "keep_modified");

    assert!(install_dir.join("packs/itsm/PACK.md").exists());
    assert!(!install_dir.join("packs/cmdb/PACK.md").exists());
    assert!(!install_dir.join("packs/cmdb").exists());
    assert!(install_dir.join("packs/knowledge/PACK.md").exists());
}

#[test]
fn skill_install_validation_errors_and_flag_conflicts() {
    let target = tempdir().unwrap();

    // Unknown pack returns VALIDATION_ERROR (exit 2) and lists available packs.
    cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            "--bundled",
            "--pack",
            "nonexistent",
            "--target-dir",
            target.path().to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("VALIDATION_ERROR"))
        .stderr(predicate::str::contains("Available packs:"))
        .stderr(predicate::str::contains("itsm"));

    // --bundled combined with a positional source is rejected by clap.
    cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            "--bundled",
            "./skills/snow-cli",
            "--target-dir",
            target.path().to_str().unwrap(),
        ])
        .assert()
        .code(2);

    // --pack combined with --all-packs returns VALIDATION_ERROR (exit 2).
    cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            "--bundled",
            "--pack",
            "itsm",
            "--all-packs",
            "--target-dir",
            target.path().to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("VALIDATION_ERROR"));
}

#[cfg(unix)]
#[test]
fn skill_install_rejects_symlinked_subdirectory_and_lockfile() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let target = temp.path().join("target");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();

    // 1. Symlinked references/ directory inside install_dir must be rejected.
    let install_dir = target.join("snow-cli");
    fs::create_dir_all(&install_dir).unwrap();
    symlink(&outside, install_dir.join("references")).unwrap();

    cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            "--bundled",
            "--target-dir",
            target.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("symlink"));
    assert!(
        fs::read_dir(&outside).unwrap().next().is_none(),
        "must not write through symlinked subdirectory"
    );

    // 2. Symlinked lockfile inside install_dir must be rejected.
    let target2 = temp.path().join("target2");
    let install_dir2 = target2.join("snow-cli");
    fs::create_dir_all(&install_dir2).unwrap();
    let outside_lock = temp.path().join("outside-lock.toml");
    fs::write(&outside_lock, "").unwrap();
    symlink(&outside_lock, install_dir2.join("snow-cli-skill.lock.toml")).unwrap();

    cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            "--bundled",
            "--target-dir",
            target2.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("symlink"));
}

#[test]
fn skill_command_is_visible_in_top_level_help() {
    let output = cargo_bin_cmd!("snow-cli")
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let help = String::from_utf8(output).unwrap();
    let re = regex::Regex::new(r"(?m)^\s+skill\s+").unwrap();
    assert!(
        re.is_match(&help),
        "expected top-level help to list `skill` subcommand row, got:\n{help}"
    );
}

#[tokio::test]
async fn staged_hosted_bundle_installs_over_http_with_verified_digests() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let staged = tempdir().unwrap();
    let target = tempdir().unwrap();

    let status = match std::process::Command::new("python3")
        .args([
            manifest_dir
                .join("scripts/stage-skill-bundle.py")
                .to_str()
                .unwrap(),
            "--source",
            bundled_skill_dir().to_str().unwrap(),
            "--out",
            staged.path().to_str().unwrap(),
        ])
        .status()
    {
        Ok(status) => status,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipping staged_hosted_bundle test: python3 not found on PATH");
            return;
        }
        Err(err) => panic!("failed to invoke python3: {err}"),
    };
    assert!(status.success(), "stage-skill-bundle.py must succeed");

    let server = MockServer::start().await;
    let mut staged_files = Vec::new();
    fn walk_all(root: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk_all(&path, out);
            } else if path.is_file() {
                out.push(path);
            }
        }
    }
    walk_all(staged.path(), &mut staged_files);
    for file in staged_files {
        let rel = file
            .strip_prefix(staged.path())
            .unwrap()
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        let bytes = fs::read(&file).unwrap();
        Mock::given(method("GET"))
            .and(path(format!("/skills/snow-cli/{rel}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
            .mount(&server)
            .await;
    }

    cargo_bin_cmd!("snow-cli")
        .args([
            "skill",
            "install",
            &format!("{}/skills/snow-cli/skill.toml", server.uri()),
            "--target-dir",
            target.path().to_str().unwrap(),
            "--pack",
            "snu",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\":\"success\""));

    let install_dir = target.path().join("snow-cli");
    assert!(install_dir.join("SKILL.md").exists());
    assert!(install_dir.join("references/acls.md").exists());
    assert!(install_dir.join("packs/snu/PACK.md").exists());
    assert!(!install_dir.join("packs/itsm/PACK.md").exists());
}
