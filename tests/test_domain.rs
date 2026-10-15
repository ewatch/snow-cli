//! Regression tests for domain filters, visibility opt-in, and incomplete metadata.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn config(instance: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    common::create_temp_config(&format!(
        "default_profile = 'default'\n[profiles.default]\ninstance = '{instance}'\nauth_method = 'api_key'\n"
    ))
}

#[tokio::test]
async fn zero_domain_limit_is_validation_error_without_requests() {
    let server = MockServer::start().await;
    let (_dir, config_path) = config(&server.uri());
    for args in [
        vec!["domain", "list", "--limit", "0"],
        vec!["domain", "inspect", "global", "--limit", "0"],
    ] {
        let result = cargo_bin_cmd!("snow-cli")
            .env("SNOW_CLI_CONFIG", &config_path)
            .env("SNOW_CLI_API_TOKEN", "test-token")
            .args(["--instance", &server.uri()])
            .args(args)
            .assert()
            .code(2)
            .stdout("");
        let error: serde_json::Value = serde_json::from_slice(&result.get_output().stderr).unwrap();
        assert_eq!(error["error"]["code"], "VALIDATION_ERROR");
        assert_eq!(error["error"]["class"], "validation");
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

async fn metadata(server: &MockServer, has_domain: bool) {
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_db_object"))
        .and(query_param("sysparm_query", "name=incident"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": [
                {"name":"incident", "super_class":{"value":"0123456789abcdef0123456789abcdef"}}
            ]})),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_db_object"))
        .and(query_param(
            "sysparm_query",
            "sys_id=0123456789abcdef0123456789abcdef",
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": [
                {"name":"task", "super_class":""}
            ]})),
        )
        .mount(server)
        .await;
    Mock::given(method("GET")).and(path("/api/now/table/sys_dictionary"))
        .and(query_param("sysparm_query", "nameINincident,task^elementINsys_domain,sys_domain_path,sys_overrides"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result":
            if has_domain { vec![serde_json::json!({"name":"task","element":"sys_domain"})] } else { vec![] }
        }))).mount(server).await;
}

#[tokio::test]
async fn domain_filter_constrains_nq_and_is_available_read_only() {
    let server = MockServer::start().await;
    metadata(&server, true).await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param(
            "sysparm_query",
            "active=true^sys_domain=global^NQpriority=1^sys_domain=global",
        ))
        .and(query_param_is_missing("sysparm_query_no_domain"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result":[]})))
        .expect(2)
        .mount(&server)
        .await;
    let (_dir, config_path) = config(&server.uri());
    for binary in ["snow-cli", "snow-cli-ro"] {
        let mut command = if binary == "snow-cli" {
            cargo_bin_cmd!("snow-cli")
        } else {
            cargo_bin_cmd!("snow-cli-ro")
        };
        command
            .env("SNOW_CLI_CONFIG", &config_path)
            .env("SNOW_CLI_API_TOKEN", "token")
            .args([
                "--instance",
                &server.uri(),
                "table",
                "list",
                "incident",
                "--domain",
                "global",
                "--query",
                "active=true^NQpriority=1^EQ",
            ])
            .assert()
            .success();
    }
}

#[tokio::test]
async fn unverifiable_domain_field_refuses_target_read() {
    let server = MockServer::start().await;
    metadata(&server, false).await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let (_dir, config_path) = config(&server.uri());
    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "token")
        .args([
            "--instance",
            &server.uri(),
            "table",
            "list",
            "incident",
            "--domain",
            "global",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Cannot verify sys_domain"));
}

#[tokio::test]
async fn query_no_domain_is_sent_on_every_page() {
    let server = MockServer::start().await;
    for (offset, rows) in [
        ("0", vec![serde_json::json!({"sys_id":"1"}); 100]),
        ("100", vec![serde_json::json!({"sys_id":"2"})]),
    ] {
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .and(query_param("sysparm_offset", offset))
            .and(query_param("sysparm_query_no_domain", "true"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"result":rows})),
            )
            .expect(1)
            .mount(&server)
            .await;
    }
    let (_dir, config_path) = config(&server.uri());
    cargo_bin_cmd!("snow-cli-ro")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "token")
        .args([
            "--instance",
            &server.uri(),
            "table",
            "list",
            "incident",
            "--all",
            "--query-no-domain",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"returned\":101"));
}

