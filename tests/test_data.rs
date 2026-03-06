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

fn write_dataset_file(dir: &tempfile::TempDir, body: serde_json::Value) -> std::path::PathBuf {
    let path = dir.path().join("dataset.json");
    std::fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();
    path
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

#[tokio::test]
async fn test_data_validate_success() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "element": "short_description",
                    "internal_type": "string",
                    "mandatory": "true",
                    "read_only": "false",
                    "default_value": ""
                },
                {
                    "element": "priority",
                    "internal_type": "integer",
                    "mandatory": "false",
                    "read_only": "false",
                    "default_value": ""
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();
    let dataset_dir = tempfile::tempdir().unwrap();
    let dataset_path = write_dataset_file(
        &dataset_dir,
        serde_json::json!({
            "version": 1,
            "kind": "table-export",
            "command": "data export",
            "instance": "https://dev.service-now.com",
            "table": "incident",
            "record_count": 1,
            "records": [
                {"short_description": "VPN issue", "priority": "2"}
            ]
        }),
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "data",
            "validate",
            "--file",
            dataset_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"kind\":\"validation-report\""))
        .stdout(predicate::str::contains("\"ready\":true"))
        .stdout(predicate::str::contains("\"field_count\":2"));
}

#[tokio::test]
async fn test_data_validate_reports_read_only_and_missing_required_fields() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "element": "short_description",
                    "internal_type": "string",
                    "mandatory": "true",
                    "read_only": "false",
                    "default_value": ""
                },
                {
                    "element": "sys_id",
                    "internal_type": "string",
                    "mandatory": "false",
                    "read_only": "true",
                    "default_value": ""
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();
    let dataset_dir = tempfile::tempdir().unwrap();
    let dataset_path = write_dataset_file(
        &dataset_dir,
        serde_json::json!({
            "version": 1,
            "kind": "table-export",
            "command": "data export",
            "instance": "https://dev.service-now.com",
            "table": "incident",
            "record_count": 1,
            "records": [
                {"sys_id": "abc123"}
            ]
        }),
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "data",
            "validate",
            "--file",
            dataset_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ready\":false"))
        .stdout(predicate::str::contains("field_not_writable"))
        .stdout(predicate::str::contains("missing_required_field"));
}

#[tokio::test]
async fn test_data_import_success() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "element": "short_description",
                    "internal_type": "string",
                    "mandatory": "true",
                    "read_only": "false",
                    "default_value": ""
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/now/table/incident"))
        .and(header("Content-Type", "application/json"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "result": {"sys_id": "new123", "short_description": "VPN issue"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();
    let dataset_dir = tempfile::tempdir().unwrap();
    let dataset_path = write_dataset_file(
        &dataset_dir,
        serde_json::json!({
            "version": 1,
            "kind": "table-export",
            "command": "data export",
            "instance": "https://dev.service-now.com",
            "table": "incident",
            "record_count": 1,
            "records": [
                {"short_description": "VPN issue"}
            ]
        }),
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "data",
            "import",
            "--file",
            dataset_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"kind\":\"import-result\""))
        .stdout(predicate::str::contains("\"strategy\":\"table_api\""))
        .stdout(predicate::str::contains("\"created\":1"))
        .stdout(predicate::str::contains("\"failed\":0"));
}

#[tokio::test]
async fn test_data_import_partial_failure_reports_summary() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "element": "short_description",
                    "internal_type": "string",
                    "mandatory": "true",
                    "read_only": "false",
                    "default_value": ""
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/now/table/incident"))
        .respond_with(
            ResponseTemplate::new(500).set_body_string("insert failed for incident record"),
        )
        .expect(2)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();
    let dataset_dir = tempfile::tempdir().unwrap();
    let dataset_path = write_dataset_file(
        &dataset_dir,
        serde_json::json!({
            "version": 1,
            "kind": "table-export",
            "command": "data export",
            "instance": "https://dev.service-now.com",
            "table": "incident",
            "record_count": 2,
            "records": [
                {"short_description": "VPN issue 1"},
                {"short_description": "VPN issue 2"}
            ]
        }),
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "data",
            "import",
            "--file",
            dataset_path.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"failed\":2"))
        .stdout(predicate::str::contains("SERVER_ERROR"));
}

