#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Wiremock-backed integration tests for the codesearch command.
//!
//! Tests exercise the full CLI binary against a wiremock MockServer.
//! The real ServiceNow endpoint is:
//!   GET /api/sn_codesearch/code_search/search?term=...&limit=...&search_all_scopes=...&search_group=...&table=...

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Create a temp config file for api_key auth.
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
async fn test_codesearch_basic() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/sn_codesearch/code_search/search"))
        .and(query_param("term", "GlideRecord"))
        .and(query_param("search_all_scopes", "true"))
        .and(query_param(
            "search_group",
            "sn_devstudio.Studio Search Group",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "sys_id": "abc123",
                    "name": "MyScriptInclude",
                    "type": "sys_script_include",
                    "match_text": "var gr = new GlideRecord('incident');"
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
            "codesearch",
            "search",
            "GlideRecord",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("MyScriptInclude"))
        .stdout(predicate::str::contains("GlideRecord"));
}

#[tokio::test]
async fn test_codesearch_with_table_filter() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/sn_codesearch/code_search/search"))
        .and(query_param("term", "incident"))
        .and(query_param("table", "sys_script_include"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "sys_id": "si001",
                    "name": "IncidentUtils",
                    "type": "sys_script_include"
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
            "codesearch",
            "search",
            "incident",
            "--source-table",
            "sys_script_include",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("IncidentUtils"))
        .stdout(predicate::str::contains("sys_script_include"));
}

#[tokio::test]
async fn test_codesearch_with_custom_limit() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/sn_codesearch/code_search/search"))
        .and(query_param("term", "myfunction"))
        .and(query_param("limit", "500"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "s1", "name": "Script1"},
                {"sys_id": "s2", "name": "Script2"}
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
            "codesearch",
            "search",
            "myfunction",
            "--limit",
            "500",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Script1"))
        .stdout(predicate::str::contains("Script2"));
}

#[tokio::test]
async fn test_codesearch_empty_result() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/sn_codesearch/code_search/search"))
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
            "codesearch",
            "search",
            "nonexistent_xyz",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("[]"));
}

#[tokio::test]
async fn test_codesearch_csv_output() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/sn_codesearch/code_search/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "abc123", "name": "TestScript", "type": "sys_script_include"}
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
            "codesearch",
            "search",
            "test",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("name"))
        .stdout(predicate::str::contains("TestScript"));
}

#[tokio::test]
async fn test_codesearch_server_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/sn_codesearch/code_search/search"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "codesearch", "search", "test"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("SERVER_ERROR"));
}

#[tokio::test]
async fn test_codesearch_non_standard_response() {
    let server = MockServer::start().await;

    // The code search API may return a non-standard JSON structure
    Mock::given(method("GET"))
        .and(path("/api/sn_codesearch/code_search/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"matches": 42, "items": ["a", "b"]}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "codesearch", "search", "test"])
        .assert()
        .success()
        .stdout(predicate::str::contains("matches"))
        .stdout(predicate::str::contains("42"));
}

#[tokio::test]
async fn test_codesearch_with_scope_filter() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/sn_codesearch/code_search/search"))
        .and(query_param("term", "GlideRecord"))
        .and(query_param("scope", "x_my_app"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "sys_id": "sc001",
                    "name": "ScopedScript",
                    "type": "sys_script_include"
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
            "codesearch",
            "search",
            "GlideRecord",
            "--scope",
            "x_my_app",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("ScopedScript"))
        .stdout(predicate::str::contains("sys_script_include"));
}

#[tokio::test]
async fn test_codesearch_with_global_scope_filter() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/sn_codesearch/code_search/search"))
        .and(query_param("term", "gs.info"))
        .and(query_param("scope", "global"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "sys_id": "gl001",
                    "name": "GlobalScript",
                    "type": "sys_script_include"
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
            "codesearch",
            "search",
            "gs.info",
            "--scope",
            "global",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("GlobalScript"));
}
