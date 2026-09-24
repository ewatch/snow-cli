#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Wiremock-backed integration tests for `auth status` and `auth login`.
//!
//! Regression coverage for servicenow-cli-120.3 and -120.23: a stored
//! credential is not proof that the instance accepts it, so `auth status`
//! reports `credentials_present`, only `--verify` talks to the server, and
//! `auth login` verifies stored secrets unless `--no-verify` is given.

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
    // Removed on purpose: it read as "the instance accepted the credentials".
    assert!(status.get("authenticated").is_none());
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

#[tokio::test]
async fn test_auth_status_verify_reports_identity_build_and_latency() {
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

    // Verification is read-only, so it must behave the same in snow-cli-ro.
    for binary in ["snow-cli", "snow-cli-ro"] {
        let mut command = if binary == "snow-cli" {
            cargo_bin_cmd!("snow-cli")
        } else {
            cargo_bin_cmd!("snow-cli-ro")
        };
        let assert = command
            .env("SNOW_CLI_CONFIG", &config_path)
            .env("SNOW_CLI_API_TOKEN", "test-api-token")
            .args(["--instance", &server.uri(), "auth", "status", "--verify"])
            .assert()
            .success();
        let status = stdout_json(assert.get_output());

        assert_eq!(status["verified"], true, "{binary}");
        assert_eq!(status["verified_user"], "admin");
        assert_eq!(status["verified_user_sys_id"], ADMIN_SYS_ID);
        assert_eq!(status["build"], "glide-zurich-07-01-2026__patch1");
        assert!(status["latency_ms"].is_u64());
    }
}

#[tokio::test]
async fn test_auth_status_verify_tolerates_unreadable_build_properties() {
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
        .args(["--instance", &server.uri(), "auth", "status", "--verify"])
        .assert()
        .success()
        .stderr("");
    let status = stdout_json(assert.get_output());

    assert_eq!(status["verified"], true);
    assert!(status["build"].is_null());
}

// --- auth login ---

/// Config for a basic-auth profile pointing at `instance`.
fn basic_config(instance: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    common::create_temp_config(&format!(
        r#"
default_profile = "dev"

[profiles.dev]
instance = "{instance}"
auth_method = "basic"
username = "admin"
"#
    ))
}

fn login_command(
    config_path: &std::path::Path,
    keychain_store: &std::path::Path,
) -> assert_cmd::Command {
    let mut command = cargo_bin_cmd!("snow-cli");
    command
        .env("SNOW_CLI_CONFIG", config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .env_remove("SNOW_CLI_PASSWORD")
        .env_remove("SNOW_CLI_API_TOKEN");
    command
}

#[tokio::test]
async fn test_auth_login_basic_verifies_stored_password() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_user"))
        // base64("admin:secret")
        .and(header("Authorization", "Basic YWRtaW46c2VjcmV0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"sys_id": ADMIN_SYS_ID, "user_name": "admin"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config_path) = basic_config(&server.uri());
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();

    let assert = login_command(&config_path, &keychain_store)
        .args(["auth", "login", "--password", "secret"])
        .assert()
        .success();
    let login = stdout_json(assert.get_output());

    assert_eq!(login["status"], "verified");
    assert_eq!(login["verified_user"], "admin");
    assert_eq!(login["verified_user_sys_id"], ADMIN_SYS_ID);
    assert_eq!(login["instance"], server.uri());
}

#[tokio::test]
async fn test_auth_login_rejected_password_fails_but_keeps_secret() {
    let server = MockServer::start().await;
    mount_unauthorized(&server).await;
    let (_dir, config_path) = basic_config(&server.uri());
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();

    let assert = login_command(&config_path, &keychain_store)
        .args(["auth", "login", "--password", "wrong"])
        .assert()
        .code(5)
        .stderr(predicate::str::contains("\"code\":\"UNAUTHORIZED\""));
    let login = stdout_json(assert.get_output());

    assert_eq!(login["status"], "stored");
    assert_eq!(login["verified"], false);
    assert_eq!(login["verification_error"], "UNAUTHORIZED");
    // Kept so a retry after fixing the account does not need re-entry.
    assert_eq!(
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "dev:password").unwrap(),
        "wrong"
    );
}

#[test]
fn test_auth_login_network_failure_reports_request_failed() {
    let (_dir, config_path) = basic_config("http://127.0.0.1:1");
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();

    let assert = login_command(&config_path, &keychain_store)
        .args([
            "--timeout-secs",
            "5",
            "auth",
            "login",
            "--password",
            "secret",
        ])
        .assert()
        .failure();
    let login = stdout_json(assert.get_output());

    assert_eq!(login["status"], "stored");
    assert_eq!(login["verification_error"], "REQUEST_FAILED");
}

#[test]
fn test_auth_login_no_verify_stores_without_request() {
    // Unreachable instance: --no-verify must not make any request.
    let (_dir, config_path) = basic_config("http://127.0.0.1:1");
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();

    let assert = login_command(&config_path, &keychain_store)
        .args(["auth", "login", "--password", "secret", "--no-verify"])
        .assert()
        .success();
    let login = stdout_json(assert.get_output());

    assert_eq!(login["status"], "stored");
    assert!(login.get("verified").is_none());
    assert_eq!(
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "dev:password").unwrap(),
        "secret"
    );
}

