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

    let (_dir, config_path) = api_key_config(&server.uri());

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
async fn test_table_list_piped_stderr_has_plain_tracing_output() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": []})))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config(&server.uri());
    let output = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .env_remove("NO_COLOR")
        .args([
            "-v",
            "--instance",
            &server.uri(),
            "table",
            "list",
            "incident",
        ])
        .assert()
        .success()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(output).unwrap();
    assert!(stderr.contains("Listing records from table"), "{stderr}");
    assert!(!stderr.contains("\x1b["), "{stderr}");
}

#[tokio::test]
async fn test_table_list_previews_long_fields_with_preview_flag() {
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

    let (_dir, config_path) = api_key_config(&server.uri());
    let run = |full: bool| {
        let mut command = cargo_bin_cmd!("snow-cli");
        command
            .env("SNOW_CLI_CONFIG", &config_path)
            .env("SNOW_CLI_API_TOKEN", "test-api-token")
            .args(["--instance", &server.uri(), "table", "list", "incident"]);
        if full {
            command.arg("--full");
        } else {
            command.arg("--preview");
        }
        command.assert().success().get_output().stdout.clone()
    };

    let previewed: serde_json::Value = serde_json::from_slice(&run(false)).unwrap();
    assert_eq!(previewed["fields_truncated"], true);
    assert_eq!(previewed["records"][0]["description"], "x".repeat(2_000));
    assert_eq!(
        previewed["preview"]["truncated_fields"],
        serde_json::json!([{
            "path": "/records/0/description",
            "original_chars": 2001,
            "shown_chars": 2000,
            "escape_hatch": "rerun this command with --full"
        }])
    );

    let full: serde_json::Value = serde_json::from_slice(&run(true)).unwrap();
    assert!(full.get("fields_truncated").is_none());
    assert!(full.get("preview").is_none());
    assert_eq!(full["records"][0]["description"], "x".repeat(2_001));
}

#[tokio::test]
async fn test_table_list_explicit_json_is_lossless_without_full_or_preview() {
    let server = MockServer::start().await;
    let long_description = format!("{}END", "é".repeat(2_000));

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"sys_id": "6816f79cc0a8016401c5a33be04be441", "description": long_description}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config(&server.uri());
    let output = cargo_bin_cmd!("snow-cli")
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
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();

    assert!(json.get("preview").is_none());
    assert!(json.get("fields_truncated").is_none());
    assert_eq!(json["records"][0]["description"], long_description);
}

#[tokio::test]
async fn test_table_list_csv_output_is_lossless_for_long_fields() {
    let server = MockServer::start().await;
    let long_description = "x".repeat(2_001);

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"sys_id": "6816f79cc0a8016401c5a33be04be441", "description": long_description}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config(&server.uri());
    let output = cargo_bin_cmd!("snow-cli")
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
        .get_output()
        .stdout
        .clone();
    let csv = String::from_utf8(output).unwrap();
    assert!(csv.contains(&"x".repeat(2_001)));
    assert!(!csv.contains("preview"));
    assert!(!csv.contains("truncated"));
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

    let (_dir, config_path) = api_key_config(&server.uri());

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

    let (_dir, config_path) = api_key_config(&server.uri());

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
async fn test_table_list_jsonl_preview_uses_metadata_envelope() {
    let server = MockServer::start().await;
    let long_description = "x".repeat(2_001);

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"sys_id": "6816f79cc0a8016401c5a33be04be441", "description": long_description}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config(&server.uri());
    let output = cargo_bin_cmd!("snow-cli")
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
            "--preview",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    let lines: Vec<&str> = stdout.trim().split('\n').collect();
    assert_eq!(lines.len(), 2);
    let header: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(header["meta"]["fields_truncated"], true);
    assert_eq!(
        header["meta"]["preview"]["truncated_fields"][0]["path"],
        "/records/0/description"
    );
    let record: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(record["description"], "x".repeat(2_000));
}

