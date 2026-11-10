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
    // The sys_id row proves the dictionary was readable.
    let mut rows = vec![serde_json::json!({"name":"task","element":"sys_id"})];
    if has_domain {
        rows.push(serde_json::json!({"name":"task","element":"sys_domain"}));
    }
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .and(query_param(
            "sysparm_query",
            "nameINincident,task^elementINsys_domain,sys_domain_path,sys_overrides,sys_id",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": rows})))
        .mount(server)
        .await;
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

const ROLE_QUERY: &str =
    "user=javascript:gs.getUserID()^role.nameINadmin,query_no_domain_table_api^state=active";

async fn session_roles(server: &MockServer, query: &str, held: bool) {
    let rows = if held {
        vec![serde_json::json!({"sys_id": "0123456789abcdef0123456789abcdef"})]
    } else {
        vec![]
    };
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_user_has_role"))
        .and(query_param("sysparm_query", query))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": rows})))
        .mount(server)
        .await;
}

fn domain_cmd(
    binary: &str,
    config_path: &std::path::Path,
    server: &MockServer,
) -> assert_cmd::Command {
    let mut cmd = match binary {
        "ro" => cargo_bin_cmd!("snow-cli-ro"),
        _ => cargo_bin_cmd!("snow-cli"),
    };
    cmd.env("SNOW_CLI_CONFIG", config_path)
        .env("SNOW_CLI_API_TOKEN", "token")
        .args(["--instance", &server.uri()]);
    cmd
}

#[tokio::test]
async fn table_diagnostic_distinguishes_unseparated_from_unknown() {
    let server = MockServer::start().await;
    metadata(&server, false).await;
    let (_dir, config_path) = config(&server.uri());
    domain_cmd("ro", &config_path, &server)
        .args(["domain", "table", "incident"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"has_sys_domain\":false"))
        .stdout(predicate::str::contains("\"element\":\"sys_id\"").not());

    // An unreadable dictionary returns no rows at all, not even the probe.
    let unreadable = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_db_object"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": []})))
        .mount(&unreadable)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": []})))
        .mount(&unreadable)
        .await;
    let (_dir, config_path) = config(&unreadable.uri());
    domain_cmd("ro", &config_path, &unreadable)
        .args(["domain", "table", "incident"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"has_sys_domain\":null"));
}