#[tokio::test]
async fn test_auth_login_oauth_client_credentials_verifies_with_new_secret() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth_token.do"))
        .and(wiremock::matchers::body_string_contains(
            "client_secret=new-secret",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "fresh-access-token",
            "token_type": "Bearer",
            "expires_in": 3600
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_user"))
        .and(header("Authorization", "Bearer fresh-access-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"sys_id": ADMIN_SYS_ID, "user_name": "integration.user"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config_path) = common::create_temp_config(&format!(
        r#"
default_profile = "default"

[profiles.default]
instance = "{}"
auth_method = "oauth2"
client_id = "client-id"
oauth_grant_type = "client_credentials"
"#,
        server.uri()
    ));
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();

    let assert = login_command(&config_path, &keychain_store)
        .env_remove("SNOW_CLI_CLIENT_SECRET")
        .args(["auth", "login", "--client-secret", "new-secret"])
        .assert()
        .success();
    let login = stdout_json(assert.get_output());

    assert_eq!(login["status"], "verified");
    assert_eq!(login["verified_user"], "integration.user");
}

// --- auth login: OAuth authorization code ---

/// Config for an OAuth2 authorization-code profile with extra `settings` lines.
fn authorization_code_config(
    instance: &str,
    settings: &str,
) -> (tempfile::TempDir, std::path::PathBuf) {
    common::create_temp_config(&format!(
        r#"
default_profile = "sdk"

[profiles.sdk]
instance = "{instance}"
auth_method = "oauth2"
client_id = "sdk-client-id"
oauth_grant_type = "authorization_code"
{settings}
"#
    ))
}

async fn mount_authorization_code_token(server: &MockServer, expected_calls: u64) {
    Mock::given(method("POST"))
        .and(path("/oauth_token.do"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "sdk-access-token",
            "refresh_token": "sdk-refresh-token",
            "token_type": "Bearer",
            "expires_in": 1800
        })))
        .expect(expected_calls)
        .mount(server)
        .await;
}

/// Decode `application/x-www-form-urlencoded` pairs (also used for URL queries).
fn form_params(encoded: &str) -> std::collections::HashMap<String, String> {
    url::form_urlencoded::parse(encoded.as_bytes())
        .into_owned()
        .collect()
}

/// Query parameters of the authorization URL printed to stderr.
fn printed_authorization_params(stderr: &str) -> std::collections::HashMap<String, String> {
    let url = stderr
        .lines()
        .find(|line| line.contains("/oauth_auth.do?"))
        .expect("authorization URL on stderr");
    form_params(url.split_once('?').unwrap().1)
}

async fn token_request_params(server: &MockServer) -> std::collections::HashMap<String, String> {
    let requests = server.received_requests().await.unwrap();
    let token_request = requests
        .iter()
        .find(|request| request.url.path() == "/oauth_token.do")
        .expect("token request");
    form_params(std::str::from_utf8(&token_request.body).unwrap())
}