#[tokio::test]
async fn get_bypass_is_opt_in_and_preserves_server_denial() {
    let server = MockServer::start().await;
    let record_path = "/api/now/table/incident/0123456789abcdef0123456789abcdef";
    Mock::given(method("GET"))
        .and(path(record_path))
        .and(query_param_is_missing("sysparm_query_no_domain"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"result":{"sys_id":"ok"}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(record_path))
        .and(query_param("sysparm_query_no_domain", "true"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(serde_json::json!({"error":{"message":"Not authorized"}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config_path) = config(&server.uri());
    for bypass in [false, true] {
        let mut cmd = cargo_bin_cmd!("snow-cli-ro");
        cmd.env("SNOW_CLI_CONFIG", &config_path)
            .env("SNOW_CLI_API_TOKEN", "token")
            .args([
                "--instance",
                &server.uri(),
                "table",
                "get",
                "incident",
                "0123456789abcdef0123456789abcdef",
            ]);
        if bypass {
            cmd.arg("--query-no-domain").assert().failure();
        } else {
            cmd.assert().success();
        }
    }
}

#[tokio::test]
async fn status_keeps_missing_activation_and_session_unknown() {
    let server = MockServer::start().await;
    for table in ["sys_properties", "sys_user"] {
        Mock::given(method("GET"))
            .and(path(format!("/api/now/table/{table}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"result":[]})),
            )
            .expect(1)
            .mount(&server)
            .await;
    }
    let (_dir, config_path) = config(&server.uri());
    cargo_bin_cmd!("snow-cli-ro")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "token")
        .args(["--instance", &server.uri(), "domain", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"enabled\":null"))
        .stdout(predicate::str::contains("\"session_domain\":null"));
}

#[tokio::test]
async fn table_diagnostic_reports_inherited_field_definition() {
    let server = MockServer::start().await;
    metadata(&server, true).await;
    let (_dir, config_path) = config(&server.uri());
    cargo_bin_cmd!("snow-cli-ro")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "token")
        .args(["--instance", &server.uri(), "domain", "table", "incident"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"has_sys_domain\":true"))
        .stdout(predicate::str::contains(
            "\"hierarchy\":[\"incident\",\"task\"]",
        ));
}

#[tokio::test]
async fn stats_domain_filter_does_not_send_table_api_bypass() {
    let server = MockServer::start().await;
    metadata(&server, true).await;
    Mock::given(method("GET"))
        .and(path("/api/now/stats/incident"))
        .and(query_param("sysparm_query", "sys_domain=global"))
        .and(query_param_is_missing("sysparm_query_no_domain"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"result":{"stats":{"count":"4"}}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config_path) = config(&server.uri());
    cargo_bin_cmd!("snow-cli-ro")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "token")
        .args([
            "--instance",
            &server.uri(),
            "table",
            "stats",
            "incident",
            "--domain",
            "global",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"count\":4"));
}

#[test]
fn reject_domain_query_injection_and_unsupported_bypass() {
    cargo_bin_cmd!("snow-cli")
        .args([
            "table",
            "list",
            "incident",
            "--domain",
            "global^NQactive=true",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid sys_id"));
    cargo_bin_cmd!("snow-cli-ro")
        .args(["table", "stats", "incident", "--query-no-domain"])
        .assert()
        .failure();
}

#[tokio::test]
async fn inspect_reads_contains_and_visibility_without_claiming_effective_access() {
    let server = MockServer::start().await;
    for (table, query) in [
        ("domain", "sys_id=global^ORparent=global"),
        ("domain_contains", "domain=global^ORcontains=global"),
        ("sys_user_visibility", "sys_domain=global"),
        ("sys_user_group_visibility", "sys_domain=global"),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/api/now/table/{table}")))
            .and(query_param("sysparm_query", query))
            .and(query_param("sysparm_query_no_domain", "true"))
            .and(query_param("sysparm_limit", "1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Total-Count", "2")
                    .set_body_json(serde_json::json!({"result":[{"sys_id":"one"}]})),
            )
            .expect(1)
            .mount(&server)
            .await;
    }
    let (_dir, config_path) = config(&server.uri());
    cargo_bin_cmd!("snow-cli-ro")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "token")
        .args([
            "--instance",
            &server.uri(),
            "domain",
            "inspect",
            "global",
            "--limit",
            "1",
            "--query-no-domain",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("domain_contains"))
        .stdout(predicate::str::contains("\"truncated\":true"));
}
