#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Wiremock-backed integration tests for table CRUD commands.
//!
//! These tests exercise the full CLI binary through `assert_cmd`, with
//! a wiremock `MockServer` providing HTTP responses. Authentication uses
//! `api_key` auth with `SNOW_CLI_API_TOKEN` env var so no OS keychain
//! is needed.

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use wiremock::matchers::{
    body_string_contains, header, method, path, query_param, query_param_is_missing,
};
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
                {"sys_id": "6816f79cc0a8016401c5a33be04be441", "number": "INC001", "short_description": "Test incident 1"},
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
        .stdout(predicate::str::contains("6816f79cc0a8016401c5a33be04be441"));
}

#[tokio::test]
async fn test_table_list_truncates_long_fields_unless_full() {
    let server = MockServer::start().await;
    let long_description = "x".repeat(2_001);

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"sys_id": "6816f79cc0a8016401c5a33be04be441", "description": long_description}]
        })))
        .expect(2)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();
    let run = |full: bool| {
        let mut command = cargo_bin_cmd!("snow-cli");
        command
            .env("SNOW_CLI_CONFIG", &config_path)
            .env("SNOW_CLI_API_TOKEN", "test-api-token")
            .args(["--instance", &server.uri(), "table", "list", "incident"]);
        if full {
            command.arg("--full");
        }
        command.assert().success().get_output().stdout.clone()
    };

    let truncated: serde_json::Value = serde_json::from_slice(&run(false)).unwrap();
    assert_eq!(truncated["fields_truncated"], true);
    let description = truncated["records"][0]["description"].as_str().unwrap();
    assert!(description.starts_with(&"x".repeat(2_000)));
    assert!(description.ends_with("[truncated 2000 of 2001 chars; use --full]"));

    let full: serde_json::Value = serde_json::from_slice(&run(true)).unwrap();
    assert!(full.get("fields_truncated").is_none());
    assert_eq!(full["records"][0]["description"], "x".repeat(2_001));
}

#[tokio::test]
async fn test_table_list_csv_output() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "6816f79cc0a8016401c5a33be04be441", "number": "INC001"},
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
async fn test_table_list_jsonl_output() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "6816f79cc0a8016401c5a33be04be441", "number": "INC001"},
                {"sys_id": "def456", "number": "INC002"}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    let assert = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--output",
            "jsonl",
            "--instance",
            &server.uri(),
            "table",
            "list",
            "incident",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 3);
    // Leading meta line carries returned/truncated state for streaming consumers
    let meta: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(meta["meta"]["returned"], 2);
    assert_eq!(meta["meta"]["truncated"], false);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(lines[1]).unwrap(),
        serde_json::json!({"number": "INC001", "sys_id": "6816f79cc0a8016401c5a33be04be441"})
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(lines[2]).unwrap(),
        serde_json::json!({"number": "INC002", "sys_id": "def456"})
    );
}

#[tokio::test]
async fn test_table_list_toon_output() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "6816f79cc0a8016401c5a33be04be441", "number": "INC001"},
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
            "toon",
            "--instance",
            &server.uri(),
            "table",
            "list",
            "incident",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("[2]"))
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
        .and(query_param("sysparm_query", "active=true^ORDERBYnumber"))
        .and(query_param("sysparm_fields", "sys_id,number"))
        .and(query_param_is_missing("sysparm_orderby"))
        .and(query_param("sysparm_offset", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "6816f79cc0a8016401c5a33be04be441", "number": "INC001"}
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

// Regression (servicenow-cli-120.1): the Table API ignores `sysparm_orderby`,
// so sorting must travel inside `sysparm_query`, and `-field` must parse as a
// value rather than as an unknown short flag.
#[tokio::test]
async fn test_table_list_order_by_descending_without_query() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param(
            "sysparm_query",
            "ORDERBYDESCsys_created_on^ORDERBYnumber",
        ))
        .and(query_param_is_missing("sysparm_orderby"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"sys_id": "6816f79cc0a8016401c5a33be04be441", "number": "INC009"}]
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
            "--order-by",
            "-sys_created_on,number",
            "--limit",
            "1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("INC009"));
}

#[tokio::test]
async fn test_table_list_invalid_order_by_is_rejected() {
    let (_dir, config_path) = api_key_config();

    // No mock needed: the sort spec is rejected before any HTTP request.
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            "http://localhost:1",
            "table",
            "list",
            "incident",
            "--order-by",
            "sys_created_on:newest",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid sort direction"));
}

