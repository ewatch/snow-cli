#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn api_key_config(instance: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    common::create_temp_config(&format!(
        r#"
default_profile = "default"

[profiles.default]
instance = "{instance}"
auth_method = "api_key"
"#
    ))
}

#[tokio::test]
async fn seed_apply_tracks_records_and_cleanup_deletes_in_reverse_order() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/table/cmn_location"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "result": {"sys_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/now/table/incident"))
        .and(body_string_contains(
            "\"location\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"",
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "result": {"sys_id": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(
            "/api/now/table/incident/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path(
            "/api/now/table/cmn_location/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let (dir, config_path) = api_key_config(&server.uri());
    let spec_path = dir.path().join("seed.json");
    std::fs::write(
        &spec_path,
        r#"{"version":1,"kind":"seed-spec","seed":42,"tables":[
            {"name":"incident","count":1,"template":{"short_description":"Generated {{index}}"},"references":{"location":{"table":"cmn_location","strategy":"round_robin"}}},
            {"name":"cmn_location","records":[{"name":"Berlin"}]}
        ]}"#,
    )
    .unwrap();

    let applied = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "seed",
            "apply",
            "--file",
            spec_path.to_str().unwrap(),
            "--yes",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result: serde_json::Value = serde_json::from_slice(&applied).unwrap();
    let run_id = result["run_id"].as_str().unwrap();
    assert_eq!(result["created"], 2);
    assert!(
        dir.path()
            .join("seed-runs")
            .join(format!("{run_id}.json"))
            .exists()
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "seed",
            "cleanup",
            run_id,
            "--yes",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"deleted\":2"));

    assert!(
        !dir.path()
            .join("seed-runs")
            .join(format!("{run_id}.json"))
            .exists()
    );
    server.verify().await;
}

#[tokio::test]
async fn seed_apply_dry_run_does_not_require_credentials_or_network() {
    let (dir, config_path) = api_key_config("http://127.0.0.1:1");
    let spec_path = dir.path().join("seed.json");
    std::fs::write(
        &spec_path,
        r#"{"version":1,"kind":"seed-spec","tables":[{"name":"incident","records":[{"short_description":"preview"}]}]}"#,
    )
    .unwrap();
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args([
            "seed",
            "apply",
            "--file",
            spec_path.to_str().unwrap(),
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"command\":\"seed plan\""));
}