#[tokio::test]
async fn test_table_list_preview_rejects_csv_output() {
    let (_dir, config_path) = api_key_config("http://127.0.0.1:9");

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--output",
            "csv",
            "--instance",
            "http://127.0.0.1:9",
            "table",
            "list",
            "incident",
            "--preview",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--preview is not supported with --output csv",
        ));
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

    let (_dir, config_path) = api_key_config(&server.uri());

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
        // The query field is added to verify it exists, then removed from output.
        .and(query_param("sysparm_fields", "sys_id,number,active"))
        .and(query_param_is_missing("sysparm_orderby"))
        .and(query_param("sysparm_offset", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "6816f79cc0a8016401c5a33be04be441", "number": "INC001", "active": "true"}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config(&server.uri());

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
        .stdout(predicate::str::contains("INC001"))
        .stdout(predicate::str::contains("\"active\"").not())
        .stderr(predicate::str::is_empty());
}

#[tokio::test]
async fn test_table_list_offset_returns_next_page_and_cursor() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_offset", "3"))
        .and(query_param("sysparm_limit", "3"))
        .and(query_param("sysparm_query", "ORDERBYDESCsys_created_on"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Total-Count", "7")
                .set_body_json(serde_json::json!({"result": [
                    {"number": "INC004"}, {"number": "INC005"}, {"number": "INC006"}
                ]})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config(&server.uri());
    let output = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "table",
            "list",
            "incident",
            "--order-by",
            "-sys_created_on",
            "--limit",
            "3",
            "--offset",
            "3",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["total"], 7);
    assert_eq!(value["returned"], 3);
    assert_eq!(value["offset"], 3);
    assert_eq!(value["next_offset"], 6);
    assert_eq!(value["truncated"], true);
    assert_eq!(value["records"][0]["number"], "INC004");
}

#[tokio::test]
async fn test_table_list_offset_at_end_has_no_next_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_offset", "6"))
        .and(query_param("sysparm_limit", "3"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Total-Count", "7")
                .set_body_json(serde_json::json!({"result": [{"number": "INC007"}]})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config(&server.uri());
    let output = cargo_bin_cmd!("snow-cli-ro")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "table",
            "list",
            "incident",
            "--limit",
            "3",
            "--offset",
            "6",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["offset"], 6);
    assert_eq!(value["next_offset"], serde_json::Value::Null);
    assert_eq!(value["truncated"], false);
    assert_eq!(value["returned"], 1);
}

#[tokio::test]
async fn test_table_list_all_can_start_at_offset() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_offset", "5"))
        .and(query_param("sysparm_limit", "100"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Total-Count", "6")
                .set_body_json(serde_json::json!({"result": [{"number": "INC006"}]})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config(&server.uri());
    let output = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "table",
            "list",
            "incident",
            "--all",
            "--offset",
            "5",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["offset"], 5);
    assert_eq!(value["next_offset"], serde_json::Value::Null);
    assert_eq!(value["truncated"], false);
    assert_eq!(value["returned"], 1);
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

    let (_dir, config_path) = api_key_config(&server.uri());

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
    let (_dir, config_path) = api_key_config("http://localhost:1");

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

    let (_dir, config_path) = api_key_config(&server.uri());

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

    let (_dir, config_path) = api_key_config(&server.uri());

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

    let (_dir, config_path) = api_key_config(&server.uri());

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

    let (_dir, config_path) = api_key_config(&server.uri());

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

    let (_dir, config_path) = api_key_config(&server.uri());

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

    let (_dir, config_path) = api_key_config(&server.uri());

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

    let (_dir, config_path) = api_key_config(&server.uri());

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

    let (_dir, config_path) = api_key_config(&server.uri());

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
async fn test_table_get_previews_long_fields_with_preview_flag() {
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

    let (_dir, config_path) = api_key_config(&server.uri());
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
        } else {
            command.arg("--preview");
        }
        command.assert().success().get_output().stdout.clone()
    };

    let previewed: serde_json::Value = serde_json::from_slice(&run(false)).unwrap();
    assert_eq!(previewed["record"]["description"], "x".repeat(2_000));
    assert_eq!(
        previewed["preview"]["truncated_fields"],
        serde_json::json!([{
            "path": "/record/description",
            "original_chars": 2001,
            "shown_chars": 2000,
            "escape_hatch": "rerun this command with --full"
        }])
    );

    let full: serde_json::Value = serde_json::from_slice(&run(true)).unwrap();
    assert_eq!(full["description"], "x".repeat(2_001));
}

#[tokio::test]
async fn test_table_get_preview_keeps_stable_envelope_without_truncation() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(
            "/api/now/table/incident/6816f79cc0a8016401c5a33be04be441",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {"sys_id": "6816f79cc0a8016401c5a33be04be441", "description": "short"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config(&server.uri());
    let output = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "table",
            "get",
            "incident",
            "6816f79cc0a8016401c5a33be04be441",
            "--preview",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let previewed: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(previewed["record"]["description"], "short");
    assert_eq!(
        previewed["preview"]["truncated_fields"],
        serde_json::json!([])
    );
}

#[tokio::test]
async fn test_table_get_explicit_json_detail_is_lossless_without_full_or_preview() {
    let server = MockServer::start().await;
    let long_description = format!("{}END", "雪".repeat(2_000));

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident/6816f79cc0a8016401c5a33be04be441"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {"sys_id": "6816f79cc0a8016401c5a33be04be441", "description": long_description}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config(&server.uri());
    let output = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--output",
            "json",
            "--instance",
            &server.uri(),
            "table",
            "get",
            "incident",
            "6816f79cc0a8016401c5a33be04be441",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();

    assert!(json.get("preview").is_none());
    assert!(json.get("record").is_none());
    assert_eq!(json["description"], long_description);
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

    let (_dir, config_path) = api_key_config(&server.uri());

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

    let (_dir, config_path) = api_key_config(&server.uri());

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

    let (_dir, config_path) = api_key_config(&server.uri());

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

    let (_dir, config_path) = api_key_config(&server.uri());

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

    let (_dir, config_path) = api_key_config(&server.uri());

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
    let (_dir, config_path) = api_key_config("http://localhost:1");

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

    let (_dir, config_path) = api_key_config(&server.uri());

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

    let (_dir, config_path) = api_key_config(&server.uri());

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

        let (_dir, config_path) = api_key_config(&server.uri());

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

    let (_dir, config_path) = api_key_config(&server.uri());

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

    let (_dir, config_path) = api_key_config(&server.uri());

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

#[tokio::test]
async fn test_table_schema_extended_csv_output_aligns_optional_columns_and_neutralizes_formulas() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "element": "short_description",
                    "internal_type": "string",
                    "column_label": "=HYPERLINK(\"http://evil\")",
                    "mandatory": "false",
                    "read_only": "false",
                    "display": "false",
                    "max_length": "160",
                    "default_value": "",
                    "reference": "",
                    "name": "incident"
                },
                {
                    "element": "caller_id",
                    "internal_type": "reference",
                    "column_label": "Caller",
                    "mandatory": "true",
                    "read_only": "false",
                    "display": "false",
                    "max_length": "32",
                    "default_value": "+cmd",
                    "reference": "sys_user",
                    "name": "incident"
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config(&server.uri());

    let output = cargo_bin_cmd!("snow-cli")
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
            "--extended",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "{stdout}");
    assert_eq!(
        lines[0],
        "column,type,label,table,required,read_only,display,max_length,default_value,reference"
    );
    assert!(
        stdout.contains(
            "short_description,string,\"'=HYPERLINK(\"\"http://evil\"\")\",incident,false,false,false,160,,"
        ),
        "{stdout}"
    );
    assert!(
        stdout.contains("caller_id,reference,Caller,incident,true,false,false,32,'+cmd,sys_user"),
        "{stdout}"
    );
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

    let (_dir, config_path) = api_key_config(&server.uri());

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
        .and(query_param("sysparm_having", "count^*^>^2"))
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

    let (_dir, config_path) = api_key_config(&server.uri());

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

    let (_dir, config_path) = api_key_config(&server.uri());

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
async fn test_table_stats_rejects_invalid_having_before_request() {
    let (_dir, config_path) = api_key_config("http://localhost:1");

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            "http://localhost:1",
            "table",
            "stats",
            "incident",
            "--group-by",
            "state",
            "--having",
            "median(priority)>2",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Invalid --having clause"));
}

#[tokio::test]
async fn test_table_stats_invalid_table_name() {
    let (_dir, config_path) = api_key_config("http://localhost:1");

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

    let (_dir, config_path) = api_key_config(&server.uri());

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

#[tokio::test]
async fn test_instance_override_to_foreign_origin_never_receives_credentials() {
    let profile_server = MockServer::start().await;
    let foreign = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&foreign)
        .await;

    let (_dir, config_path) = api_key_config(&profile_server.uri());

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &foreign.uri(), "table", "list", "incident"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("INSTANCE_MISMATCH"))
        .stderr(predicate::str::contains("test-api-token").not());
}

// --- table find / choices ---

#[tokio::test]
async fn test_table_find_searches_name_and_label_bounded() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_db_object"))
        .and(query_param(
            "sysparm_query",
            "nameLIKEflow^ORlabelLIKEflow^ORDERBYname",
        ))
        .and(query_param(
            "sysparm_fields",
            "name,label,super_class.name,sys_scope.scope",
        ))
        .and(query_param("sysparm_limit", "20"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Total-Count", "42")
                .set_body_json(serde_json::json!({
                    "result": [{
                        "name": "sys_hub_flow", "label": "Flow",
                        "super_class.name": "sys_hub_flow_base", "sys_scope.scope": "global"
                    }]
                })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config(&server.uri());

    let assert = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "table", "find", "flow"])
        .assert()
        .success();
    let stdout: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(stdout["total"], 42);
    assert_eq!(stdout["truncated"], true);
    assert_eq!(
        stdout["records"][0],
        serde_json::json!({
            "name": "sys_hub_flow", "label": "Flow",
            "extends": "sys_hub_flow_base", "scope": "global"
        })
    );
}

#[test]
fn test_table_find_rejects_query_operators() {
    let (_dir, config_path) = api_key_config("http://localhost:1");

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            "http://localhost:1",
            "table",
            "find",
            "a^ORactive=true",
        ])
        .assert()
        .code(2);
}

