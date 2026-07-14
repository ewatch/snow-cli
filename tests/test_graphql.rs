#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Wiremock-backed integration tests for the opt-in `graphql` command.

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use wiremock::matchers::{body_json, header, method, path};
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
async fn inline_query_and_variables_emit_only_graphql_data() {
    let server = MockServer::start().await;
    let query = "query Incident($number: String!) { incident(number: $number) { number } }";
    let variables = serde_json::json!({"number": "INC0010001"});

    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(header("Authorization", "Bearer test-api-token"))
        .and(header("Accept", "application/json"))
        .and(header("Content-Type", "application/json"))
        .and(body_json(serde_json::json!({
            "query": query,
            "variables": variables
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {
                "incident": {
                    "number": "INC0010001",
                    "caller": {"name": "Ada Lovelace"}
                }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_directory, config_path) = api_key_config();
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "graphql",
            "--query",
            query,
            "--variables",
            &variables.to_string(),
        ])
        .assert()
        .success()
        .stdout(predicate::eq(
            "{\"incident\":{\"number\":\"INC0010001\",\"caller\":{\"name\":\"Ada Lovelace\"}}}\n",
        ));
}

#[tokio::test]
async fn query_file_is_posted_with_default_empty_variables() {
    let server = MockServer::start().await;
    let query = "{ incident { number } }";
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(body_json(serde_json::json!({
            "query": query,
            "variables": {}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"incident": {"number": "INC0010002"}}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let query_directory = tempfile::tempdir().unwrap();
    let query_path = query_directory.path().join("incident.graphql");
    std::fs::write(&query_path, query).unwrap();
    let (_config_directory, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "graphql", "--query-file"])
        .arg(&query_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("INC0010002"));
}

#[tokio::test]
async fn query_can_be_read_from_stdin() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .and(body_json(serde_json::json!({
            "query": "{ user { name } }",
            "variables": {}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"user": {"name": "Grace Hopper"}}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_directory, config_path) = api_key_config();
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "graphql"])
        .write_stdin("{ user { name } }")
        .assert()
        .success()
        .stdout(predicate::str::contains("Grace Hopper"));
}

#[tokio::test]
async fn malformed_variables_are_rejected_without_a_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let (_directory, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "graphql",
            "{ incident { number } }",
            "--variables",
            "{malformed",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Invalid GraphQL variables JSON"));

    server.verify().await;
}

#[tokio::test]
async fn absent_document_is_rejected_without_a_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let (_directory, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "graphql"])
        .write_stdin("")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("stdin is empty"));

    server.verify().await;
}

#[tokio::test]
async fn graphql_errors_are_structured_and_raw_envelope_is_suppressed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/now/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"partial": "partial-data-secret"},
            "errors": [{
                "message": "Unknown field callerName",
                "path": ["partial"],
                "extensions": {"debug": "extension-secret"}
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (_directory, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "graphql",
            "{ incident { callerName } }",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("GRAPHQL_ERROR"))
        .stderr(predicate::str::contains("Unknown field callerName"))
        .stderr(predicate::str::contains("partial-data-secret").not())
        .stderr(predicate::str::contains("extension-secret").not())
        .stderr(predicate::str::contains("\"errors\"").not());
}

#[test]
fn full_cli_read_only_mode_denies_graphql_before_config_resolution() {
    let directory = tempfile::tempdir().unwrap();
    let nonexistent_config = directory.path().join("does-not-exist.toml");

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", nonexistent_config)
        .args(["--read-only", "graphql", "{ incident { number } }"])
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("POLICY_DENIED"))
        .stderr(predicate::str::contains("may contain mutations"));
}

#[test]
fn read_only_binary_does_not_advertise_or_parse_graphql() {
    cargo_bin_cmd!("snow-cli-ro")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("graphql").not())
        .stdout(predicate::str::contains("GraphQL").not());

    cargo_bin_cmd!("snow-cli-ro")
        .args(["graphql", "{ incident { number } }"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unrecognized subcommand 'graphql'",
        ));
}
