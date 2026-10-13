//! Representative response classes × formats × binaries, without output snapshots.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test assertions and shared fixtures intentionally panic on failure"
)]
mod common;
#[path = "common/response_contract.rs"]
mod contract;

use contract::{BINARIES, FORMATS, command, csv_rows, error, no_credentials, structured};
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

fn config() -> (tempfile::TempDir, std::path::PathBuf) {
    common::create_temp_config(
        "default_profile = 'default'\n[profiles.default]\ninstance = 'https://placeholder.service-now.com'\nauth_method = 'api_key'\n",
    )
}

#[tokio::test]
async fn list_states_formats_and_binaries() {
    let (_dir, config) = config();
    // Unicode, CSV quoting, and fields beyond the preview threshold must survive.
    let description = format!("quoted, \"text\"\n{}END", "é".repeat(2001));
    let record = json!({"sys_id": "abc123", "description": description});
    for (state, records, total) in [
        ("empty", json!([]), 0),
        ("full", json!([record]), 1),
        ("partial", json!([record]), 3),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("X-Total-Count", total.to_string())
                    .set_body_json(json!({"result": records})),
            )
            .expect((BINARIES.len() * FORMATS.len()) as u64)
            .mount(&server)
            .await;
        for binary in BINARIES {
            for format in FORMATS {
                let output = command(binary, &config, &server.uri(), format)
                    .args([
                        "table",
                        "list",
                        "incident",
                        "--limit",
                        "1",
                        "--fields",
                        "sys_id,description",
                    ])
                    .assert()
                    .success()
                    .get_output()
                    .clone();
                assert!(
                    output.stderr.is_empty(),
                    "{binary}/{format}/{state}: {:?}",
                    output.stderr
                );
                no_credentials(&output);
                if format == "csv" {
                    assert_eq!(
                        csv_rows(&output.stdout),
                        records.as_array().unwrap().clone(),
                        "{binary}/{state}"
                    );
                } else {
                    let value = structured(format, &output.stdout);
                    assert_eq!(value["records"], records, "{binary}/{format}/{state}");
                    assert_eq!(value["total"], total);
                    assert_eq!(value["returned"], if state == "empty" { 0 } else { 1 });
                    assert_eq!(value["truncated"], state == "partial");
                    assert!(value.get("preview").is_none());
                    if state == "partial" {
                        assert_eq!(value["next_offset"], 1);
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn detail_aggregate_and_mutation_formats() {
    let (_dir, config) = config();
    // These classes all produce a single record, unlike the list envelope.
    for (class, verb, endpoint, args, response, expected) in [
        (
            "detail",
            "GET",
            "/api/now/table/incident/6816f79cc0a8016401c5a33be04be441",
            vec![
                "table",
                "get",
                "incident",
                "6816f79cc0a8016401c5a33be04be441",
            ],
            json!({"result": {"sys_id": "abc123", "description": "x".repeat(2001)}}),
            json!({"sys_id": "abc123", "description": "x".repeat(2001)}),
        ),
        (
            "aggregate",
            "GET",
            "/api/now/stats/incident",
            vec!["table", "stats", "incident"],
            json!({"result": {"stats": {"count": "0"}}}),
            json!({"count": 0}),
        ),
        (
            "mutation",
            "POST",
            "/api/now/table/incident",
            vec![
                "table",
                "create",
                "incident",
                "--data",
                "{\"short_description\":\"created\"}",
            ],
            json!({"result": {"sys_id": "abc123", "short_description": "created"}}),
            json!({"sys_id": "abc123", "short_description": "created"}),
        ),
    ] {
        let server = MockServer::start().await;
        let runs = if class == "mutation" {
            1
        } else {
            BINARIES.len()
        } * FORMATS.len();
        Mock::given(method(verb))
            .and(path(endpoint))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(runs as u64)
            .mount(&server)
            .await;
        for binary in BINARIES {
            if class == "mutation" && binary == "snow-cli-ro" {
                continue;
            }
            for format in FORMATS {
                let output = command(binary, &config, &server.uri(), format)
                    .args(&args)
                    .assert()
                    .success()
                    .get_output()
                    .clone();
                assert!(output.stderr.is_empty(), "{binary}/{format}/{class}");
                no_credentials(&output);
                if format == "csv" {
                    let rows = csv_rows(&output.stdout);
                    assert_eq!(rows.len(), 1);
                    for (key, value) in expected.as_object().unwrap() {
                        let text = value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string());
                        assert_eq!(rows[0][key], text);
                    }
                } else {
                    assert_eq!(
                        structured(format, &output.stdout),
                        expected,
                        "{binary}/{format}/{class}"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn artifacts_are_lossless_and_summary_respects_format() {
    let (dir, config) = config();
    let record = json!({"sys_id": "abc123", "description": "é".repeat(2001)});
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [record]})))
        .expect((BINARIES.len() * FORMATS.len()) as u64)
        .mount(&server)
        .await;
    for binary in BINARIES {
        for format in FORMATS {
            let artifact = dir.path().join(format!("{binary}-{format}.json"));
            let output = command(binary, &config, &server.uri(), format)
                .args([
                    "data",
                    "export",
                    "incident",
                    "--out",
                    artifact.to_str().unwrap(),
                ])
                .assert()
                .success()
                .get_output()
                .clone();
            assert!(output.stderr.is_empty());
            no_credentials(&output);
            let bytes = std::fs::read(&artifact).unwrap();
            if format == "csv" {
                assert_eq!(csv_rows(&bytes), vec![record.clone()]);
            } else {
                let value = structured(format, &bytes);
                assert_eq!(value["kind"], "table-export");
                assert_eq!(value["records"], json!([record]));
            }
            let summary = if format == "csv" {
                csv_rows(&output.stdout)[0].clone()
            } else {
                structured(format, &output.stdout)
            };
            assert_eq!(summary["kind"], "export-result");
        }
    }
}

#[tokio::test]
async fn previews_are_explicit_and_diagnostics_stay_off_stdout() {
    let (_dir, config) = config();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            {"sys_id": "abc123", "description": "é".repeat(2001)}
        ]})))
        .expect((BINARIES.len() * (FORMATS.len() - 1)) as u64)
        .mount(&server)
        .await;
    for binary in BINARIES {
        for format in FORMATS.into_iter().filter(|format| *format != "csv") {
            let output = command(binary, &config, &server.uri(), format)
                .args(["-v", "table", "list", "incident", "--preview"])
                .assert()
                .success()
                .get_output()
                .clone();
            let value = structured(format, &output.stdout);
            assert_eq!(value["fields_truncated"], true);
            assert_eq!(value["records"][0]["description"], "é".repeat(2000));
            let field = &value["preview"]["truncated_fields"][0];
            assert_eq!(field["original_chars"], 2001);
            assert_eq!(field["shown_chars"], 2000);
            assert_eq!(field["path"], "/records/0/description");
            assert!(field["escape_hatch"].as_str().unwrap().contains("--full"));
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("Listing records from table"));
            assert!(!stderr.contains("\\x1b["));
            assert!(!stderr.contains(&"é".repeat(2000)));
            no_credentials(&output);
        }
    }
}