// Regression (servicenow-cli-120.2): the structured error carries the
// ServiceNow message and a specific code, and the raw body is not logged.
#[tokio::test]
async fn test_table_list_invalid_table_reports_servicenow_message_only() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/u_does_not_exist"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": {"message": "Invalid table u_does_not_exist", "detail": null},
            "status": "failure"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    let assert = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "table",
            "list",
            "u_does_not_exist",
            "--limit",
            "1",
        ])
        .assert()
        .code(5)
        .stdout("");
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 1, "unexpected stderr: {stderr}");
    let error: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(error["error"]["code"], "INVALID_TABLE");
    assert_eq!(error["error"]["message"], "Invalid table u_does_not_exist");
    assert_eq!(error["error"]["status"], 400);
    assert!(error["error"].get("detail").is_none());
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
        .stdout(predicate::str::contains("\"records\":[]"))
        .stdout(predicate::str::contains("\"returned\":0"))
        .stdout(predicate::str::contains("\"truncated\":false"));
}

#[tokio::test]
async fn test_table_list_default_is_bounded_with_compact_fields() {
    let server = MockServer::start().await;

    // Omitting --limit/--fields must request a bounded page with the
    // compact incident projection — never an unbounded full-field fetch.
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_limit", "20"))
        .and(query_param(
            "sysparm_fields",
            "sys_id,number,short_description,state,priority,assigned_to,sys_updated_on",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Total-Count", "4381")
                .set_body_json(serde_json::json!({
                    "result": [
                        {"sys_id": "6816f79cc0a8016401c5a33be04be441", "number": "INC001"}
                    ]
                })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--output",
            "json",
            "--instance",
            &server.uri(),
            "table",
            "list",
            "incident",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"total\":4381"))
        .stdout(predicate::str::contains("\"returned\":1"))
        .stdout(predicate::str::contains("\"truncated\":true"));
}

#[tokio::test]
async fn test_table_list_all_fetches_every_record() {
    let server = MockServer::start().await;

    // --all removes the bounded default: full page size, auto-pagination.
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_limit", "100"))
        .and(query_param("sysparm_offset", "0"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Total-Count", "2")
                .set_body_json(serde_json::json!({
                    "result": [
                        {"sys_id": "6816f79cc0a8016401c5a33be04be441", "number": "INC001"},
                        {"sys_id": "def456", "number": "INC002"}
                    ]
                })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--output",
            "json",
            "--instance",
            &server.uri(),
            "table",
            "list",
            "incident",
            "--all",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"returned\":2"))
        .stdout(predicate::str::contains("\"truncated\":false"));
}

#[tokio::test]
async fn test_table_list_fields_star_requests_all_fields() {
    let server = MockServer::start().await;

    // --fields '*' opts out of the compact projection entirely.
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param_is_missing("sysparm_fields"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "6816f79cc0a8016401c5a33be04be441", "number": "INC001"}
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
            "--fields",
            "*",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("INC001"));
}

#[tokio::test]
async fn test_table_list_limit_conflicts_with_all() {
    cargo_bin_cmd!("snow-cli")
        .args(["table", "list", "incident", "--limit", "5", "--all"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
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
        .and(path("/api/now/table/incident/6816f79cc0a8016401c5a33be04be441"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {"sys_id": "6816f79cc0a8016401c5a33be04be441", "number": "INC001", "state": "1"}
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
            "6816f79cc0a8016401c5a33be04be441",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("6816f79cc0a8016401c5a33be04be441"))
        .stdout(predicate::str::contains("INC001"));
}

#[tokio::test]
async fn test_table_get_with_fields() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(
            "/api/now/table/incident/6816f79cc0a8016401c5a33be04be441",
        ))
        .and(query_param("sysparm_fields", "sys_id,number"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {"sys_id": "6816f79cc0a8016401c5a33be04be441", "number": "INC001"}
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
            "6816f79cc0a8016401c5a33be04be441",
            "--fields",
            "sys_id,number",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("6816f79cc0a8016401c5a33be04be441"))
        .stdout(predicate::str::contains("INC001"));
}

#[tokio::test]
async fn test_table_get_truncates_long_fields_unless_full() {
    let server = MockServer::start().await;
    let long_description = "x".repeat(2_001);

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident/6816f79cc0a8016401c5a33be04be441"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {"sys_id": "6816f79cc0a8016401c5a33be04be441", "description": long_description}
        })))
        .expect(2)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();
    let run = |full: bool| {
        let mut command = cargo_bin_cmd!("snow-cli");
        command
            .env("SNOW_CLI_CONFIG", &config_path)
            .env("SNOW_CLI_API_TOKEN", "test-api-token")
            .args([
                "--instance",
                &server.uri(),
                "table",
                "get",
                "incident",
                "6816f79cc0a8016401c5a33be04be441",
            ]);
        if full {
            command.arg("--full");
        }
        command.assert().success().get_output().stdout.clone()
    };

    let truncated: serde_json::Value = serde_json::from_slice(&run(false)).unwrap();
    let description = truncated["description"].as_str().unwrap();
    assert!(description.starts_with(&"x".repeat(2_000)));
    assert!(description.ends_with("[truncated 2000 of 2001 chars; use --full]"));

    let full: serde_json::Value = serde_json::from_slice(&run(true)).unwrap();
    assert_eq!(full["description"], "x".repeat(2_001));
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
        .and(path(
            "/api/now/table/incident/6816f79cc0a8016401c5a33be04be441",
        ))
        .and(header("Content-Type", "application/json"))
        .and(body_string_contains("state"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {
                "sys_id": "6816f79cc0a8016401c5a33be04be441",
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
            "6816f79cc0a8016401c5a33be04be441",
            "--data",
            r#"{"state":"2"}"#,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("6816f79cc0a8016401c5a33be04be441"))
        .stdout(predicate::str::contains("\"state\""));
}

// --- table delete ---

#[tokio::test]
async fn test_table_delete_with_yes() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path(
            "/api/now/table/incident/6816f79cc0a8016401c5a33be04be441",
        ))
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
            "6816f79cc0a8016401c5a33be04be441",
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

// --- table schema ---

#[tokio::test]
async fn test_table_schema_compact() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .and(query_param(
            "sysparm_query",
            "name=incident^elementISNOTEMPTY^element!=sys_tags^ORDERBYelement",
        ))
        .and(query_param(
            "sysparm_fields",
            "element,internal_type,column_label,name",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"element": "number", "internal_type": "string", "column_label": "Number", "name": "incident"},
                {"element": "state", "internal_type": "integer", "column_label": "State", "name": "incident"},
                {"element": "short_description", "internal_type": "string", "column_label": "Short description", "name": "incident"}
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
            "schema",
            "incident",
            "--own-only",
        ])
        .assert()
        .success()
        // --own-only skips the hierarchy walk and the per-column table tag.
        .stdout(predicate::str::contains("\"table\"").not())
        .stdout(predicate::str::contains("number"))
        .stdout(predicate::str::contains("string"))
        .stdout(predicate::str::contains("Number"))
        .stdout(predicate::str::contains("state"))
        .stdout(predicate::str::contains("integer"));
}

