mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use wiremock::matchers::{header, method, path, query_param};
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
async fn test_data_export_json_output() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(header("Authorization", "Bearer test-api-token"))
        .and(query_param("sysparm_query", "active=true"))
        .and(query_param(
            "sysparm_fields",
            "sys_id,number,short_description",
        ))
        .and(query_param("sysparm_orderby", "number"))
        .and(query_param("sysparm_offset", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "sys_id": "abc123",
                    "number": "INC001",
                    "short_description": "Email outage"
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
            "data",
            "export",
            "incident",
            "--query",
            "active=true",
            "--fields",
            "sys_id,number,short_description",
            "--order-by",
            "number",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"kind\":\"table-export\""))
        .stdout(predicate::str::contains("\"command\":\"data export\""))
        .stdout(predicate::str::contains("\"table\":\"incident\""))
        .stdout(predicate::str::contains("\"record_count\":1"))
        .stdout(predicate::str::contains("INC001"));
}

#[tokio::test]
async fn test_data_export_csv_output() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "abc123", "user_name": "alice", "email": "alice@example.com"},
                {"sys_id": "def456", "user_name": "bob", "email": "bob@example.com"}
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
            "data",
            "export",
            "sys_user",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("email,sys_id,user_name"))
        .stdout(predicate::str::contains("alice@example.com,abc123,alice"))
        .stdout(predicate::str::contains("bob@example.com,def456,bob"));
}

#[tokio::test]
async fn test_data_export_writes_file_and_prints_summary() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "abc123", "number": "INC001"}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let export_path = temp_dir.path().join("incident-export.json");

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "data",
            "export",
            "incident",
            "--out",
            export_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"kind\":\"export-result\""))
        .stdout(predicate::str::contains(export_path.to_str().unwrap()));

    let written = std::fs::read_to_string(&export_path).unwrap();
    assert!(written.contains("\"kind\":\"table-export\""));
    assert!(written.contains("\"table\":\"incident\""));
    assert!(written.contains("INC001"));
}

#[tokio::test]
async fn test_data_export_404_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/nonexistent"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Table not found"))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "data", "export", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("NOT_FOUND"));
}