#[tokio::test]
async fn test_data_validate_accepts_inherited_fields() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_db_object"))
        .and(query_param("sysparm_query", "name=incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "name": "incident",
                    "super_class": {"value": "task-sys-id"}
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_db_object"))
        .and(query_param("sysparm_query", "sys_id=task-sys-id"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "name": "task",
                    "super_class": ""
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .and(query_param(
            "sysparm_query",
            "nameINincident,task^elementISNOTEMPTY^element!=sys_tags",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "element": "short_description",
                    "internal_type": {"value": "string"},
                    "mandatory": "true",
                    "read_only": "false",
                    "default_value": ""
                },
                {
                    "element": "priority",
                    "internal_type": {"value": "integer"},
                    "mandatory": "false",
                    "read_only": "false",
                    "default_value": ""
                },
                {
                    "element": "number",
                    "internal_type": {"value": "string"},
                    "mandatory": "false",
                    "read_only": "false",
                    "default_value": ""
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();
    let dataset_dir = tempfile::tempdir().unwrap();
    let dataset_path = write_dataset_file(
        &dataset_dir,
        serde_json::json!({
            "version": 1,
            "kind": "table-export",
            "command": "data export",
            "instance": "https://dev.service-now.com",
            "table": "incident",
            "record_count": 1,
            "records": [
                {
                    "number": "INC0010001",
                    "short_description": "Email outage",
                    "priority": "3"
                }
            ]
        }),
    );

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "data",
            "validate",
            "--file",
            dataset_path.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ready\":true"))
        .stdout(predicate::str::contains("\"field_count\":3"));
}

#[tokio::test]
async fn test_data_export_package_writes_manifest_and_reference_placeholders() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/u_parent"))
        .and(query_param("sysparm_fields", "name,sys_id"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "parent-source-1", "name": "Parent A"}
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/u_child"))
        .and(query_param("sysparm_fields", "name,parent_ref,sys_id"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {
                    "sys_id": "child-source-1",
                    "name": "Child A",
                    "parent_ref": {"value": "parent-source-1"}
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let spec_path = write_dataset_file(
        &temp_dir,
        serde_json::json!({
            "version": 1,
            "kind": "dataset-export-spec",
            "tables": [
                {
                    "name": "u_parent",
                    "fields": ["name"]
                },
                {
                    "name": "u_child",
                    "fields": ["name", "parent_ref"],
                    "depends_on": ["u_parent"],
                    "references": [
                        {
                            "field": "parent_ref",
                            "target_table": "u_parent",
                            "source_key": "name",
                            "target_key": "name"
                        }
                    ]
                }
            ]
        }),
    );
    let out_dir = temp_dir.path().join("dataset");

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "data",
            "export-package",
            "--file",
            spec_path.to_str().unwrap(),
            "--out-dir",
            out_dir.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"kind\":\"dataset-export-result\"",
        ))
        .stdout(predicate::str::contains("\"table_count\":2"));

    let manifest = std::fs::read_to_string(out_dir.join("manifest.json")).unwrap();
    let child = std::fs::read_to_string(out_dir.join("u_child.json")).unwrap();
    assert!(manifest.contains("\"kind\":\"dataset\""));
    assert!(manifest.contains("\"name\":\"u_parent\""));
    assert!(manifest.contains("\"name\":\"u_child\""));
    assert!(child.contains("__reference"));
    assert!(child.contains("\"source_value\":\"Parent A\""));
}

#[tokio::test]
async fn test_data_import_package_remaps_references() {
    let server = MockServer::start().await;

    for table in ["u_parent", "u_child"] {
        Mock::given(method("GET"))
            .and(path("/api/now/table/sys_dictionary"))
            .and(query_param(
                "sysparm_query",
                &format!("name={}^elementISNOTEMPTY^element!=sys_tags", table),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": if table == "u_parent" {
                    vec![serde_json::json!({
                        "element": "name",
                        "internal_type": "string",
                        "mandatory": "true",
                        "read_only": "false",
                        "default_value": ""
                    })]
                } else {
                    vec![
                        serde_json::json!({
                            "element": "name",
                            "internal_type": "string",
                            "mandatory": "true",
                            "read_only": "false",
                            "default_value": ""
                        }),
                        serde_json::json!({
                            "element": "parent_ref",
                            "internal_type": "reference",
                            "mandatory": "false",
                            "read_only": "false",
                            "default_value": ""
                        })
                    ]
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
    }

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_db_object"))
        .respond_with(ResponseTemplate::new(404))
        .expect(2)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/now/table/u_parent"))
        .and(body_string_contains("Parent A"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "result": {"sys_id": "parent-target-1", "name": "Parent A"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/now/table/u_child"))
        .and(body_string_contains("parent-target-1"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "result": {"sys_id": "child-target-1", "name": "Child A"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let dataset_dir = temp_dir.path().join("dataset");
    std::fs::create_dir_all(&dataset_dir).unwrap();

    std::fs::write(
        dataset_dir.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "kind": "dataset",
            "command": "data export-package",
            "instance": "https://dev.service-now.com",
            "exported_at_unix_s": 1,
            "tables": [
                {
                    "name": "u_parent",
                    "file": "u_parent.json",
                    "fields": ["name"],
                    "record_count": 1,
                    "depends_on": [],
                    "references": []
                },
                {
                    "name": "u_child",
                    "file": "u_child.json",
                    "fields": ["name", "parent_ref"],
                    "record_count": 1,
                    "depends_on": ["u_parent"],
                    "references": [
                        {
                            "field": "parent_ref",
                            "target_table": "u_parent",
                            "source_key": "name",
                            "target_key": "name"
                        }
                    ]
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    std::fs::write(
        dataset_dir.join("u_parent.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "kind": "dataset-table",
            "table": "u_parent",
            "source_key_fields": ["name"],
            "records": [
                {
                    "source_sys_id": "parent-source-1",
                    "data": {"name": "Parent A"}
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    std::fs::write(
        dataset_dir.join("u_child.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "kind": "dataset-table",
            "table": "u_child",
            "source_key_fields": [],
            "records": [
                {
                    "source_sys_id": "child-source-1",
                    "data": {
                        "name": "Child A",
                        "parent_ref": {
                            "__reference": {
                                "target_table": "u_parent",
                                "source_key": "name",
                                "target_key": "name",
                                "source_value": "Parent A"
                            }
                        }
                    }
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "data",
            "import",
            "--file",
            dataset_dir.join("manifest.json").to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"kind\":\"dataset-import-result\"",
        ))
        .stdout(predicate::str::contains("\"created\":2"))
        .stdout(predicate::str::contains("\"table\":\"u_child\""));
}

#[tokio::test]
async fn test_data_import_package_dry_run_reports_plan_without_writes() {
    let server = MockServer::start().await;

    for table in ["u_parent", "u_child"] {
        Mock::given(method("GET"))
            .and(path("/api/now/table/sys_dictionary"))
            .and(query_param(
                "sysparm_query",
                &format!("name={}^elementISNOTEMPTY^element!=sys_tags", table),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": if table == "u_parent" {
                    vec![serde_json::json!({
                        "element": "name",
                        "internal_type": "string",
                        "mandatory": "true",
                        "read_only": "false",
                        "default_value": ""
                    })]
                } else {
                    vec![
                        serde_json::json!({
                            "element": "name",
                            "internal_type": "string",
                            "mandatory": "true",
                            "read_only": "false",
                            "default_value": ""
                        }),
                        serde_json::json!({
                            "element": "parent_ref",
                            "internal_type": "reference",
                            "mandatory": "false",
                            "read_only": "false",
                            "default_value": ""
                        })
                    ]
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
    }

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_db_object"))
        .respond_with(ResponseTemplate::new(404))
        .expect(2)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let dataset_dir = temp_dir.path().join("dataset");
    std::fs::create_dir_all(&dataset_dir).unwrap();

    std::fs::write(
        dataset_dir.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "kind": "dataset",
            "command": "data export-package",
            "instance": "https://dev.service-now.com",
            "exported_at_unix_s": 1,
            "tables": [
                {
                    "name": "u_parent",
                    "file": "u_parent.json",
                    "fields": ["name"],
                    "record_count": 1,
                    "depends_on": [],
                    "references": []
                },
                {
                    "name": "u_child",
                    "file": "u_child.json",
                    "fields": ["name", "parent_ref"],
                    "record_count": 1,
                    "depends_on": ["u_parent"],
                    "references": [
                        {
                            "field": "parent_ref",
                            "target_table": "u_parent",
                            "source_key": "name",
                            "target_key": "name"
                        }
                    ]
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    std::fs::write(
        dataset_dir.join("u_parent.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "kind": "dataset-table",
            "table": "u_parent",
            "source_key_fields": ["name"],
            "records": [{"source_sys_id": "parent-source-1", "data": {"name": "Parent A"}}]
        }))
        .unwrap(),
    )
    .unwrap();

    std::fs::write(
        dataset_dir.join("u_child.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "kind": "dataset-table",
            "table": "u_child",
            "source_key_fields": [],
            "records": [{
                "source_sys_id": "child-source-1",
                "data": {
                    "name": "Child A",
                    "parent_ref": {
                        "__reference": {
                            "target_table": "u_parent",
                            "source_key": "name",
                            "target_key": "name",
                            "source_value": "Parent A"
                        }
                    }
                }
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "data",
            "import",
            "--file",
            dataset_dir.join("manifest.json").to_str().unwrap(),
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"kind\":\"dataset-import-dry-run\"",
        ))
        .stdout(predicate::str::contains("\"created\":0"))
        .stdout(predicate::str::contains("\"skipped\":2"));
}
