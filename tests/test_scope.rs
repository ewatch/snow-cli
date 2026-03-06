mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use wiremock::matchers::{method, path, query_param};
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

async fn mount_scope_inventory_mocks(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_scope"))
        .and(query_param(
            "sysparm_query",
            "scope=x_my_app^ORsys_id=x_my_app",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "scope123", "scope": "x_my_app", "name": "My App", "version": "1.2.3"}
            ]
        })))
        .expect(1)
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_db_object"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"sys_id": "sys_db_object-1", "name": "x_my_app_table", "sys_scope": "scope123"}]
        })))
        .expect(1)
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"sys_id": "dict-1", "element": "u_flag"}]
        })))
        .expect(1)
        .mount(server)
        .await;

    for table in [
        "sys_script_include",
        "sys_script",
        "sys_ui_action",
        "sys_ui_page",
        "sys_ui_policy",
        "sys_properties",
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/api/now/table/{table}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [{"sys_id": format!("{table}-1"), "name": format!("{table}_name"), "sys_scope": "scope123"}]
            })))
            .expect(1)
            .mount(server)
            .await;
    }
}

#[tokio::test]
async fn test_scope_inspect_basic_json() {
    let server = MockServer::start().await;
    mount_scope_inventory_mocks(&server).await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "scope", "inspect", "x_my_app"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"scope\":\"x_my_app\""))
        .stdout(predicate::str::contains("\"artifact_counts\""))
        .stdout(predicate::str::contains("\"total_artifacts\":8"));
}

#[tokio::test]
async fn test_scope_inventory_json() {
    let server = MockServer::start().await;
    mount_scope_inventory_mocks(&server).await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "scope",
            "inventory",
            "x_my_app",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"rows\""))
        .stdout(predicate::str::contains("\"artifact_type\":\"tables\""))
        .stdout(predicate::str::contains(
            "\"source_table\":\"sys_db_object\"",
        ));
}

#[tokio::test]
async fn test_scope_inventory_csv() {
    let server = MockServer::start().await;
    mount_scope_inventory_mocks(&server).await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--output",
            "csv",
            "--instance",
            &server.uri(),
            "scope",
            "inventory",
            "x_my_app",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "scope,scope_sys_id,category,artifact_type,source_table,sys_id,name",
        ))
        .stdout(predicate::str::contains(
            "data_model_logic,tables,sys_db_object,sys_db_object-1,x_my_app_table",
        ));
}

#[tokio::test]
async fn test_scope_inspect_unknown_scope_fails() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_scope"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": []
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
            "scope",
            "inspect",
            "does_not_exist",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Scope 'does_not_exist' was not found",
        ));
}
