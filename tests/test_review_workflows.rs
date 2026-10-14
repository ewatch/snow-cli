#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn import_ready_rejects_formats_that_cannot_be_reimported_before_client_creation() {
    let (_dir, config) = common::create_temp_config(
        "default_profile = 'default'\n[profiles.default]\ninstance = 'https://placeholder.service-now.com'\nauth_method = 'api_key'\n",
    );
    for format in ["csv", "jsonl", "toon", "text"] {
        cargo_bin_cmd!("snow-cli")
            .env("SNOW_CLI_CONFIG", &config)
            .env_remove("SNOW_CLI_API_TOKEN")
            .args([
                "--output",
                format,
                "data",
                "export",
                "incident",
                "--import-ready",
            ])
            .assert()
            .code(2)
            .stderr(predicates::str::contains(
                "Import-ready export requires json or auto output",
            ));
    }
}

#[tokio::test]
async fn import_ready_does_not_overwrite_artifacts_when_schema_is_unavailable() {
    let server = MockServer::start().await;
    let (_dir, config) = common::create_temp_config(
        "default_profile = 'default'\n[profiles.default]\ninstance = 'https://placeholder.service-now.com'\nauth_method = 'api_key'\n",
    );
    for table in ["incident", "sys_db_object", "sys_dictionary"] {
        Mock::given(method("GET"))
            .and(path(format!("/api/now/table/{table}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": []})))
            .mount(&server)
            .await;
    }
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("preserved.json");
    std::fs::write(&file, "original artifact").unwrap();
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config)
        .env("SNOW_CLI_API_TOKEN", "test-token")
        .args([
            "--instance",
            &server.uri(),
            "data",
            "export",
            "incident",
            "--import-ready",
            "--out",
            file.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicates::str::contains(
            "requires readable dictionary columns",
        ));
    assert_eq!(std::fs::read_to_string(file).unwrap(), "original artifact");
}

#[tokio::test]
async fn import_validation_errors_are_itemized_without_record_values_or_writes() {
    let server = MockServer::start().await;
    let (_dir, config) = common::create_temp_config(
        "default_profile = 'default'\n[profiles.default]\ninstance = 'https://placeholder.service-now.com'\nauth_method = 'api_key'\n",
    );
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_db_object"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": []})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            {"element": "sys_id", "internal_type": "string", "read_only": "true"},
            {"element": "short_description", "internal_type": "string", "mandatory": "true"}
        ]})))
        .mount(&server)
        .await;
    let artifacts = tempfile::tempdir().unwrap();
    let file = artifacts.path().join("dataset.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({
            "version": 1, "kind": "table-export", "instance": server.uri(), "table": "incident",
            "records": [{"sys_id": "private-record-value", "unknown": "private-field-value"}]
        }))
        .unwrap(),
    )
    .unwrap();
    let result = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config)
        .env("SNOW_CLI_API_TOKEN", "test-token")
        .args([
            "--instance",
            &server.uri(),
            "data",
            "import",
            "--file",
            file.to_str().unwrap(),
            "--dry-run",
        ])
        .assert()
        .code(2)
        .get_output()
        .clone();
    assert!(result.stdout.is_empty());
    let error: Value = serde_json::from_slice(&result.stderr).unwrap();
    assert_eq!(error["error"]["code"], "VALIDATION_ERROR");
    assert_eq!(error["error"]["issues"].as_array().unwrap().len(), 3);
    assert_eq!(error["error"]["issues"][0]["kind"], "field_not_writable");
    assert_eq!(error["error"]["issues"][2]["record_index"], 0);
    assert!(
        !String::from_utf8(result.stderr)
            .unwrap()
            .contains("private-")
    );
    assert!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.method == "GET")
    );
}

#[tokio::test]
async fn import_ready_export_round_trips_without_changing_archival_export() {
    let server = MockServer::start().await;
    let (_dir, config) = common::create_temp_config(
        "default_profile = 'default'\n[profiles.default]\ninstance = 'https://placeholder.service-now.com'\nauth_method = 'api_key'\n",
    );
    let records = json!([{
        "sys_id": "source-id", "short_description": "Representative incident",
        "computed": "derived", "sys_tags": "tag", "comments": "journal history",
        "location": "source-location-id"
    }]);
    for (table, result) in [
        ("incident", records.clone()),
        ("sys_db_object", json!([])),
        (
            "sys_dictionary",
            json!([
                {"element": "short_description", "internal_type": "string", "mandatory": "true"},
                {"element": "computed", "internal_type": "string", "read_only": "true"},
                {"element": "comments", "internal_type": "journal"},
                {"element": "location", "internal_type": "reference"}
            ]),
        ),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/api/now/table/{table}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": result})))
            .mount(&server)
            .await;
    }
    let artifact_dir = tempfile::tempdir().unwrap();
    let artifact_path = artifact_dir.path().join("import-ready.json");
    for binary in ["snow-cli", "snow-cli-ro"] {
        let mut command = if binary == "snow-cli" {
            cargo_bin_cmd!("snow-cli")
        } else {
            cargo_bin_cmd!("snow-cli-ro")
        };
        command
            .env("SNOW_CLI_CONFIG", &config)
            .env("SNOW_CLI_API_TOKEN", "test-token")
            .args([
                "--instance",
                &server.uri(),
                "data",
                "export",
                "incident",
                "--import-ready",
                "--out",
                artifact_path.to_str().unwrap(),
            ])
            .assert()
            .success();
        let artifact: Value =
            serde_json::from_slice(&std::fs::read(&artifact_path).unwrap()).unwrap();
        assert_eq!(
            artifact["records"],
            json!([{"short_description": "Representative incident", "location": "source-location-id"}])
        );
        assert_eq!(
            artifact["preparation"]["omitted_fields"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert_eq!(
            artifact["preparation"]["reference_policy"],
            "source_values_unchanged"
        );
    }
    let result = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config)
        .env("SNOW_CLI_API_TOKEN", "test-token")
        .args([
            "--instance",
            &server.uri(),
            "data",
            "import",
            "--file",
            artifact_path.to_str().unwrap(),
            "--dry-run",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&result).unwrap();
    assert_eq!(report["validation"]["ready"], true);
    let archival = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config)
        .env("SNOW_CLI_API_TOKEN", "test-token")
        .args(["--instance", &server.uri(), "data", "export", "incident"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let archival: Value = serde_json::from_slice(&archival).unwrap();
    assert_eq!(archival["records"], records);
    assert!(archival.get("preparation").is_none());
    assert!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.method == "GET")
    );
}
