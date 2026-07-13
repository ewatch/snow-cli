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
        .success()
        .stdout(predicate::str::contains(
            "\"summary\":{\"total\":5,\"inserted\":1,\"updated\":1,\"ignored\":1,\"error\":1,\"other\":1}",
        ))
        .stdout(predicate::str::contains("ISET0010001"));
}

#[tokio::test]
async fn test_import_set_load_with_errors_succeeds_by_default() {
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
        .success()
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
