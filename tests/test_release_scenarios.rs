//! Execute the domain/native-session TOML matrix through the real CLI and runner.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn domain_and_session_scenarios_execute_and_clean_up_with_mock_instance() {
    let server = MockServer::start().await;
    let user = "6816f79cc0a8016401c5a33be04be441";
    let update = "11111111111111111111111111111111";
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result":[]})))
        .with_priority(10)
        .mount(&server)
        .await;
    for (endpoint, result) in [
        (
            "/api/now/table/sys_user".to_string(),
            json!([{"sys_id":user,"user_name":"admin","sys_domain":"global"}]),
        ),
        (
            format!("/api/now/table/sys_user/{user}"),
            json!({"sys_domain":"global"}),
        ),
        (
            "/api/now/table/sys_db_object".to_string(),
            json!([{"name":"incident","super_class":""}]),
        ),
        (
            "/api/now/table/sys_dictionary".to_string(),
            json!([{"name":"incident","element":"sys_domain","internal_type":"reference"}]),
        ),
        (
            "/api/now/ui/user/current_user".to_string(),
            json!({"user_sys_id":user,"user_name":"admin"}),
        ),
        (
            "/api/now/ui/concoursepicker/current".to_string(),
            json!({"currentDomain":{"value":"global"},"currentApplication":{"sysId":"global","scopeName":"global"},"currentUpdateSet":{"sysId":update}}),
        ),
        (
            "/api/now/ui/concoursepicker/application".to_string(),
            json!({"list":[{"sysId":"global","scopeName":"global"}]}),
        ),
    ] {
        Mock::given(method("GET"))
            .and(path(endpoint))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result":result})))
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/navpage.do"))
        .respond_with(ResponseTemplate::new(200).set_body_string("g_ck='scenario-csrf-secret';"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/login.do"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", "/navpage.do")
                .insert_header(
                    "Set-Cookie",
                    "JSESSIONID=scenario-cookie-secret; Path=/; HttpOnly",
                ),
        )
        .expect(5)
        .mount(&server)
        .await;
    // The live switch scenario deliberately verifies an already-current option.
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("keychain.json");
    let artifacts = fixture.path().join("artifacts");
    let output = std::process::Command::new("bash")
        .arg(repo.join("scripts/e2e-run"))
        .arg(repo.join("tests/e2e/scenarios/domain"))
        .arg(repo.join("tests/e2e/scenarios/session"))
        .current_dir(repo)
        .env("SNOW_CLI_BIN", assert_cmd::cargo::cargo_bin!("snow-cli"))
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", &store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .env("SNOW_E2E_ARTIFACTS_DIR", &artifacts)
        .env("SNOW_E2E_INSTANCE_URL", server.uri())
        .env("SNOW_E2E_USERNAME", "admin")
        .env("SNOW_E2E_PASSWORD", "scenario-password-secret")
        .env("SNOW_E2E_NATIVE_SESSION", "1")
        .env("SNOW_E2E_DOMAIN_SEPARATION", "1")
        .env("SNOW_E2E_SN_UTILS", "0")
        .env_remove("SNOW_CLI_PASSWORD")
        .env_remove("SNOW_CLI_API_TOKEN")
        .env_remove("SNOW_SESSION_COOKIE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let results = std::fs::read_to_string(artifacts.join("results.jsonl")).unwrap();
    assert_eq!(results.lines().count(), 10);
    for line in results.lines() {
        let result: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(result["status"], "pass", "{line}");
        assert_eq!(result["cleanup_warnings"], json!([]), "{line}");
    }
    for secret in [
        "scenario-password-secret",
        "scenario-cookie-secret",
        "scenario-csrf-secret",
    ] {
        assert!(!results.contains(secret));
    }
    for (profile, credential) in [("e2e-domain", "password"), ("e2e-native", "native_session")] {
        assert!(
            common::read_test_keychain_entry(
                &store,
                "snow-cli",
                &format!("{profile}:{credential}")
            )
            .is_none()
        );
    }
}
