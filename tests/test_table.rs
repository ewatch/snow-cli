//! Wiremock-backed integration tests for table CRUD commands.
//!
//! These tests exercise the full CLI binary through `assert_cmd`, with
//! a wiremock `MockServer` providing HTTP responses. Authentication uses
//! `api_key` auth with `SNOW_CLI_API_TOKEN` env var so no OS keychain
//! is needed.

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use wiremock::matchers::{body_string_contains, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Create a temp config file for api_key auth and return (dir, config_path).
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

// --- table list ---

#[tokio::test]
async fn test_table_list_json_output() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(header("Authorization", "Bearer test-api-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "abc123", "number": "INC001", "short_description": "Test incident 1"},
                {"sys_id": "def456", "number": "INC002", "short_description": "Test incident 2"}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "table", "list", "incident"])
        .assert()
        .success()
        .stdout(predicate::str::contains("INC001"))
        .stdout(predicate::str::contains("INC002"))
        .stdout(predicate::str::contains("abc123"));
}

#[tokio::test]
async fn test_table_list_csv_output() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "abc123", "number": "INC001"},
                {"sys_id": "def456", "number": "INC002"}
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
            "--output",
            "csv",
            "--instance",
            &server.uri(),
            "table",
            "list",
            "incident",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("number"))
        .stdout(predicate::str::contains("sys_id"))
        .stdout(predicate::str::contains("INC001"))
        .stdout(predicate::str::contains("INC002"));
}

#[tokio::test]
async fn test_table_list_with_query_params() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_query", "active=true"))
        .and(query_param("sysparm_fields", "sys_id,number"))
        .and(query_param("sysparm_orderby", "number"))
        .and(query_param("sysparm_offset", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "abc123", "number": "INC001"}
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
            "table",
            "list",
            "incident",
            "--query",
            "active=true",
            "--fields",
            "sys_id,number",
            "--limit",
            "5",
            "--order-by",
            "number",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("INC001"));
}

#[tokio::test]
async fn test_table_list_empty_result() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": []})))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "table", "list", "incident"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[]"));
}

#[tokio::test]
async fn test_table_list_404_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/nonexistent"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Table 'nonexistent' not found"))
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "table", "list", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("NOT_FOUND"));
}

// --- table get ---

#[tokio::test]
async fn test_table_get_single_record() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident/abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {"sys_id": "abc123", "number": "INC001", "state": "1"}
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
            "table",
            "get",
            "incident",
            "abc123",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("abc123"))
        .stdout(predicate::str::contains("INC001"));
}

#[tokio::test]
async fn test_table_get_with_fields() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident/abc123"))
        .and(query_param("sysparm_fields", "sys_id,number"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {"sys_id": "abc123", "number": "INC001"}
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
            "table",
            "get",
            "incident",
            "abc123",
            "--fields",
            "sys_id,number",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("abc123"))
        .stdout(predicate::str::contains("INC001"));
}

// --- table create ---

#[tokio::test]
async fn test_table_create_with_data_flag() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/table/incident"))
        .and(header("Content-Type", "application/json"))
        .and(body_string_contains("short_description"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "result": {
                "sys_id": "new789",
                "number": "INC003",
                "short_description": "New test incident"
            }
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
            "table",
            "create",
            "incident",
            "--data",
            r#"{"short_description":"New test incident"}"#,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("new789"))
        .stdout(predicate::str::contains("INC003"));
}

#[tokio::test]
async fn test_table_create_from_stdin() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/table/incident"))
        .and(body_string_contains("piped_description"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "result": {
                "sys_id": "stdin789",
                "number": "INC004",
                "short_description": "piped_description"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "table", "create", "incident"])
        .write_stdin(r#"{"short_description":"piped_description"}"#)
        .assert()
        .success()
        .stdout(predicate::str::contains("stdin789"))
        .stdout(predicate::str::contains("INC004"));
}

// --- table update ---

#[tokio::test]
async fn test_table_update_with_data_flag() {
    let server = MockServer::start().await;

    Mock::given(method("PATCH"))
        .and(path("/api/now/table/incident/abc123"))
        .and(header("Content-Type", "application/json"))
        .and(body_string_contains("state"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {
                "sys_id": "abc123",
                "number": "INC001",
                "state": "2"
            }
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
            "table",
            "update",
            "incident",
            "abc123",
            "--data",
            r#"{"state":"2"}"#,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("abc123"))
        .stdout(predicate::str::contains("\"state\""));
}

// --- table delete ---

#[tokio::test]
async fn test_table_delete_with_yes() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/api/now/table/incident/abc123"))
        .and(header("Authorization", "Bearer test-api-token"))
        .respond_with(ResponseTemplate::new(204))
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
            "table",
            "delete",
            "incident",
            "abc123",
            "--yes",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("deleted"));
}

// --- error cases ---

#[tokio::test]
async fn test_table_list_server_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "table", "list", "incident"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("SERVER_ERROR"));
}

#[tokio::test]
async fn test_table_create_invalid_json() {
    let (_dir, config_path) = api_key_config();

    // No mock needed — invalid JSON is caught before the HTTP request
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            "http://localhost:1",
            "table",
            "create",
            "incident",
            "--data",
            "not valid json",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid JSON"));
}
