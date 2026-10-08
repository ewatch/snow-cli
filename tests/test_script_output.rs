//! Structured Script Background output must honor requested formats.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn script_execution_log_is_structured_and_text_is_preserved() {
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
    let history = "0123456789abcdef0123456789abcdef";
    let response = format!(
        "<HTML><BODY>[0:00:00.006] Script completed in scope global: script<BR/>Script execution history <A target='blank' HREF='sys_script_execution_history.do?sys_id={history}'>available here</A><HR/><PRE>*** Script: a<BR/>*** Script: {{&quot;ok&quot;:true}}<BR/>*** Script: &lt;foo&gt;bar&lt;/foo&gt;<BR/>*** Script: <BR/></PRE></BODY></HTML>"
    );
    Mock::given(method("POST"))
        .and(path("/sys.scripts.do"))
        .respond_with(ResponseTemplate::new(200).set_body_string(response))
        .mount(&server)
        .await;
    let (_dir, config) = common::create_temp_config(
        "default_profile = 'default'\n[profiles.default]\ninstance = 'https://placeholder.service-now.com'\nauth_method = 'api_key'\n",
    );
    let expected = json!({"output": ["a", "{\"ok\":true}", "<foo>bar</foo>", ""], "duration_ms":6, "scope":"global", "history_sys_id":history, "errors":[]});
    for format in ["json", "jsonl", "toon", "auto", "text", "csv"] {
        let assertion = cargo_bin_cmd!("snow-cli")
            .env("SNOW_CLI_CONFIG", &config)
            .env("SNOW_CLI_API_TOKEN", "test-api-token")
            .args([
                "--instance",
                &server.uri(),
                "--output",
                format,
                "script",
                "run",
                "--code",
                "gs.print('a')",
            ])
            .assert()
            .success();
        let stdout = String::from_utf8_lossy(&assertion.get_output().stdout);
        match format {
            "json" | "jsonl" => {
                assert_eq!(serde_json::from_str::<Value>(&stdout).unwrap(), expected);
                assert_eq!(stdout.lines().count(), 1);
            }
            "auto" => {
                // This envelope normally chooses JSON, but auto may legitimately
                // choose TOON. Compare to rendering the same value at the seam.
                let toon = format!("{}\n", toon_format::encode_default(&expected).unwrap());
                assert!(
                    serde_json::from_str::<Value>(&stdout).ok().as_ref() == Some(&expected)
                        || stdout == toon
                );
            }
            "toon" => {
                let toon = format!("{}\n", toon_format::encode_default(&expected).unwrap());
                assert_eq!(stdout, toon);
            }
            _ => {
                assert!(stdout.contains("Script completed in scope global"));
                assert!(stdout.contains("*** Script: a"));
                assert!(stdout.contains(history));
            }
        }
    }
}