#[tokio::test]
async fn test_table_choices_walks_hierarchy_and_prefers_most_specific_table() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_db_object"))
        .and(query_param("sysparm_query", "name=incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"name": "incident", "super_class": {"value": "taskid", "link": "x"}}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_db_object"))
        .and(query_param("sysparm_query", "sys_id=taskid"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"name": "task", "super_class": ""}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_choice"))
        .and(query_param(
            "sysparm_query",
            "nameINincident,task^inactive=false^language=en^ORDERBYelement^ORDERBYsequence",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"name": "task", "element": "priority", "value": "1", "label": "1 - Critical", "sequence": "1", "dependent_value": ""},
                {"name": "incident", "element": "state", "value": "1", "label": "New", "sequence": "1", "dependent_value": ""},
                {"name": "task", "element": "state", "value": "-5", "label": "Pending", "sequence": "1", "dependent_value": ""}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config(&server.uri());

    let assert = cargo_bin_cmd!("snow-cli-ro")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "table", "choices", "incident"])
        .assert()
        .success();
    let rows: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(
        rows,
        serde_json::json!([
            {"table": "task", "element": "priority", "value": "1", "label": "1 - Critical", "sequence": "1"},
            {"table": "incident", "element": "state", "value": "1", "label": "New", "sequence": "1"}
        ])
    );
}

