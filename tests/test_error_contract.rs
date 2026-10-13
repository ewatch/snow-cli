//! Error classes, exit codes, retry guidance, and redaction at the binary boundary.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "integration tests intentionally panic on assertion/fixture failures"
)]
mod common;
#[path = "common/response_contract.rs"]
#[allow(
    dead_code,
    reason = "shared response decoders are used by the success contract matrix"
)]
mod contract;

use contract::{BINARIES, FORMATS, command, error, no_credentials};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

fn config() -> (tempfile::TempDir, std::path::PathBuf) {
    common::create_temp_config(
        "default_profile = 'default'\n[profiles.default]\ninstance = 'https://placeholder.service-now.com'\nauth_method = 'api_key'\n",
    )
}

fn assert_contract(output: &std::process::Output, code: &str, class: &str, retry: &str, exit: i32) {
    assert_eq!(
        output.status.code(),
        Some(exit),
        "{code}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    no_credentials(output);
    let value = error(&output.stderr);
    assert_eq!(value["error"]["code"], code);
    assert_eq!(value["error"]["class"], class);
    assert_eq!(value["error"]["retry"], retry);
}

#[tokio::test]
async fn local_errors_are_classified_without_payloads_or_network_calls() {
    let (dir, config) = config();
    let server = MockServer::start().await;
    let missing = dir.path().join("private-path-secret.json");
    for binary in BINARIES {
        for format in FORMATS {
            for (args, code, class, exit) in [
                (
                    vec!["table", "list", "incident", "--preview"],
                    "VALIDATION_ERROR",
                    "validation",
                    2,
                ),
                (
                    vec!["--profile", "missing", "table", "list", "incident"],
                    "PROFILE_NOT_FOUND",
                    "config",
                    2,
                ),
                (
                    vec!["data", "validate", "--file", missing.to_str().unwrap()],
                    "IO_ERROR",
                    "io",
                    6,
                ),
            ] {
                let output_format = if args.contains(&"--preview") {
                    "csv"
                } else if class == "io" && format == "csv" {
                    "json"
                } else {
                    format
                };
                let output = command(binary, &config, &server.uri(), output_format)
                    .args(&args)
                    .assert()
                    .failure()
                    .get_output()
                    .clone();
                assert_contract(
                    &output,
                    code,
                    class,
                    if class == "io" { "unknown" } else { "never" },
                    exit,
                );
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(!stderr.contains("private-url-secret"));
                assert!(!stderr.contains("private-path-secret"));
            }
        }
    }
    for binary in BINARIES {
        let output = command(
            binary,
            &config,
            "https://user:private-url-secret@dev.service-now.com",
            "json",
        )
        .args(["table", "list", "incident"])
        .assert()
        .failure()
        .get_output()
        .clone();
        assert_contract(&output, "INVALID_INSTANCE", "config", "never", 2);
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-url-secret"));
    }
    for args in [
        vec![
            "table",
            "create",
            "incident",
            "--data",
            "{private-payload-secret",
        ],
        vec![
            "import-set",
            "load",
            "u_import",
            "--data",
            "{private-payload-secret",
        ],
        vec![
            "graphql",
            "--query",
            "{private-query-secret}",
            "--variables",
            "{private-payload-secret",
        ],
        vec!["api", "get", "/api/contract", "-H", "private-header-secret"],
    ] {
        let output = command("snow-cli", &config, &server.uri(), "json")
            .args(&args)
            .assert()
            .failure()
            .get_output()
            .clone();
        assert_contract(&output, "VALIDATION_ERROR", "validation", "never", 2);
        for private in [
            "private-payload-secret",
            "private-query-secret",
            "private-header-secret",
        ] {
            assert!(!String::from_utf8_lossy(&output.stderr).contains(private));
        }
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn malformed_configuration_never_echoes_toml_excerpts() {
    for contents in [
        "default_profile = 'default'\n[profiles.default]\ninstance = 'https://dev.service-now.com'\npassword = 'private-config-secret'\n",
        "default_profile = 'private-config-secret' !!!",
    ] {
        let (_dir, config) = common::create_temp_config(contents);
        for binary in BINARIES {
            let output = command(binary, &config, "http://127.0.0.1:1", "json")
                .args(["table", "list", "incident"])
                .assert()
                .failure()
                .get_output()
                .clone();
            assert_contract(&output, "CONFIG_INVALID", "config", "never", 2);
            assert!(!String::from_utf8_lossy(&output.stderr).contains("private-config-secret"));
        }
    }
}

#[tokio::test]
async fn missing_credentials_are_auth_errors_not_internal_errors() {
    let (_dir, config) = config();
    let (_keychain_dir, store) = common::create_temp_keychain_store();
    let server = MockServer::start().await;
    for binary in BINARIES {
        let output = command(binary, &config, &server.uri(), "json")
            .env_remove("SNOW_CLI_API_TOKEN")
            .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &store)
            .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
            .args(["table", "list", "incident"])
            .assert()
            .failure()
            .get_output()
            .clone();
        assert_contract(&output, "AUTH_FAILED", "auth", "never", 3);
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn status_codes_have_retry_guidance_in_every_format() {
    let (_dir, config) = config();
    for (status, code, retry, exit) in [
        (400, "BAD_REQUEST", "never", 5),
        (401, "UNAUTHORIZED", "never", 5),
        (403, "FORBIDDEN", "never", 5),
        (404, "NOT_FOUND", "never", 4),
        (429, "RATE_LIMITED", "safe_reads_only", 5),
        (503, "SERVER_ERROR", "safe_reads_only", 5),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/now/table/incident"))
            .respond_with(ResponseTemplate::new(status).set_body_string("private-http-body-secret"))
            .expect((BINARIES.len() * FORMATS.len()) as u64)
            .mount(&server)
            .await;
        for binary in BINARIES {
            for format in FORMATS {
                let output = command(binary, &config, &server.uri(), format)
                    .env("SNOW_CLI_DEBUG_HTTP_INCLUDE_SENSITIVE", "1")
                    .args(["table", "list", "incident"])
                    .assert()
                    .failure()
                    .get_output()
                    .clone();
                assert_contract(&output, code, "api", retry, exit);
                assert_eq!(error(&output.stderr)["error"]["status"], status);
                assert!(
                    !String::from_utf8_lossy(&output.stderr).contains("private-http-body-secret")
                );
            }
        }
    }
}

#[tokio::test]
async fn transport_failure_never_includes_queries_or_request_urls() {
    let (_dir, config) = config();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(2))
                .set_body_string("private-response-secret"),
        )
        .expect(2)
        .mount(&server)
        .await;
    for binary in BINARIES {
        let output = command(binary, &config, &server.uri(), "json")
            .args([
                "--timeout-secs",
                "1",
                "table",
                "list",
                "incident",
                "--query",
                "description=private-query-secret",
            ])
            .assert()
            .failure()
            .get_output()
            .clone();
        assert_contract(
            &output,
            "REQUEST_TIMEOUT",
            "transport",
            "safe_reads_only",
            5,
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("private-query-secret"));
        assert!(!stderr.contains("private-response-secret"));
        assert!(!stderr.contains("sysparm_query"));
        assert!(!stderr.contains(&server.uri()));
    }
}

#[tokio::test]
async fn malformed_server_json_is_not_misclassified_as_local_validation() {
    let (_dir, config) = config();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{private-response-secret"))
        .expect(2)
        .mount(&server)
        .await;
    for binary in BINARIES {
        let output = command(binary, &config, &server.uri(), "json")
            .args(["table", "list", "incident"])
            .assert()
            .failure()
            .get_output()
            .clone();
        assert_contract(&output, "INVALID_RESPONSE", "transport", "unknown", 5);
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-response-secret"));
    }
}

#[tokio::test]
async fn policy_denial_is_non_retryable_and_precedes_config_resolution() {
    let (dir, _) = config();
    let output = command(
        "snow-cli",
        &dir.path().join("missing.toml"),
        "http://127.0.0.1:1",
        "json",
    )
    .args(["--read-only", "table", "create", "incident", "--data", "{}"])
    .assert()
    .failure()
    .get_output()
    .clone();
    assert_contract(&output, "POLICY_DENIED", "policy", "never", 7);
}

#[tokio::test]
async fn safe_server_envelopes_do_not_leak_query_or_script_diagnostics() {
    let (_dir, config) = config();
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({"error": {
            "message": "Invalid request parameters",
            "detail": "script: gs.print('private-script-secret'); query=private-query-secret; password=private-password-secret"
        }}))).expect(2).mount(&server).await;
    for binary in BINARIES {
        let output = command(binary, &config, &server.uri(), "json")
            .args(["table", "list", "incident"])
            .assert()
            .failure()
            .get_output()
            .clone();
        assert_contract(&output, "BAD_REQUEST", "api", "never", 5);
        for secret in [
            "private-script-secret",
            "private-query-secret",
            "private-password-secret",
        ] {
            assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
        }
    }
}

#[tokio::test]
async fn failed_mutations_never_receive_unconditional_retry_permission() {
    let (_dir, config) = config();
    let server = MockServer::start().await;
    Mock::given(method("POST")).and(path("/api/now/import/u_import"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": [
            {"status": "inserted", "sys_id": "created123"}, {"status": "error", "error_message": "bad row"}
        ]}))).expect(1).mount(&server).await;
    let output = command("snow-cli", &config, &server.uri(), "json")
        .args(["import-set", "load", "u_import", "--data", "{}"])
        .assert()
        .code(1)
        .get_output()
        .clone();
    let value = error(&output.stderr);
    assert_eq!(value["error"]["class"], "command");
    assert_eq!(value["error"]["retry"], "manual_review");
    // Partial mutation results are the explicit stdout exception.
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["summary"]["inserted"], 1);
    no_credentials(&output);
}
