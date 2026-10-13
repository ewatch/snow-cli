#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn api_key_config() -> (tempfile::TempDir, std::path::PathBuf) {
    common::create_temp_config(
        r#"
default_profile = "default"

[profiles.default]
instance = "https://placeholder.service-now.com"
auth_method = "api_key"
"#,
    )
}

#[tokio::test]
async fn test_import_set_csv_is_rejected_before_post() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/import/u_import_table"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"status": "inserted", "sys_id": "created123"},
                {"status": "error", "error_message": "bad row"}
            ]
        })))
        .expect(0)
        .mount(&server)
        .await;

    for source in ["flag", "env", "config"] {
        let (_dir, config_path) = common::create_temp_config(&format!(
            "default_profile = 'default'\n{}[profiles.default]\ninstance = 'https://placeholder.service-now.com'\nauth_method = 'api_key'\n",
            if source == "config" {
                "default_output = 'csv'\n"
            } else {
                ""
            }
        ));
        for row_flag in [None, Some("--allow-row-errors"), Some("--fail-on-error")] {
            let mut cmd = cargo_bin_cmd!("snow-cli");
            cmd.env("SNOW_CLI_CONFIG", &config_path)
                .env("SNOW_CLI_API_TOKEN", "test-api-token")
                .env_remove("SNOW_CLI_OUTPUT")
                .args(["--instance", &server.uri()]);
            match source {
                "flag" => {
                    cmd.args(["--output", "csv"]);
                }
                "env" => {
                    cmd.env("SNOW_CLI_OUTPUT", "csv");
                }
                _ => {}
            }
            cmd.args(["import-set", "load", "u_import_table", "--data", "{}"]);
            if let Some(flag) = row_flag {
                cmd.arg(flag);
            }
            let assertion = cmd.assert().code(2).stdout("");
            let error: serde_json::Value =
                serde_json::from_slice(&assertion.get_output().stderr).unwrap();
            assert!(
                error["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("CSV output is not supported for import-set load")
            );
            assert!(
                error["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("--output json")
            );
        }
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_import_set_explicit_json_overrides_csv_defaults() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/import/u_import_table"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"status": "inserted", "sys_id": "created123"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config_path) = common::create_temp_config(
        "default_profile = 'default'\ndefault_output = 'csv'\n[profiles.default]\ninstance = 'https://placeholder.service-now.com'\nauth_method = 'api_key'\n",
    );
    let assertion = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .env("SNOW_CLI_OUTPUT", "csv")
        .args([
            "--instance",
            &server.uri(),
            "--output",
            "json",
            "import-set",
            "load",
            "u_import_table",
            "--data",
            "{}",
        ])
        .assert()
        .success()
        .stderr("");
    let value: serde_json::Value = serde_json::from_slice(&assertion.get_output().stdout).unwrap();
    assert_eq!(value["summary"]["inserted"], 1);
    assert_eq!(value["result"][0]["sys_id"], "created123");
}

#[tokio::test]
async fn test_import_set_load_with_data_flag() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/import/u_import_table"))
        .and(header("Authorization", "Bearer test-api-token"))
        .and(header("Content-Type", "application/json"))
        .and(body_string_contains("test_value"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "result": [{
                "status": "inserted",
                "record_link": "https://instance.service-now.com/api/now/table/u_import_table/imported123",
                "sys_id": "imported123"
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "import-set",
            "load",
            "u_import_table",
            "--data",
            r#"{"name":"test_value"}"#,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"command\":\"import-set load\""))
        .stdout(predicate::str::contains(
            "\"summary\":{\"total\":1,\"inserted\":1",
        ))
        .stdout(predicate::str::contains("inserted"))
        .stdout(predicate::str::contains("imported123"));
}

#[tokio::test]
async fn test_import_set_load_from_stdin() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/import/u_import_table"))
        .and(body_string_contains("stdin_value"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"status": "inserted", "sys_id": "stdin123"}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "import-set",
            "load",
            "u_import_table",
        ])
        .write_stdin(r#"{"name":"stdin_value"}"#)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"summary\":{\"total\":1,\"inserted\":1",
        ))
        .stdout(predicate::str::contains("stdin123"));
}

#[tokio::test]
async fn test_import_set_load_summarizes_mixed_statuses() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/import/u_import_table"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "import_set": "ISET0010001",
            "staging_table": "u_import_table",
            "result": [
                {"status": "inserted", "sys_id": "a1"},
                {"status": "updated", "sys_id": "a2"},
                {"status": "ignored", "sys_id": "a3"},
                {"status": "error", "error_message": "bad row"},
                {"status": "skipped"}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "import-set",
            "load",
            "u_import_table",
            "--data",
            r#"{"name":"mixed"}"#,
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("IMPORT_SET_ROW_ERRORS"))
        .stdout(predicate::str::contains(
            "\"summary\":{\"total\":5,\"inserted\":1,\"updated\":1,\"ignored\":1,\"error\":1,\"other\":1}",
        ))
        .stdout(predicate::str::contains("ISET0010001"));
}