// --- display values, field checks, name lookup (agent ergonomics) ---

fn bin_cmd(binary: &str) -> assert_cmd::Command {
    match binary {
        "snow-cli-ro" => cargo_bin_cmd!("snow-cli-ro"),
        _ => cargo_bin_cmd!("snow-cli"),
    }
}

fn run_ok(
    binary: &str,
    server: &MockServer,
    config: &std::path::Path,
    args: &[&str],
) -> std::process::Output {
    bin_cmd(binary)
        .env("SNOW_CLI_CONFIG", config)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .env_remove("SNOW_CLI_CHECK_FIELDS")
        .env_remove("SNOW_CLI_OUTPUT")
        .arg("--instance")
        .arg(server.uri())
        .args(args)
        .output()
        .unwrap()
}

#[tokio::test]
async fn test_table_list_display_value_both_compacts_pairs_in_json_and_splits_in_csv() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_display_value", "all"))
        .and(query_param("sysparm_exclude_reference_link", "true"))
        .and(query_param(
            "sysparm_fields",
            "number,priority,assignment_group",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Total-Count", "1")
                .set_body_json(serde_json::json!({"result": [{
                    "number": {"display_value": "INC0010025", "value": "INC0010025"},
                    "priority": {"display_value": "4 - Low", "value": "4"},
                    "assignment_group": {"display_value": "", "value": ""}
                }]})),
        )
        .expect(2)
        .mount(&server)
        .await;
    let (_dir, config) = api_key_config(&server.uri());
    let list = [
        "table",
        "list",
        "incident",
        "--fields",
        "number,priority,assignment_group",
        "--display-value",
        "both",
    ];

    let output = run_ok("snow-cli", &server, &config, &list);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["total"], 1);
    assert_eq!(
        value["records"][0],
        serde_json::json!({
            "assignment_group": "",
            "number": "INC0010025",
            "priority": {"value": "4", "display": "4 - Low"}
        })
    );
    assert!(value.get("warnings").is_none());

    let mut csv_args = list.to_vec();
    csv_args.extend(["--output", "csv"]);
    let output = run_ok("snow-cli-ro", &server, &config, &csv_args);
    assert!(output.status.success(), "{output:?}");
    let csv = String::from_utf8(output.stdout).unwrap();
    assert!(
        csv.starts_with("assignment_group,number,priority,priority.display\n"),
        "{csv}"
    );
    assert!(csv.contains(",INC0010025,4,4 - Low"), "{csv}");
}