#[tokio::test]
async fn test_auth_login_sdk_oauth_exchanges_pasted_code_without_listener() {
    let server = MockServer::start().await;
    mount_authorization_code_token(&server, 1).await;
    // Occupy the profile's loopback port: SDK mode must not try to bind it.
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port();
    let (_dir, config_path) =
        authorization_code_config(&server.uri(), &format!("oauth_redirect_port = {port}"));
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();
    common::write_test_keychain_entry(&keychain_store, "snow-cli", "sdk:client_secret", "stale");

    let assert = login_command(&config_path, &keychain_store)
        .args([
            "auth",
            "login",
            "--sdk-oauth",
            "--no-browser",
            "--code-stdin",
        ])
        .write_stdin("pasted-code-123\n")
        .assert()
        .success();
    let output = assert.get_output();
    let login = stdout_json(output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(login["status"], "verified");
    assert_eq!(login["redirect_uri"], "/sdk-oauth.do");
    assert_eq!(login["has_refresh_token"], true);

    let authorize = printed_authorization_params(&stderr);
    assert_eq!(authorize["response_type"], "code");
    assert_eq!(authorize["client_id"], "sdk-client-id");
    assert_eq!(authorize["redirect_uri"], "/sdk-oauth.do");
    assert_eq!(authorize["code_challenge_method"], "S256");
    assert_eq!(authorize["scope"], "");
    assert!(!authorize["state"].is_empty());

    let token = token_request_params(&server).await;
    assert_eq!(token["grant_type"], "authorization_code");
    assert_eq!(token["client_id"], "sdk-client-id");
    assert_eq!(token["code"], "pasted-code-123");
    assert_eq!(token["redirect_uri"], "/sdk-oauth.do");
    assert!(!token.contains_key("client_secret"));
    let verifier = &token["code_verifier"];
    assert_eq!(
        snow_cli::auth::oauth2::pkce_code_challenge_s256(verifier),
        authorize["code_challenge"]
    );

    // Failure messages name the value, never print it.
    for (label, secret) in [
        ("authorization code", "pasted-code-123"),
        ("code verifier", verifier.as_str()),
        ("access token", "sdk-access-token"),
        ("refresh token", "sdk-refresh-token"),
    ] {
        assert!(!stdout.contains(secret), "stdout leaked the {label}");
        assert!(!stderr.contains(secret), "stderr leaked the {label}");
    }

    let stored = common::read_test_keychain_entry(&keychain_store, "snow-cli", "sdk:oauth_token")
        .expect("stored OAuth token");
    assert!(stored.contains("sdk-access-token"));
    assert!(stored.contains("sdk-refresh-token"));
    assert!(
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "sdk:client_secret")
            .is_none(),
        "public-client login must clear a stale client secret"
    );
    drop(occupied);
}

#[tokio::test]
async fn test_auth_login_sdk_oauth_uses_configured_scope() {
    let server = MockServer::start().await;
    mount_authorization_code_token(&server, 1).await;
    let (_dir, config_path) =
        authorization_code_config(&server.uri(), r#"oauth_scope = "useraccount""#);
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();

    let assert = login_command(&config_path, &keychain_store)
        .args([
            "auth",
            "login",
            "--sdk-oauth",
            "--no-browser",
            "--code-stdin",
        ])
        .write_stdin("pasted-code")
        .assert()
        .success();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(
        printed_authorization_params(&stderr)["scope"],
        "useraccount"
    );
}

#[tokio::test]
async fn test_auth_login_sdk_oauth_rejects_empty_code() {
    let server = MockServer::start().await;
    mount_authorization_code_token(&server, 0).await;
    let (_dir, config_path) = authorization_code_config(&server.uri(), "");
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();

    login_command(&config_path, &keychain_store)
        .args([
            "auth",
            "login",
            "--sdk-oauth",
            "--no-browser",
            "--code-stdin",
        ])
        .write_stdin("\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Empty authorization code"));

    assert!(
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "sdk:oauth_token").is_none()
    );
}