#[tokio::test]
async fn test_table_schema_extended() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .and(query_param(
            "sysparm_fields",
            "element,internal_type,column_label,mandatory,read_only,display,max_length,default_value,reference,name",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "element": "caller_id",
                    "internal_type": "reference",
                    "column_label": "Caller",
                    "mandatory": "true",
                    "read_only": "false",
                    "display": "false",
                    "max_length": "32",
                    "default_value": "",
                    "reference": "sys_user",
                    "name": "incident"
                }
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
            "schema",
            "incident",
            "--extended",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("caller_id"))
        .stdout(predicate::str::contains("reference"))
        .stdout(predicate::str::contains("sys_user"))
        .stdout(predicate::str::contains("true")); // required=true
}

/// Mount `sys_db_object` rows for the `incident -> task` hierarchy.
async fn mount_incident_hierarchy(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_db_object"))
        .and(query_param("sysparm_query", "name=incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{
                "name": "incident",
                "super_class": {"link": "https://x/api/now/table/sys_db_object/t", "value": "task-sys-id"}
            }]
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_db_object"))
        .and(query_param("sysparm_query", "sys_id=task-sys-id"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"name": "task", "super_class": ""}]
        })))
        .mount(server)
        .await;
}

// Regression (servicenow-cli-88): `nameINSTANCEOF` on sys_dictionary.name is
// an exact match, so inherited columns were never returned. The effective
// schema now walks sys_db_object.super_class and queries `nameIN<chain>`.
#[tokio::test]
async fn test_table_schema_includes_inherited_columns_by_default() {
    for extra_args in [&[][..], &["--include-inherited"][..]] {
        let server = MockServer::start().await;
        mount_incident_hierarchy(&server).await;

        Mock::given(method("GET"))
            .and(path("/api/now/table/sys_dictionary"))
            .and(query_param(
                "sysparm_query",
                "nameINincident,task^elementISNOTEMPTY^element!=sys_tags^ORDERBYelement",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [
                    {"element": "category", "internal_type": "string", "column_label": "Category", "name": "incident"},
                    {"element": "short_description", "internal_type": "string", "column_label": "Short description", "name": "task"},
                    {"element": "state", "internal_type": "integer", "column_label": "Task state", "name": "task"},
                    {"element": "state", "internal_type": "integer", "column_label": "Incident state", "name": "incident"}
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let (_dir, config_path) = api_key_config();

        let uri = server.uri();
        let mut args = vec!["--instance", uri.as_str(), "table", "schema", "incident"];
        args.extend_from_slice(extra_args);

        let assert = cargo_bin_cmd!("snow-cli")
            .env("SNOW_CLI_CONFIG", &config_path)
            .env("SNOW_CLI_API_TOKEN", "test-api-token")
            .args(&args)
            .assert()
            .success();
        let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
        let columns: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap();
        let summary: Vec<(&str, &str, &str)> = columns
            .iter()
            .map(|c| {
                (
                    c["column"].as_str().unwrap(),
                    c["table"].as_str().unwrap(),
                    c["label"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("category", "incident", "Category"),
                ("short_description", "task", "Short description"),
                // The incident override wins over the task definition.
                ("state", "incident", "Incident state"),
            ],
            "args: {extra_args:?}"
        );
    }
}

#[tokio::test]
async fn test_table_schema_own_only_conflicts_with_include_inherited() {
    cargo_bin_cmd!("snow-cli")
        .args([
            "table",
            "schema",
            "incident",
            "--own-only",
            "--include-inherited",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[tokio::test]
async fn test_table_schema_handles_link_object_internal_type() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .and(query_param(
            "sysparm_query",
            "name=incident^elementISNOTEMPTY^element!=sys_tags^ORDERBYelement",
        ))
        .and(query_param(
            "sysparm_fields",
            "element,internal_type,column_label,name",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "element": "caller_id",
                    "internal_type": {
                        "link": "https://example.service-now.com/api/now/table/sys_glide_object?name=reference",
                        "value": "reference"
                    },
                    "column_label": "Caller",
                    "name": "incident"
                },
                {
                    "element": "number",
                    "internal_type": {
                        "link": "https://example.service-now.com/api/now/table/sys_glide_object?name=string",
                        "value": "string"
                    },
                    "column_label": "Number",
                    "name": "incident"
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "table", "schema", "incident"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"column\":\"caller_id\",\"type\":\"reference\"",
        ))
        .stdout(predicate::str::contains(
            "\"column\":\"number\",\"type\":\"string\"",
        ));
}

#[tokio::test]
async fn test_table_schema_csv_output() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"element": "sys_id", "internal_type": "GUID", "column_label": "Sys ID", "name": "incident"},
                {"element": "number", "internal_type": "string", "column_label": "Number", "name": "incident"}
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
            "schema",
            "incident",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("column,type,label"))
        .stdout(predicate::str::contains("sys_id,GUID,Sys ID"))
        .stdout(predicate::str::contains("number,string,Number"));
}

// --- table stats ---

#[tokio::test]
async fn test_table_stats_ungrouped_count() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/stats/incident"))
        .and(query_param("sysparm_count", "true"))
        .and(query_param("sysparm_query", "active=true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {"stats": {"count": "4381"}}
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
            "stats",
            "incident",
            "--query",
            "active=true",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#"{"count":4381}"#));
}

#[tokio::test]
async fn test_table_stats_grouped_with_aggregates() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/stats/incident"))
        .and(query_param("sysparm_count", "true"))
        .and(query_param("sysparm_group_by", "state"))
        .and(query_param("sysparm_avg_fields", "priority"))
        .and(query_param("sysparm_having", "count>2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "stats": {"count": "8", "avg": {"priority": "2.5"}},
                    "groupby_fields": [{"field": "state", "value": "1"}]
                },
                {
                    "stats": {"count": "3", "avg": {"priority": "1.5"}},
                    "groupby_fields": [{"field": "state", "value": "2"}]
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    let assert = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "table",
            "stats",
            "incident",
            "--group-by",
            "state",
            "--avg",
            "priority",
            "--having",
            "count>2",
        ])
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let rows: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        rows,
        serde_json::json!([
            {"state": "1", "count": 8, "avg_priority": 2.5},
            {"state": "2", "count": 3, "avg_priority": 1.5}
        ])
    );
}

#[tokio::test]
async fn test_table_stats_grouped_csv_output() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/stats/incident"))
        .and(query_param("sysparm_count", "true"))
        .and(query_param("sysparm_group_by", "state"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "stats": {"count": "8"},
                    "groupby_fields": [{"field": "state", "value": "1"}]
                }
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
            "stats",
            "incident",
            "--group-by",
            "state",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("count,state"))
        .stdout(predicate::str::contains("8,1"));
}

#[tokio::test]
async fn test_table_stats_invalid_table_name() {
    let (_dir, config_path) = api_key_config();

    // No mock needed — the invalid table name is rejected before the HTTP request.
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            "http://localhost:1",
            "table",
            "stats",
            "incident/../oops",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid table name"));
}

#[tokio::test]
async fn test_table_schema_empty_result() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": []})))
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
            "schema",
            "nonexistent_table",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("No columns found"));
}
