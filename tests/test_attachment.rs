//! Wiremock-backed integration tests for `attachment` commands.

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use wiremock::matchers::{body_string_contains, header, method, path, query_param};
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
async fn test_attachment_list() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/attachment"))
        .and(query_param(
            "sysparm_query",
            "table_name=incident^table_sys_id=abc123",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "sys_id": "att1",
                    "file_name": "error.log",
                    "content_type": "text/plain",
                    "size_bytes": "42",
                    "table_name": "incident",
                    "table_sys_id": "abc123",
                    "download_link": "/api/now/attachment/att1/file"
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
            "attachment",
            "list",
            "incident",
            "abc123",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("error.log"));
}

#[tokio::test]
async fn test_attachment_download_to_explicit_path() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/attachment/att42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": {
                "sys_id": "att42",
                "file_name": "from_server.txt",
                "download_link": "/api/now/attachment/att42/file",
                "content_type": "text/plain",
                "size_bytes": "12",
                "table_name": "incident",
                "table_sys_id": "abc123"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/attachment/att42/file"))
        .and(header("Authorization", "Bearer test-api-token"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello world\n".to_vec()))
        .expect(1)
        .mount(&server)
        .await;

    let temp_dir = tempfile::tempdir().unwrap();
    let out_path = temp_dir.path().join("downloaded.txt");
    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "attachment",
            "download",
            "att42",
            "--out",
            &out_path.to_string_lossy(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Downloaded attachment"));

    let content = std::fs::read_to_string(&out_path).unwrap();
    assert_eq!(content, "hello world\n");
}

#[tokio::test]
async fn test_attachment_upload() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/now/attachment/upload"))
        .and(query_param("table_name", "incident"))
        .and(query_param("table_sys_id", "abc123"))
        .and(query_param("file_name", "upload.txt"))
        .and(header("Authorization", "Bearer test-api-token"))
        .and(body_string_contains("upload body"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "result": {
                "sys_id": "newatt",
                "file_name": "upload.txt",
                "content_type": "application/octet-stream",
                "size_bytes": "11",
                "table_name": "incident",
                "table_sys_id": "abc123",
                "download_link": "/api/now/attachment/newatt/file"
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let temp_dir = tempfile::tempdir().unwrap();
    let upload_path = temp_dir.path().join("upload.txt");
    std::fs::write(&upload_path, "upload body").unwrap();

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "attachment",
            "upload",
            "incident",
            "abc123",
            "--file",
            &upload_path.to_string_lossy(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("newatt"));
}

#[test]
fn test_attachment_help() {
    cargo_bin_cmd!("snow-cli")
        .args(["attachment", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("download"))
        .stdout(predicate::str::contains("upload"));
}