#[tokio::test]
async fn test_auth_login_sdk_oauth_without_terminal_requires_code_stdin() {
    let server = MockServer::start().await;
    mount_authorization_code_token(&server, 0).await;
    let (_dir, config_path) = authorization_code_config(&server.uri(), "");
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();
    common::write_test_keychain_entry(&keychain_store, "snow-cli", "sdk:client_secret", "kept");

    login_command(&config_path, &keychain_store)
        .args(["auth", "login", "--sdk-oauth", "--no-browser"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--code-stdin"))
        .stderr(predicate::str::contains("oauth_auth.do").not());

    // Fails before any keychain change.
    assert_eq!(
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "sdk:client_secret")
            .as_deref(),
        Some("kept")
    );
}

#[test]
fn test_auth_login_sdk_oauth_rejects_other_grants() {
    let (_dir, config_path) = common::create_temp_config(
        r#"
default_profile = "integration"

[profiles.integration]
instance = "http://127.0.0.1:1"
auth_method = "oauth2"
client_id = "client-id"
oauth_grant_type = "client_credentials"
"#,
    );
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();

    login_command(&config_path, &keychain_store)
        .args(["auth", "login", "--sdk-oauth", "--client-secret", "secret"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("authorization-code grant"));
    assert!(
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "integration:client_secret")
            .is_none()
    );
}

#[test]
fn test_auth_login_sdk_oauth_rejects_basic_profile() {
    let (_dir, config_path) = basic_config("http://127.0.0.1:1");
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();

    login_command(&config_path, &keychain_store)
        .args(["auth", "login", "--sdk-oauth", "--password", "secret"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("authorization-code grant"));
    assert!(
        common::read_test_keychain_entry(&keychain_store, "snow-cli", "dev:password").is_none()
    );
}

#[test]
fn test_auth_login_code_stdin_requires_sdk_oauth() {
    let (_dir, config_path) = authorization_code_config("http://127.0.0.1:1", "");
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();

    login_command(&config_path, &keychain_store)
        .args(["auth", "login", "--code-stdin"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--sdk-oauth"));
}

#[tokio::test]
async fn test_auth_login_authorization_code_default_uses_loopback_callback() {
    use std::io::{BufRead, Read, Write};

    let server = MockServer::start().await;
    mount_authorization_code_token(&server, 1).await;
    // Port 0 lets the OS pick a free port; the printed redirect URI reports it.
    let (_dir, config_path) = authorization_code_config(&server.uri(), "oauth_redirect_port = 0");
    let (_keychain_dir, keychain_store) = common::create_temp_keychain_store();

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_snow-cli"))
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &keychain_store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .args(["auth", "login", "--no-browser"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let mut stderr = std::io::BufReader::new(child.stderr.take().unwrap());
    let mut printed = String::new();
    let redirect_uri = loop {
        let mut line = String::new();
        assert_ne!(stderr.read_line(&mut line).unwrap(), 0, "stderr: {printed}");
        printed.push_str(&line);
        if let Some(rest) = line.strip_prefix("Waiting for ServiceNow OAuth redirect on ") {
            break rest.trim_end_matches(" ...\n").to_string();
        }
    };
    let authorize = printed_authorization_params(&printed);
    assert_eq!(authorize["redirect_uri"], redirect_uri);
    assert_eq!(authorize["scope"], "useraccount");
    let callback = redirect_uri.strip_prefix("http://").unwrap();
    let (address, callback_path) = callback.split_once('/').unwrap();

    let state = authorize["state"].clone();
    let address = address.to_string();
    let callback_path = callback_path.to_string();
    let child = tokio::task::spawn_blocking(move || {
        let mut stream = std::net::TcpStream::connect(&address).unwrap();
        write!(
            stream,
            "GET /{callback_path}?code=loopback-code&state={state} HTTP/1.1\r\nHost: {address}\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        child.wait_with_output().unwrap()
    })
    .await
    .unwrap();

    assert!(child.status.success());
    let login = stdout_json(&child);
    assert_eq!(login["redirect_uri"], redirect_uri);
    let token = token_request_params(&server).await;
    assert_eq!(token["code"], "loopback-code");
    assert_eq!(token["redirect_uri"], redirect_uri);
    assert_eq!(
        snow_cli::auth::oauth2::pkce_code_challenge_s256(&token["code_verifier"]),
        authorize["code_challenge"]
    );
}