#[tokio::test]
async fn test_table_list_display_value_both_uniform_in_toon_auto_and_truncated_in_preview() {
    let server = MockServer::start().await;
    let long_raw = "r".repeat(2_005);
    let long_disp = "d".repeat(2_005);
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_display_value", "all"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Total-Count", "2")
                .set_body_json(serde_json::json!({"result": [
                    {
                        "number": {"display_value": "INC1", "value": "INC1"},
                        "assignment_group": {"display_value": "Network", "value": "grp1"},
                        "description": {"display_value": long_disp, "value": long_raw}
                    },
                    {
                        "number": {"display_value": "INC2", "value": "INC2"},
                        "assignment_group": {"display_value": "", "value": ""},
                        "description": {"display_value": "ok", "value": "ok"}
                    }
                ]})),
        )
        .expect(3)
        .mount(&server)
        .await;
    let (_dir, config) = api_key_config(&server.uri());

    // 1. JSON + --preview truncates inside {"value","display"} pairs.
    let output = run_ok(
        "snow-cli",
        &server,
        &config,
        &[
            "table",
            "list",
            "incident",
            "--fields",
            "number,assignment_group,description",
            "--display-value",
            "both",
            "--preview",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["fields_truncated"], true);
    assert_eq!(
        json["records"][0]["description"]["value"],
        "r".repeat(2_000)
    );
    assert_eq!(
        json["records"][0]["description"]["display"],
        "d".repeat(2_000)
    );

    // 2. TOON uses compact tabular encoding [2]{...}: because split_both_columns_uniform
    //    normalizes assignment_group.display and description.display across all rows.
    let output = run_ok(
        "snow-cli",
        &server,
        &config,
        &[
            "--output",
            "toon",
            "table",
            "list",
            "incident",
            "--fields",
            "number,assignment_group,description",
            "--display-value",
            "both",
            "--preview",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let toon = String::from_utf8(output.stdout).unwrap();
    assert!(
        toon.contains("records[2]{assignment_group,assignment_group.display,description,description.display,number}:"),
        "expected uniform TOON table header, got:\n{toon}"
    );

    // 3. Auto also splits both-columns uniformly so TOON can encode tabular rows.
    let output = run_ok(
        "snow-cli",
        &server,
        &config,
        &[
            "--output",
            "auto",
            "table",
            "list",
            "incident",
            "--fields",
            "number,assignment_group,description",
            "--display-value",
            "both",
            "--preview",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let auto_out = String::from_utf8(output.stdout).unwrap();
    assert!(auto_out.contains("assignment_group.display"), "{auto_out}");
}

#[tokio::test]
async fn test_table_list_bare_display_value_sends_true_and_default_sends_nothing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_display_value", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"number": "INC1", "priority": "4 - Low"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param_is_missing("sysparm_display_value"))
        .and(query_param_is_missing("sysparm_exclude_reference_link"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"number": "INC1", "priority": "4"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run_ok(
        "snow-cli",
        &server,
        &config,
        &[
            "table",
            "list",
            "incident",
            "--fields",
            "number,priority",
            "--display-value",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["records"][0]["priority"], "4 - Low");

    let output = run_ok(
        "snow-cli",
        &server,
        &config,
        &["table", "list", "incident", "--fields", "number,priority"],
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["records"][0]["priority"], "4");
}

#[tokio::test]
async fn test_table_list_warns_on_unknown_query_and_fields_without_extra_request() {
    let server = MockServer::start().await;
    // Query fields are added to the projection, so one request verifies them.
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param(
            "sysparm_fields",
            "number,extends,active,assigned_two.name",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Total-Count", "48")
                .set_body_json(serde_json::json!({"result": [
                    {"number": "INC1", "active": "true"}
                ]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run_ok(
        "snow-cli-ro",
        &server,
        &config,
        &[
            "table",
            "list",
            "incident",
            "--fields",
            "number,extends",
            "--query",
            "active=true^assigned_two.name=Abel Tuter",
            "--limit",
            "1",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    // Fields added only for checking are not printed.
    assert_eq!(value["records"][0], serde_json::json!({"number": "INC1"}));
    let warnings = value["warnings"].as_array().unwrap();
    let fields: Vec<_> = warnings
        .iter()
        .map(|w| w["field"].as_str().unwrap())
        .collect();
    assert_eq!(fields, ["extends", "assigned_two.name"]);
    assert_eq!(warnings[1]["in"], "query");
    assert_eq!(warnings[1]["code"], "UNKNOWN_FIELD");

    let stderr = String::from_utf8(output.stderr).unwrap();
    let lines: Vec<serde_json::Value> = stderr
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "{stderr}");
    assert_eq!(lines[1]["warning"]["field"], "assigned_two.name");
    // The query value never appears in diagnostics.
    assert!(!stderr.contains("Abel Tuter"));
}

#[tokio::test]
async fn test_table_list_check_fields_error_fails_before_main_query() {
    let server = MockServer::start().await;
    // Probe: one row, no query, projected to the checked fields.
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param_is_missing("sysparm_query"))
        .and(query_param("sysparm_limit", "1"))
        .and(query_param("sysparm_fields", "active,assigned_two.name"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"active": "true"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param(
            "sysparm_query",
            "active=true^assigned_two.name=Abel Tuter",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": []})))
        .expect(0)
        .mount(&server)
        .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run_ok(
        "snow-cli",
        &server,
        &config,
        &[
            "table",
            "list",
            "incident",
            "--query",
            "active=true^assigned_two.name=Abel Tuter",
            "--check-fields",
            "error",
        ],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "VALIDATION_ERROR");
    assert_eq!(error["error"]["issues"][0]["kind"], "unknown_field");
    assert_eq!(error["error"]["issues"][0]["field"], "assigned_two.name");
}

#[tokio::test]
async fn test_table_list_check_fields_off_and_empty_result_probe() {
    let server = MockServer::start().await;
    // Main query returns nothing, so warn mode probes one unfiltered row.
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_query", "bogus_field=1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": []})))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param_is_missing("sysparm_query"))
        .and(query_param("sysparm_fields", "bogus_field"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config) = api_key_config(&server.uri());
    let args = [
        "table",
        "list",
        "incident",
        "--fields",
        "*",
        "--query",
        "bogus_field=1",
    ];

    let output = run_ok("snow-cli", &server, &config, &args);
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["warnings"][0]["field"], "bogus_field");

    let mut off = args.to_vec();
    off.extend(["--check-fields", "off"]);
    let output = run_ok("snow-cli", &server, &config, &off);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value.get("warnings").is_none());
}

#[tokio::test]
async fn test_table_list_probe_failure_never_blocks_the_read() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_query", "active=true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": []})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param_is_missing("sysparm_query"))
        .respond_with(ResponseTemplate::new(500).set_body_string("<html>gateway</html>"))
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run_ok(
        "snow-cli",
        &server,
        &config,
        &["table", "list", "incident", "--query", "active=true"],
    );
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["returned"], 0);
    assert!(value.get("warnings").is_none());
}

#[tokio::test]
async fn test_table_get_by_name_raw_prints_unescaped_field() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_script_include"))
        .and(query_param(
            "sysparm_query",
            "name=ChangeRequestStateHandler",
        ))
        .and(query_param("sysparm_fields", "script,sys_id,sys_scope"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{
                "script": "var A = Class.create();\n\tA.prototype = {\"q\": 1};",
                "sys_id": "7d12a0e0c0a8016401c5a33be04be441",
                "sys_scope": {"link": "x", "value": "global"}
            }]
        })))
        .expect(2)
        .mount(&server)
        .await;
    let (_dir, config) = api_key_config(&server.uri());
    let args = [
        "table",
        "get",
        "sys_script_include",
        "ChangeRequestStateHandler",
        "--fields",
        "script",
    ];

    let mut raw = args.to_vec();
    raw.push("--raw");
    let output = run_ok("snow-cli-ro", &server, &config, &raw);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "var A = Class.create();\n\tA.prototype = {\"q\": 1};\n"
    );

    // Without --raw the record holds only the requested field.
    let output = run_ok("snow-cli", &server, &config, &args);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 1, "{value}");
}

#[tokio::test]
async fn test_table_get_by_name_not_found_and_ambiguous() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_script_include"))
        .and(query_param("sysparm_query", "name=Missing"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": []})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_script_include"))
        .and(query_param("sysparm_query", "name=Twice"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": [
            {"sys_id": "11111111111111111111111111111111", "sys_scope": "global"},
            {"sys_id": "22222222222222222222222222222222", "sys_scope": {"value": "x_app", "link": "l"}}
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_query", "number=INC0010001"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": [
                {"sys_id": "33333333333333333333333333333333", "number": "INC0010001"}
            ]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run_ok(
        "snow-cli",
        &server,
        &config,
        &["table", "get", "sys_script_include", "Missing"],
    );
    assert_eq!(output.status.code(), Some(4), "{output:?}");
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "RECORD_NOT_FOUND");

    let output = run_ok(
        "snow-cli",
        &server,
        &config,
        &["table", "get", "sys_script_include", "Twice"],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["issues"].as_array().unwrap().len(), 2);
    assert_eq!(
        error["error"]["issues"][1]["field"],
        "22222222222222222222222222222222"
    );

    // Task numbers are matched on `number` by default.
    let output = run_ok(
        "snow-cli",
        &server,
        &config,
        &["table", "get", "incident", "INC0010001"],
    );
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["number"], "INC0010001");
}

#[tokio::test]
async fn test_table_get_by_sys_id_display_value_and_field_warning() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(
            "/api/now/table/incident/6816f79cc0a8016401c5a33be04be441",
        ))
        .and(query_param("sysparm_display_value", "true"))
        .and(query_param("sysparm_exclude_reference_link", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {"number": "INC1", "priority": "4 - Low"}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run_ok(
        "snow-cli",
        &server,
        &config,
        &[
            "table",
            "get",
            "incident",
            "6816f79cc0a8016401c5a33be04be441",
            "--fields",
            "number,priority,bogus",
            "--display-value",
            "display",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["priority"], "4 - Low");
    let warning: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(warning["warning"]["field"], "bogus");
    assert_eq!(warning["warning"]["in"], "fields");
}

#[tokio::test]
async fn test_table_stats_warns_on_unknown_group_by() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_fields", "active,stat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"active": "true"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/stats/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {"stats": {"count": "48"}}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run_ok(
        "snow-cli-ro",
        &server,
        &config,
        &[
            "table",
            "stats",
            "incident",
            "--query",
            "active=true",
            "--group-by",
            "stat",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let warning: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(warning["warning"]["field"], "stat");
    assert_eq!(warning["warning"]["in"], "group_by");
}

#[tokio::test]
async fn test_table_list_star_fields_checks_dot_walked_query_fields() {
    let server = MockServer::start().await;
    // Main query with --fields '*' sends no sysparm_fields and returns top-level fields.
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_query", "assigned_to.bogus=x"))
        .and(query_param_is_missing("sysparm_fields"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"sys_id": "11111111111111111111111111111111", "assigned_to": "abc"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    // Warn mode probes only the dot-walked path so assigned_to.bogus is caught.
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param_is_missing("sysparm_query"))
        .and(query_param("sysparm_limit", "1"))
        .and(query_param("sysparm_fields", "assigned_to.bogus"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run_ok(
        "snow-cli-ro",
        &server,
        &config,
        &[
            "table",
            "list",
            "incident",
            "--fields",
            " * ",
            "--query",
            "assigned_to.bogus=x",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["warnings"][0]["field"], "assigned_to.bogus");
    assert_eq!(value["warnings"][0]["in"], "query");
}

#[tokio::test]
async fn test_table_list_escaped_caret_query_passes_check_fields_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param_is_missing("sysparm_query"))
        .and(query_param("sysparm_limit", "1"))
        .and(query_param("sysparm_fields", "short_description"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"short_description": "a^b"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_query", "short_descriptionLIKEa^^b"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"sys_id": "11111111111111111111111111111111", "short_description": "a^b"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run_ok(
        "snow-cli-ro",
        &server,
        &config,
        &[
            "table",
            "list",
            "incident",
            "--query",
            "short_descriptionLIKEa^^b",
            "--check-fields",
            "error",
        ],
    );
    assert!(output.status.success(), "{output:?}");
}

#[tokio::test]
async fn test_table_stats_checks_aggregate_and_having_fields_with_domain_probe() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param("sysparm_fields", "bogus_avg,bogus_having"))
        .and(query_param("sysparm_query_no_domain", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_db_object"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"name": "incident", "super_class": ""}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"name": "incident", "element": "sys_id"},
                {"name": "incident", "element": "sys_domain"}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/stats/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {"stats": {"count": "12"}}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run_ok(
        "snow-cli-ro",
        &server,
        &config,
        &[
            "table",
            "stats",
            "incident",
            "--domain",
            "global",
            "--avg",
            "bogus_avg",
            "--having",
            "sum(bogus_having)>1",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    let lines: Vec<serde_json::Value> = stderr
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "{stderr}");
    assert_eq!(lines[0]["warning"]["field"], "bogus_avg");
    assert_eq!(lines[0]["warning"]["in"], "avg");
    assert_eq!(lines[1]["warning"]["field"], "bogus_having");
    assert_eq!(lines[1]["warning"]["in"], "having");
}

#[test]
fn test_table_help_lists_agent_recipes() {
    for binary in ["snow-cli", "snow-cli-ro"] {
        let output = bin_cmd(binary).args(["table", "--help"]).output().unwrap();
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("Recipes for agents"), "{binary}: {help}");
        assert!(help.contains("--display-value"), "{binary}");
        assert!(help.contains("include show"), "{binary}");
        let output = bin_cmd(binary)
            .args(["table", "list", "--help"])
            .output()
            .unwrap();
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("--display-value"), "{binary}");
        assert!(help.contains("--check-fields"), "{binary}");
    }
}