#[tokio::test]
async fn raw_passthrough_is_an_intentional_format_exception() {
    let (_dir, config) = config();
    let server = MockServer::start().await;
    let body = "plain response, not JSON\nsecond line";
    Mock::given(method("GET"))
        .and(path("/api/contract"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .expect((BINARIES.len() * FORMATS.len()) as u64)
        .mount(&server)
        .await;
    for binary in BINARIES {
        for format in FORMATS {
            let output = command(binary, &config, &server.uri(), format)
                .args(["api", "get", "/api/contract"])
                .assert()
                .success()
                .get_output()
                .clone();
            assert_eq!(output.stdout, format!("{body}\n").as_bytes());
            assert!(output.stderr.is_empty());
            no_credentials(&output);
        }
    }
}

#[tokio::test]
async fn errors_are_structured_on_stderr_in_every_format() {
    let (_dir, config) = config();
    for (status, code) in [
        (403, "FORBIDDEN"),
        (404, "NOT_FOUND"),
        (429, "RATE_LIMITED"),
        (500, "SERVER_ERROR"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(
                ResponseTemplate::new(status).set_body_string("<html>private-record-secret</html>"),
            )
            .expect((BINARIES.len() * FORMATS.len()) as u64)
            .mount(&server)
            .await;
        for binary in BINARIES {
            for format in FORMATS {
                let output = command(binary, &config, &server.uri(), format)
                    .args(["table", "list", "incident"])
                    .assert()
                    .failure()
                    .get_output()
                    .clone();
                assert!(output.stdout.is_empty());
                assert_eq!(
                    error(&output.stderr)["error"]["code"],
                    code,
                    "status {status}"
                );
                assert!(!String::from_utf8_lossy(&output.stderr).contains("private-record-secret"));
                no_credentials(&output);
            }
        }
    }
}

#[tokio::test]
async fn timeout_has_no_result_or_credential_leak() {
    let (_dir, config) = config();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(2))
                .set_body_json(json!({"result": []})),
        )
        .expect(2)
        .mount(&server)
        .await;
    for binary in BINARIES {
        let output = command(binary, &config, &server.uri(), "json")
            .args(["--timeout-secs", "1", "table", "list", "incident"])
            .assert()
            .failure()
            .get_output()
            .clone();
        assert!(output.stdout.is_empty());
        error(&output.stderr);
        no_credentials(&output);
    }
}

#[tokio::test]
async fn discovery_parse_errors_and_read_only_denial_do_not_touch_network() {
    let (_dir, config) = config();
    let server = MockServer::start().await;
    for binary in BINARIES {
        for args in [
            vec!["--help"],
            vec!["table", "list", "--help"],
            vec!["--version"],
        ] {
            let output = command(binary, &config, &server.uri(), "json")
                .args(&args)
                .assert()
                .success()
                .get_output()
                .clone();
            assert!(!output.stdout.is_empty());
            assert!(output.stderr.is_empty());
        }
        let output = command(binary, &config, &server.uri(), "json")
            .arg("--unknown-contract-flag")
            .assert()
            .failure()
            .get_output()
            .clone();
        assert!(output.stdout.is_empty());
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument"));
        let output = command(binary, &config, &server.uri(), "json")
            .assert()
            .code(2)
            .get_output()
            .clone();
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
        let mut cmd = command(binary, &config, &server.uri(), "json");
        if binary == "snow-cli" {
            cmd.arg("--read-only");
        }
        let output = cmd
            .args(["table", "create", "incident", "--data", "{}"])
            .assert()
            .failure()
            .get_output()
            .clone();
        assert!(output.stdout.is_empty());
        if binary == "snow-cli-ro" {
            assert_eq!(output.status.code(), Some(2));
            assert!(String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand"));
        } else {
            assert_eq!(error(&output.stderr)["error"]["code"], "POLICY_DENIED");
        }
        no_credentials(&output);
    }
    for format in FORMATS {
        let output = command("snow-cli", &config, &server.uri(), format)
            .args([
                "table",
                "delete",
                "incident",
                "6816f79cc0a8016401c5a33be04be441",
            ])
            .assert()
            .failure()
            .get_output()
            .clone();
        assert!(output.stdout.is_empty());
        assert!(
            error(&output.stderr)["error"]["message"]
                .as_str()
                .unwrap()
                .contains("--yes")
        );
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn import_partial_failure_keeps_results_and_requires_explicit_acceptance() {
    let (_dir, config) = config();
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/api/now/import/u_contract"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": [
            {"status": "inserted", "sys_id": "abc123"}, {"status": "error", "error_message": "bad row"}
        ]})))
        .expect(((FORMATS.len() - 1) * 2) as u64).mount(&server).await;
    for format in FORMATS.into_iter().filter(|format| *format != "csv") {
        for allow in [false, true] {
            let mut cmd = command("snow-cli", &config, &server.uri(), format);
            cmd.args(["import-set", "load", "u_contract", "--data", "{}"]);
            if allow {
                cmd.arg("--allow-row-errors");
            }
            let assertion = if allow {
                cmd.assert().success()
            } else {
                cmd.assert().code(1)
            };
            let output = assertion.get_output();
            no_credentials(output);
            let value = structured(format, &output.stdout);
            assert_eq!(value["summary"]["error"], 1);
            assert_eq!(value["summary"]["inserted"], 1);
            if allow {
                assert!(String::from_utf8_lossy(&output.stderr).contains("WARN"));
            } else {
                assert_eq!(
                    error(&output.stderr)["error"]["code"],
                    "IMPORT_SET_ROW_ERRORS"
                );
            }
        }
    }
}