#[tokio::test]
async fn test_import_set_load_with_errors_fails_by_default() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/import/u_import_table"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"status": "error", "error_message": "bad row"}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "import-set",
            "load",
            "u_import_table",
            "--data",
            r#"{"name":"mixed"}"#,
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("IMPORT_SET_ROW_ERRORS"))
        .stdout(predicate::str::contains("\"error\":1"))
        .stdout(predicate::str::contains("bad row"));
}

#[tokio::test]
async fn test_import_set_load_fail_on_error_returns_failure() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/import/u_import_table"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"status": "error", "error_message": "bad row"}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "import-set",
            "load",
            "u_import_table",
            "--fail-on-error",
            "--data",
            r#"{"name":"mixed"}"#,
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"error\":1"))
        .stderr(predicate::str::contains("1 row-level error"));
}

#[tokio::test]
async fn row_error_opt_out_warns_and_error_messages_without_status_fail() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/import/u_import_table"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"status":"inserted", "sys_id":"created"},
                {"error_message":"private failed-row detail"},
                {"status":" ERROR ", "error_message":""}
            ]
        })))
        .expect(2)
        .mount(&server)
        .await;
    let (_dir, config) = api_key_config();
    for allow in [false, true] {
        let mut cmd = cargo_bin_cmd!("snow-cli");
        cmd.env("SNOW_CLI_CONFIG", &config)
            .env("SNOW_CLI_API_TOKEN", "test-api-token")
            .args([
                "--instance",
                &server.uri(),
                "import-set",
                "load",
                "u_import_table",
                "--data",
                "{}",
            ]);
        if allow {
            cmd.arg("--allow-row-errors");
        }
        let assertion = cmd.assert().code(if allow { 0 } else { 1 });
        let output = assertion.get_output();
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["summary"]["inserted"], 1);
        assert_eq!(result["summary"]["error"], 2);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("private failed-row detail"));
        if allow {
            assert!(stderr.contains("--allow-row-errors"), "{stderr}");
            assert!(stderr.contains("row-level errors"), "{stderr}");
        } else {
            let error: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
            assert_eq!(error["error"]["code"], "IMPORT_SET_ROW_ERRORS");
        }
    }
}

#[test]
fn row_error_flags_conflict_and_help_describes_default() {
    cargo_bin_cmd!("snow-cli")
        .args([
            "import-set",
            "load",
            "u_import_table",
            "--fail-on-error",
            "--allow-row-errors",
            "--data",
            "{}",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
    cargo_bin_cmd!("snow-cli")
        .args(["import-set", "load", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("exit non-zero by default"))
        .stdout(predicate::str::contains("--allow-row-errors"));
}

#[test]
fn test_import_set_transform_still_fails_gracefully() {
    // Isolate from ambient config so the command reaches the "not implemented
    // yet" branch instead of failing earlier at profile resolution in a clean
    // environment (e.g. CI).
    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .args([
            "import-set",
            "transform",
            "6816f79cc0a8016401c5a33be04be441",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not implemented yet"))
        .stderr(predicate::str::contains("ran the transform automatically"));
}
