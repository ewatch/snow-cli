//! Wiremock-backed integration tests for `api` and `script` commands.
//!
//! Uses `assert_cmd` with wiremock `MockServer` and api_key auth
//! via `SNOW_CLI_API_TOKEN` env var (no OS keychain needed).

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use wiremock::matchers::{body_string_contains, header, method, path};
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

// =============================================================
// api get
// =============================================================

#[tokio::test]
async fn test_api_get_json_response() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/x_myapp/my_endpoint"))
        .and(header("Authorization", "Bearer test-api-token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"result": "hello from scripted rest"})),
        )
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
            "api",
            "get",
            "/api/x_myapp/my_endpoint",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("hello from scripted rest"));
}

#[tokio::test]
async fn test_api_get_with_custom_header() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/x_myapp/endpoint"))
        .and(header("X-Custom", "my-value"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"custom": true})))
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
            "api",
            "get",
            "/api/x_myapp/endpoint",
            "-H",
            "X-Custom: my-value",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""custom": true"#));
}

// =============================================================
// api post
// =============================================================

#[tokio::test]
async fn test_api_post_with_data() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/x_myapp/create"))
        .and(body_string_contains("test_value"))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(serde_json::json!({"result": {"sys_id": "new123"}})),
        )
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
            "api",
            "post",
            "/api/x_myapp/create",
            "--data",
            r#"{"key":"test_value"}"#,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("new123"));
}

#[tokio::test]
async fn test_api_post_from_stdin() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/x_myapp/create"))
        .and(body_string_contains("stdin_value"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
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
            "api",
            "post",
            "/api/x_myapp/create",
        ])
        .write_stdin(r#"{"key":"stdin_value"}"#)
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""ok": true"#));
}

// =============================================================
// api put
// =============================================================

#[tokio::test]
async fn test_api_put_with_data() {
    let server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/api/x_myapp/update/abc123"))
        .and(body_string_contains("updated"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
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
            "api",
            "put",
            "/api/x_myapp/update/abc123",
            "--data",
            r#"{"state":"updated"}"#,
        ])
        .assert()
        .success();
}

// =============================================================
// api delete
// =============================================================

#[tokio::test]
async fn test_api_delete() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/api/x_myapp/record/del456"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    // 204 No Content — the response body is empty, output should still succeed
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "api",
            "delete",
            "/api/x_myapp/record/del456",
        ])
        .assert()
        .success();
}

// =============================================================
// api error handling
// =============================================================

#[tokio::test]
async fn test_api_get_404_returns_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/x_myapp/missing"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not found"))
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "api",
            "get",
            "/api/x_myapp/missing",
        ])
        .assert()
        .failure();
}

// =============================================================
// api help
// =============================================================

#[test]
fn test_api_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["api", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("get"))
        .stdout(predicate::str::contains("post"))
        .stdout(predicate::str::contains("put"))
        .stdout(predicate::str::contains("delete"));
}

// =============================================================
// script run
// =============================================================

#[tokio::test]
async fn test_script_run_with_inline_code() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/script/run"))
        .and(body_string_contains("gs.info"))
        .and(body_string_contains("global"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"result": "script output here"})),
        )
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
            "script",
            "run",
            "--code",
            "gs.info('hello world')",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("script output here"));
}

#[tokio::test]
async fn test_script_run_with_custom_scope() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/script/run"))
        .and(body_string_contains("x_myapp"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": "scoped"})),
        )
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
            "script",
            "run",
            "--code",
            "gs.info('test')",
            "--scope",
            "x_myapp",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("scoped"));
}

#[tokio::test]
async fn test_script_run_from_file() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/script/run"))
        .and(body_string_contains("from_file_script"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"result": "file executed"})),
        )
        .expect(1)
        .mount(&server)
        .await;

    // Create a temp script file
    let dir = tempfile::tempdir().unwrap();
    let script_path = dir.path().join("test_script.js");
    std::fs::write(&script_path, "gs.info('from_file_script')").unwrap();

    let (_config_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "script",
            "run",
            "--file",
            &script_path.to_string_lossy(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("file executed"));
}

#[tokio::test]
async fn test_script_run_from_stdin() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/script/run"))
        .and(body_string_contains("stdin_script"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"result": "stdin executed"})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "script", "run"])
        .write_stdin("gs.info('stdin_script')")
        .assert()
        .success()
        .stdout(predicate::str::contains("stdin executed"));
}

#[test]
fn test_script_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["script", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("run"))
        .stdout(predicate::str::contains("Execute"));
}

#[test]
fn test_script_run_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["script", "run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--file"))
        .stdout(predicate::str::contains("--code"))
        .stdout(predicate::str::contains("--scope"));
}