async fn grouped_stats(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/now/stats/incident"))
        .and(query_param("sysparm_group_by", "sys_domain"))
        .and(query_param("sysparm_display_value", "all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": [
            {"stats": {"count": "67"}, "groupby_fields": [{"field": "sys_domain", "value": "global", "display_value": "global"}]},
            {"stats": {"count": "3"}, "groupby_fields": [{"field": "sys_domain", "value": "c90d4b084a362312013398f051272c0d", "display_value": "TOP/ACME"}]}
        ]})))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn counts_without_cross_domain_role_cover_visible_domains_only() {
    let server = MockServer::start().await;
    metadata(&server, true).await;
    grouped_stats(&server).await;
    session_roles(&server, ROLE_QUERY, false).await;
    let (_dir, config_path) = config(&server.uri());
    let output = domain_cmd("ro", &config_path, &server)
        .args(["domain", "table", "incident", "--counts"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let counts = &value["counts"];
    assert_eq!(counts["scope"], "visible_domains");
    assert_eq!(counts["total"], 70);
    assert_eq!(counts["by_domain"][1]["domain"], "ACME");
    assert_eq!(counts["by_domain"][1]["display"], "TOP/ACME");
    assert_eq!(counts["by_domain"][1]["count"], 3);
    assert_eq!(counts["cross_domain"]["available"], false);
    assert!(counts["cross_domain"]["hidden"].is_null());
}

#[tokio::test]
async fn counts_with_cross_domain_role_report_hidden_records() {
    let server = MockServer::start().await;
    metadata(&server, true).await;
    grouped_stats(&server).await;
    session_roles(&server, ROLE_QUERY, true).await;
    for (bypass, total) in [(false, "67"), (true, "68")] {
        let mock = Mock::given(method("GET")).and(path("/api/now/table/incident"));
        let mock = if bypass {
            mock.and(query_param("sysparm_query_no_domain", "true"))
        } else {
            mock.and(query_param_is_missing("sysparm_query_no_domain"))
        };
        mock.respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Total-Count", total)
                .set_body_json(serde_json::json!({"result": [{"sys_id": "a"}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    }
    let (_dir, config_path) = config(&server.uri());
    let output = domain_cmd("ro", &config_path, &server)
        .args(["domain", "table", "incident", "--counts"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let cross = &value["counts"]["cross_domain"];
    assert_eq!(cross["available"], true);
    assert_eq!(cross["visible_total"], 67);
    assert_eq!(cross["all_total"], 68);
    assert_eq!(cross["hidden"], 1);
}

#[tokio::test]
async fn script_counts_are_denied_read_only_and_require_admin() {
    let server = MockServer::start().await;
    let (_dir, config_path) = config(&server.uri());
    domain_cmd("ro", &config_path, &server)
        .args(["domain", "table", "incident", "--counts", "--via-script"])
        .assert()
        .code(7)
        .stderr(predicate::str::contains("POLICY_DENIED"));
    assert!(server.received_requests().await.unwrap().is_empty());

    metadata(&server, true).await;
    session_roles(
        &server,
        "user=javascript:gs.getUserID()^role.nameINadmin^state=active",
        false,
    )
    .await;
    domain_cmd("full", &config_path, &server)
        .args(["domain", "table", "incident", "--counts", "--via-script"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("VALIDATION_ERROR"))
        .stderr(predicate::str::contains("admin role"));
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .all(|r| !r.url.path().contains("sys.scripts"))
    );
}

#[tokio::test]
async fn with_domain_names_records_and_summarizes_the_page() {
    let server = MockServer::start().await;
    let acme = "c90d4b084a362312013398f051272c0d";
    let hidden = "09ff3d105f231000b12e3572f2b4775d";
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(query_param(
            "sysparm_fields",
            "sys_id,number,short_description,state,priority,assigned_to,sys_updated_on,sys_domain,sys_domain_path",
        ))
        .respond_with(ResponseTemplate::new(200).insert_header("X-Total-Count", "3").set_body_json(
            serde_json::json!({"result": [
                {"number": "INC1", "sys_domain": {"link": "x", "value": "global"}},
                {"number": "INC2", "sys_domain": {"link": "x", "value": acme}},
                {"number": "INC3", "sys_domain": {"link": "x", "value": hidden}}
            ]}),
        ))
        .mount(&server)
        .await;
    // The session can read ACME's domain record but not the other one.
    Mock::given(method("GET"))
        .and(path("/api/now/table/domain"))
        .and(query_param(
            "sysparm_query",
            format!("sys_idIN{hidden},{acme}"),
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"result": [{"sys_id": acme, "name": "ACME"}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config_path) = config(&server.uri());
    let output = domain_cmd("ro", &config_path, &server)
        .args([
            "table",
            "list",
            "incident",
            "--with-domain",
            "--output",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let names: Vec<_> = value["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["sys_domain_name"].clone())
        .collect();
    assert_eq!(
        names,
        vec![
            serde_json::json!("global"),
            serde_json::json!("ACME"),
            serde_json::Value::Null
        ]
    );
    assert_eq!(
        value["domains_in_page"],
        serde_json::json!({"global": 1, "ACME": 1, hidden: 1})
    );
}

#[tokio::test]
async fn query_no_domain_warns_when_the_role_is_missing() {
    let server = MockServer::start().await;
    session_roles(&server, ROLE_QUERY, false).await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Total-Count", "0")
                .set_body_json(serde_json::json!({"result": []})),
        )
        .mount(&server)
        .await;
    let (_dir, config_path) = config(&server.uri());
    domain_cmd("ro", &config_path, &server)
        .args(["table", "list", "incident", "--query-no-domain"])
        .assert()
        .success()
        .stderr(predicate::str::contains("--query-no-domain has no effect"));
}

#[tokio::test]
async fn status_infers_separation_from_a_tenant_home_domain() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_properties"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": []})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": [
            {"sys_id": "a", "user_name": "acme.user", "sys_domain": {"link": "x", "value": "c90d4b084a362312013398f051272c0d"}}
        ]})))
        .mount(&server)
        .await;
    let (_dir, config_path) = config(&server.uri());
    domain_cmd("ro", &config_path, &server)
        .args(["domain", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"enabled\":null"))
        .stdout(predicate::str::contains("\"separation_in_use\":true"));
}
