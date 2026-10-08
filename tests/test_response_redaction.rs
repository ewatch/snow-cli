//! Regression coverage for errors that formerly echoed entire server responses.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn move_file_non_json_failure_does_not_echo_script_output() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/sys.scripts.modern.do"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", "JSESSIONID=test-session; Path=/; HttpOnly")
                .set_body_string("<script>g_ck='test-csrf';</script>"),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/sys.scripts.do"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("<HTML><BODY>private record; g_ck='session-secret'</BODY></HTML>"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config) = common::create_temp_config(
        "default_profile = 'default'\n[profiles.default]\ninstance = 'https://placeholder.service-now.com'\nauth_method = 'api_key'\n",
    );
    let assertion = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", config)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .env_remove("SNOW_CLI_DEBUG_HTTP_INCLUDE_SENSITIVE")
        .args([
            "--instance",
            &server.uri(),
            "scope",
            "move-file",
            "sys_script_include",
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6",
            "--target-scope",
            "x_target",
            "--dry-run",
        ])
        .assert()
        .failure();
    let output = assertion.get_output();
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("returned non-JSON output"), "{stderr}");
    assert!(stderr.contains("response body redacted"), "{stderr}");
    assert!(!stderr.contains("private record"), "{stderr}");
    assert!(!stderr.contains("session-secret"), "{stderr}");
}

#[tokio::test]
async fn command_http_errors_do_not_echo_response_bodies() {
    for command in ["script", "upload", "download"] {
        let server = MockServer::start().await;
        let endpoint = match command {
            "script" => "/api/test/script",
            "upload" => "/api/now/attachment/upload",
            _ => "/api/now/attachment/615ea769c0a80166001cf5f2367302f5/file",
        };
        let secret_body = "<html>private record; g_ck='session-secret'; password=secret</html>";
        Mock::given(method(if command == "download" { "GET" } else { "POST" }))
            .and(path(endpoint))
            .respond_with(ResponseTemplate::new(403).set_body_string(secret_body))
            .expect(1)
            .mount(&server)
            .await;
        if command == "download" {
            Mock::given(method("GET"))
                .and(path("/api/now/attachment/615ea769c0a80166001cf5f2367302f5"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "result": {
                        "sys_id": "615ea769c0a80166001cf5f2367302f5",
                        "file_name": "test.txt",
                        "download_link": endpoint,
                        "content_type": "text/plain", "size_bytes": "12",
                        "table_name": "incident", "table_sys_id": "6816f79cc0a8016401c5a33be04be441"
                    }
                })))
                .mount(&server)
                .await;
        }
        let (dir, config) = common::create_temp_config(
            "default_profile = 'default'\n[profiles.default]\ninstance = 'https://placeholder.service-now.com'\nauth_method = 'api_key'\n",
        );
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "test attachment").unwrap();
        let mut cmd = cargo_bin_cmd!("snow-cli");
        cmd.env("SNOW_CLI_CONFIG", config)
            .env("SNOW_CLI_API_TOKEN", "test-api-token")
            .env_remove("SNOW_CLI_DEBUG_HTTP_INCLUDE_SENSITIVE")
            .args(["--instance", &server.uri()]);
        match command {
            "script" => {
                cmd.args([
                    "script",
                    "run",
                    "--code",
                    "gs.print(1)",
                    "--endpoint",
                    endpoint,
                ]);
            }
            "upload" => {
                cmd.args([
                    "attachment",
                    "upload",
                    "incident",
                    "6816f79cc0a8016401c5a33be04be441",
                    "--file",
                ])
                .arg(&file);
            }
            _ => {
                cmd.args([
                    "attachment",
                    "download",
                    "615ea769c0a80166001cf5f2367302f5",
                    "--out",
                ])
                .arg(dir.path().join("download.txt"));
            }
        }
        let assertion = cmd.assert().failure();
        let output = assertion.get_output();
        assert!(output.stdout.is_empty(), "{command}: unexpected stdout");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("session-secret"), "{command}: {stderr}");
        assert!(!stderr.contains("private record"), "{command}: {stderr}");
        let error: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
        assert_eq!(error["error"]["code"], "FORBIDDEN", "{command}");
        assert_eq!(error["error"]["status"], 403, "{command}");
        assert!(
            error["error"]["detail"]
                .as_str()
                .unwrap()
                .contains("response body redacted")
        );
    }
}
