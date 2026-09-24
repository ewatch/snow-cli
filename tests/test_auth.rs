#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Wiremock-backed integration tests for `auth status` and `ping`.
//!
//! Regression coverage for servicenow-cli-120.3: a stored credential is not
//! proof that the instance accepts it, so `auth status` reports
//! `credentials_present` and only `--verify` / `ping` talk to the server.

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ADMIN_SYS_ID: &str = "6816f79cc0a8016401c5a33be04be441";

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

async fn mount_current_user(server: &MockServer, expected_calls: u64) {
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_user"))
        .and(header("Authorization", "Bearer test-api-token"))
        .and(query_param(
            "sysparm_query",
            "sys_id=javascript:gs.getUserID()",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"sys_id": ADMIN_SYS_ID, "user_name": "admin"}]
        })))
        .expect(expected_calls)
        .mount(server)
        .await;
}

async fn mount_unauthorized(server: &MockServer) {
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": {"message": "User is not authenticated", "detail": "Required to provide Auth information"},
            "status": "failure"
        })))
        .mount(server)
        .await;
}

fn stdout_json(output: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

// --- auth status ---

#[test]
fn test_auth_status_reports_credentials_present_without_server_check() {
    let (_dir, config_path) = api_key_config();

    // No server: without --verify, status must not make a request.
    let assert = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", "http://127.0.0.1:1", "auth", "status"])
        .assert()
        .success();
    let status = stdout_json(assert.get_output());

    assert_eq!(status["credentials_present"], true);
    assert_eq!(status["authenticated"], true);
    assert_eq!(status["instance"], "http://127.0.0.1:1");
    assert!(status.get("verified").is_none());
}

#[tokio::test]
async fn test_auth_status_verify_succeeds_with_accepted_credentials() {
    let server = MockServer::start().await;
    mount_current_user(&server, 1).await;
    let (_dir, config_path) = api_key_config();

    let assert = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "auth", "status", "--verify"])
        .assert()
        .success();
    let status = stdout_json(assert.get_output());

    assert_eq!(status["verified"], true);
    assert_eq!(status["verified_user"], "admin");
    assert_eq!(status["credentials_present"], true);
}

#[tokio::test]
async fn test_auth_status_verify_fails_on_rejected_credentials() {
    let server = MockServer::start().await;
    mount_unauthorized(&server).await;
    let (_dir, config_path) = api_key_config();

    let assert = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "wrong-token")
        .args(["--instance", &server.uri(), "auth", "status", "--verify"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("\"code\":\"UNAUTHORIZED\""))
        .stderr(predicate::str::contains("User is not authenticated"));
    let status = stdout_json(assert.get_output());

    // A stored credential is still reported, but it is not verified.
    assert_eq!(status["credentials_present"], true);
    assert_eq!(status["verified"], false);
    assert_eq!(status["verification_error"], "UNAUTHORIZED");
}

#[test]
fn test_auth_status_verify_fails_on_network_error() {
    let (_dir, config_path) = api_key_config();

    let assert = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            "http://127.0.0.1:1",
            "--timeout-secs",
            "5",
            "auth",
            "status",
            "--verify",
        ])
        .assert()
        .failure();
    let status = stdout_json(assert.get_output());

    assert_eq!(status["verified"], false);
    assert_eq!(status["verification_error"], "REQUEST_FAILED");
}

#[tokio::test]
async fn test_auth_status_browser_session_uses_cookie_env_var() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_user"))
        .and(header("Cookie", "JSESSIONID=abc; glide_user_route=x"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"sys_id": ADMIN_SYS_ID, "user_name": "abel.tuter"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "sso"

[profiles.sso]
instance = "https://placeholder.service-now.com"
auth_method = "browser_session"
"#,
    );

    let assert = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_SESSION_COOKIE", "JSESSIONID=abc; glide_user_route=x")
        .args(["--instance", &server.uri(), "auth", "status", "--verify"])
        .assert()
        .success();
    let status = stdout_json(assert.get_output());

    assert_eq!(status["credentials_present"], true);
    assert_eq!(status["session_cookie_set"], true);
    assert_eq!(status["verified"], true);
    assert_eq!(status["verified_user"], "abel.tuter");
}

#[test]
fn test_auth_status_honours_output_format() {
    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--output", "jsonl", "auth", "status"])
        .assert()
        .success()
        .stdout(predicate::str::is_match(r#"^\{"profile":"default".*\}\n$"#).unwrap());
}

// --- ping ---

#[tokio::test]
async fn test_ping_reports_user_build_and_latency() {
    let server = MockServer::start().await;
    mount_current_user(&server, 2).await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_properties"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"name": "glide.buildtag", "value": "glide-zurich-07-01-2026__patch1"}]
        })))
        .mount(&server)
        .await;
    let (_dir, config_path) = api_key_config();

    // `ping` is read-only, so it must also be available in snow-cli-ro.
    for binary in ["snow-cli", "snow-cli-ro"] {
        let mut command = if binary == "snow-cli" {
            cargo_bin_cmd!("snow-cli")
        } else {
            cargo_bin_cmd!("snow-cli-ro")
        };
        let assert = command
            .env("SNOW_CLI_CONFIG", &config_path)
            .env("SNOW_CLI_API_TOKEN", "test-api-token")
            .args(["--instance", &server.uri(), "ping"])
            .assert()
            .success();
        let ping = stdout_json(assert.get_output());

        assert_eq!(ping["user"], "admin", "{binary}");
        assert_eq!(ping["user_sys_id"], ADMIN_SYS_ID);
        assert_eq!(ping["build"], "glide-zurich-07-01-2026__patch1");
        assert!(ping["latency_ms"].is_u64());
        assert!(ping["instance"].as_str().unwrap().starts_with("http://"));
    }
}

#[tokio::test]
async fn test_ping_tolerates_unreadable_build_properties() {
    let server = MockServer::start().await;
    mount_current_user(&server, 1).await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_properties"))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "error": {"message": "Operation Failed", "detail": "ACL restricts the record retrieval"},
            "status": "failure"
        })))
        .mount(&server)
        .await;
    let (_dir, config_path) = api_key_config();

    let assert = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "ping"])
        .assert()
        .success()
        .stderr("");
    let ping = stdout_json(assert.get_output());

    assert_eq!(ping["user"], "admin");
    assert!(ping["build"].is_null());
}

#[tokio::test]
async fn test_ping_fails_on_rejected_credentials() {
    let server = MockServer::start().await;
    mount_unauthorized(&server).await;
    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "wrong-token")
        .args(["--instance", &server.uri(), "ping"])
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("\"code\":\"UNAUTHORIZED\""));
}
