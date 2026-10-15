//! HTTP-successful Script Background responses must not hide execution failure.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn script_failures_exit_nonzero_with_safe_structured_stderr() {
    for (index, marker) in [
        "KittyScript validation failed: Syntax error: Expected end of expression",
        "Script compilation error: syntax error",
        "Evaluator.evaluateString() problem: runtime failure",
        "Background message, type:error, message: failed",
        "java.lang.RuntimeException: script failed",
        "org.mozilla.javascript.EvaluatorException: syntax error",
        "<span class='error'>KittyScript validation failed</span>",
        "Caused by: java.lang.IllegalArgumentException: rejected",
    ]
    .into_iter()
    .enumerate()
    {
        let format = ["json", "jsonl", "toon", "csv", "text", "auto"][index % 6];
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
        // Include apparently usable JSON after the failure. Detection must
        // happen before the existing JSON-extraction shortcut discards errors.
        Mock::given(method("POST"))
            .and(path("/sys.scripts.do"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                "<HTML><BODY><pre>{marker}: private customer data g_ck=csrf-secret\n\tat com.glide.Script.run(Script.java:1)\n</pre>*** Script: {{\"ok\":true}}</BODY></HTML>"
            )))
            .expect(1).mount(&server).await;
        let (_dir, config) = common::create_temp_config(&format!(
            "default_profile = 'default'\n[profiles.default]\ninstance = '{instance}'\nauth_method = 'api_key'\n",
            instance = server.uri()
        ));
        let assertion = cargo_bin_cmd!("snow-cli")
            .env("SNOW_CLI_CONFIG", config)
            .env("SNOW_CLI_API_TOKEN", "test-api-token")
            .env("SNOW_CLI_DEBUG_HTTP_INCLUDE_SENSITIVE", "1")
            .args([
                "--instance",
                &server.uri(),
                "--output",
                format,
                "script",
                "run",
                "--sandbox",
            ])
            .write_stdin("gs.print(1); gs.print(2);")
            .assert()
            .code(1);
        let output = assertion.get_output();
        assert!(output.stdout.is_empty(), "{marker}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let error_line = stderr.lines().last().unwrap();
        let error: Value = serde_json::from_str(error_line).unwrap();
        assert_eq!(error["error"]["code"], "SCRIPT_EXECUTION_FAILED");
        assert!(!stderr.contains("csrf-secret"), "{stderr}");
        assert!(!stderr.contains("private customer data"), "{stderr}");
        assert!(!stderr.contains("Script.java"), "{stderr}");
    }
}

#[tokio::test]
async fn printed_error_words_and_json_data_are_not_execution_failures() {
    for response in [
        "*** Script: Script compilation error: example text",
        "*** Script: java.lang.RuntimeException: example text",
        "*** Script: error\n*** Script: failed\n*** Script: Syntax error: example",
        "*** Script: {\"message\":\"KittyScript validation failed\",\"error\":\"example\"}",
        "Background message, type:info, message: type:error is an example",
        "Background message, type:errors, message: ordinary data",
        "<span>*** Script: KittyScript validation failed: example</span>",
    ] {
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
                    .set_body_string(format!("<HTML><BODY>{response}</BODY></HTML>")),
            )
            .mount(&server)
            .await;
        let (_dir, config) = common::create_temp_config(&format!(
            "default_profile = 'default'\n[profiles.default]\ninstance = '{instance}'\nauth_method = 'api_key'\n",
            instance = server.uri()
        ));
        cargo_bin_cmd!("snow-cli")
            .env("SNOW_CLI_CONFIG", config)
            .env("SNOW_CLI_API_TOKEN", "test-api-token")
            .args([
                "--instance",
                &server.uri(),
                "script",
                "run",
                "--code",
                "gs.print('example')",
            ])
            .assert()
            .success();
    }
}
