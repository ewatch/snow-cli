//! Native-session CLI integration: no SN-Utils, no real keychain or instance required.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
const USER: &str = "6816f79cc0a8016401c5a33be04be441";
const APP: &str = "0123456789abcdef0123456789abcdef";
const UPDATE: &str = "11111111111111111111111111111111";

fn config(instance: &str) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let (dir, config_path) = common::create_temp_config(&format!(
        "default_profile='native'\n[profiles.native]\ninstance='{instance}'\nauth_method='native_session'\n[profiles.source]\ninstance='{instance}'\nauth_method='basic'\nusername='admin'\n"
    ));
    let store = dir.path().join("keychain.json");
    common::write_test_keychain_entry(
        &store,
        "snow-cli",
        "native:native_session",
        &json!({
            "instance":instance,"cookie_header":"JSESSIONID=initial-secret; route=node1",
            "csrf_token":"csrf-secret", "authenticated_user":"admin"
        })
        .to_string(),
    );
    (dir, config_path, store)
}
fn command(binary: &str, config: &std::path::Path, store: &std::path::Path) -> assert_cmd::Command {
    let mut cmd = if binary == "ro" {
        cargo_bin_cmd!("snow-cli-ro")
    } else {
        cargo_bin_cmd!("snow-cli")
    };
    cmd.env("SNOW_CLI_CONFIG", config)
        .env("SNOW_CLI_TEST_KEYCHAIN_STORE", store)
        .env("SNOW_CLI_ALLOW_PLAINTEXT_TEST_KEYCHAIN", "1")
        .env_remove("SNOW_SESSION_COOKIE");
    cmd
}
fn bootstrap() -> String {
    // Mirrors the actual native UI's JS string-escaped JSON.parse payload.
    let payload = json!({"output":{"isLoggedIn":true,"user":{
        "sys_id":USER,"roles":["admin","security_admin"], "elevatedRoles":["security_admin","soap"],
        "isImpersonating":false,"language":"en","timeZone":"Europe/Berlin",
        "cookie":"do-not-output", "preferences":[{"name":"secret","value":"do-not-output"}]
    },"properties":{"sessionTimeout":{"value":"90"}}}})
    .to_string();
    format!(
        "g_ck='csrf-secret'; uxGlobals.session = JSON.parse('{}');",
        payload.replace('\\', "\\\\").replace('"', "\\\"")
    )
}
fn pickers(application: &str) -> Value {
    json!({"result":{"currentDomain":{"value":"global","label":"global"},
        "currentApplication":{"sysId":application,"scopeName":if application == "global" {"global"} else {"x_acme_demo"},"name":"App"},
        "currentUpdateSet":{"sysId":UPDATE,"name":if application == "global" {"Default"} else {"App Default"}}}})
}
async fn base(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/navpage.do"))
        .respond_with(ResponseTemplate::new(200).set_body_string(bootstrap()))
        .mount(server)
        .await;
    Mock::given(method("GET")).and(path("/api/now/ui/user/current_user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result":{"user_sys_id":USER,"user_name":"admin","user_display_name":"Administrator"}})))
        .mount(server).await;
    Mock::given(method("GET"))
        .and(path(format!("/api/now/table/sys_user/{USER}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"result":{"sys_domain":{"value":"global"}}})),
        )
        .mount(server)
        .await;
}

#[tokio::test]
async fn native_inspection_is_redacted_and_distinguishes_available_elevation() {
    let server = MockServer::start().await;
    base(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/now/ui/concoursepicker/current"))
        .and(header("Cookie", "JSESSIONID=initial-secret; route=node1"))
        .and(header("X-UserToken", "csrf-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pickers("global")))
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config, store) = config(&server.uri());
    let output = command("ro", &config, &store)
        .args(["session", "show"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let data: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(data["transport"], "native_cookie");
    assert_eq!(data["active_elevated_roles"], json!(["security_admin"]));
    assert_eq!(
        data["available_elevated_roles"],
        json!(["security_admin", "soap"])
    );
    assert_eq!(data["home_domain"], "global");
    assert!(data["record_domain"].is_null());
    let stdout = String::from_utf8(output).unwrap();
    for secret in ["initial-secret", "csrf-secret", "do-not-output"] {
        assert!(!stdout.contains(secret));
    }
}

#[tokio::test]
async fn switch_verifies_all_contexts_and_resolves_scope_name() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let server = MockServer::start().await;
    base(&server).await;
    let switched = Arc::new(AtomicBool::new(false));
    let read = switched.clone();
    Mock::given(method("GET"))
        .and(path("/api/now/ui/concoursepicker/current"))
        .respond_with(move |_: &wiremock::Request| {
            ResponseTemplate::new(200).set_body_json(pickers(if read.load(Ordering::SeqCst) {
                APP
            } else {
                "global"
            }))
        })
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/ui/concoursepicker/application"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"result":{"list":[{"sysId":APP,"scopeName":"x_acme_demo","name":"App"}]}}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/now/ui/concoursepicker/application"))
        .and(header("Cookie", "JSESSIONID=initial-secret; route=node1"))
        .and(header("X-UserToken", "csrf-secret"))
        .and(body_json(json!({"app_id":APP})))
        .respond_with(move |_: &wiremock::Request| {
            switched.store(true, Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(json!({"result":{}}))
        })
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config, store) = config(&server.uri());
    command("full", &config, &store)
        .args(["session", "switch", "application", "x_acme_demo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"verified\":true"))
        .stdout(predicate::str::contains("App Default"));
}

#[tokio::test]
async fn successful_http_status_does_not_mean_verified_switch() {
    let server = MockServer::start().await;
    base(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/now/ui/concoursepicker/current"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pickers("global")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/ui/concoursepicker/domain"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"result":{"list":[{"value":APP,"label":"TOP"}]}})),
        )
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/now/ui/concoursepicker/domain"))
        .and(body_json(json!({"value":APP})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result":{}})))
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config, store) = config(&server.uri());
    command("full", &config, &store)
        .args(["session", "switch", "domain", APP])
        .assert()
        .failure()
        .stderr(predicate::str::contains("read-back did not match"));
}

#[tokio::test]
async fn expiry_does_not_trigger_basic_login_or_mutation() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/navpage.do"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-is-logged-in", "false")
                .set_body_string("login form"),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let (_dir, config, store) = config(&server.uri());
    command("full", &config, &store)
        .args(["session", "switch", "domain", "global"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("expired"));
}

#[tokio::test]
async fn rotated_cookies_are_persisted_and_reused_by_table_commands() {
    let server = MockServer::start().await;
    base(&server).await;
    Mock::given(method("GET"))
        .and(path("/navpage.do"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", "JSESSIONID=rotated-secret; Path=/; HttpOnly")
                .set_body_string(bootstrap()),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/ui/concoursepicker/current"))
        .and(header("Cookie", "route=node1; JSESSIONID=rotated-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pickers("global")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .and(header("Cookie", "route=node1; JSESSIONID=rotated-secret"))
        .and(header("X-UserToken", "csrf-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result":[]})))
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config, store) = config(&server.uri());
    command("ro", &config, &store)
        .args(["session", "show"])
        .assert()
        .success();
    command("ro", &config, &store)
        .args(["table", "list", "incident"])
        .assert()
        .success();
}

#[tokio::test]
async fn cross_instance_override_is_rejected_before_sending_cookies() {
    let first = MockServer::start().await;
    let second = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&second)
        .await;
    let (_dir, config, store) = config(&first.uri());
    command("full", &config, &store)
        .args(["--instance", &second.uri(), "table", "list", "incident"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("different instance"));
}

#[tokio::test]
async fn read_only_policy_blocks_switching_before_network() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let (_dir, config, store) = config(&server.uri());
    command("full", &config, &store)
        .args(["--read-only", "session", "switch", "domain", "global"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("POLICY_DENIED"));
    command("ro", &config, &store)
        .args(["session", "switch", "domain", "global"])
        .assert()
        .failure();
}

#[tokio::test]
async fn cookie_attachment_creates_dedicated_profile_without_exporting_secrets() {
    let server = MockServer::start().await;
    base(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/now/ui/concoursepicker/current"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pickers("global")))
        .mount(&server)
        .await;
    let (_dir, config, store) = config(&server.uri());
    command("ro", &config, &store)
        .args([
            "--profile",
            "source",
            "session",
            "attach",
            "attached",
            "--cookie-stdin",
        ])
        .write_stdin("JSESSIONID=attached-secret\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("attached-secret").not())
        .stdout(predicate::str::contains("csrf-secret").not());
    let saved = std::fs::read_to_string(&config).unwrap();
    assert!(saved.contains("native_session"));
    assert!(!saved.contains("attached-secret"));
    let credential =
        common::read_test_keychain_entry(&store, "snow-cli", "attached:native_session").unwrap();
    assert!(credential.contains("attached-secret"));
    command("ro", &config, &store)
        .args(["--profile", "attached", "session", "forget"])
        .assert()
        .success();
    assert!(
        common::read_test_keychain_entry(&store, "snow-cli", "attached:native_session").is_none()
    );
}

#[tokio::test]
async fn already_current_update_set_is_verified_without_a_put() {
    let server = MockServer::start().await;
    base(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/now/ui/concoursepicker/current"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pickers("global")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/ui/concoursepicker/updateset"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"result":{"updateSet":[{"sysId":UPDATE,"name":"Default [Global]"}]}}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .respond_with(ResponseTemplate::new(400))
        .expect(0)
        .mount(&server)
        .await;
    let (_dir, config, store) = config(&server.uri());
    command("full", &config, &store)
        .args(["session", "switch", "updateset", UPDATE])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"verified\":true"));
}

#[tokio::test]
async fn update_set_switch_uses_native_body_and_verifies_result() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let server = MockServer::start().await;
    base(&server).await;
    let switched = Arc::new(AtomicBool::new(false));
    let read = switched.clone();
    Mock::given(method("GET"))
        .and(path("/api/now/ui/concoursepicker/current"))
        .respond_with(move |_: &wiremock::Request| {
            let mut context = pickers("global");
            if read.load(Ordering::SeqCst) {
                context["result"]["currentUpdateSet"]["sysId"] = json!(APP);
            }
            ResponseTemplate::new(200).set_body_json(context)
        })
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/ui/concoursepicker/updateset"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"result":{"updateSet":[{"sysId":APP,"name":"Work [Global]"}]}}),
            ),
        )
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/now/ui/concoursepicker/updateset"))
        .and(body_json(json!({"sysId":APP,"name":"Work [Global]"})))
        .respond_with(move |_: &wiremock::Request| {
            switched.store(true, Ordering::SeqCst);
            ResponseTemplate::new(200).set_body_json(json!({"result":{"success":true}}))
        })
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config, store) = config(&server.uri());
    command("full", &config, &store)
        .args(["session", "switch", "updateset", APP])
        .assert()
        .success();
}

#[tokio::test]
async fn login_verifies_cookie_only_identity_and_does_not_store_password() {
    let server = MockServer::start().await;
    base(&server).await;
    Mock::given(method("POST"))
        .and(path("/login.do"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", "/navpage.do")
                .insert_header("Set-Cookie", "JSESSIONID=login-secret; Path=/; HttpOnly"),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/ui/user/current_user"))
        .and(header("Cookie", "JSESSIONID=login-secret"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"result":{"user_sys_id":USER,"user_name":"admin"}})),
        )
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    let (_dir, config, store) = config(&server.uri());
    command("full", &config, &store)
        .args([
            "--profile",
            "source",
            "session",
            "login",
            "loggedin",
            "--password-stdin",
        ])
        .write_stdin("password-secret\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("login-secret").not())
        .stdout(predicate::str::contains("password-secret").not());
    let saved =
        common::read_test_keychain_entry(&store, "snow-cli", "loggedin:native_session").unwrap();
    assert!(saved.contains("login-secret"));
    assert!(!saved.contains("password-secret"));
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .filter(|r| r.method == "GET")
            .all(|r| !r.headers.contains_key("authorization"))
    );
    command("full", &config, &store)
        .args(["--profile", "loggedin", "auth", "token"])
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("not exported"));
}

#[tokio::test]
async fn native_bootstrap_follows_same_origin_shell_redirect_with_rotated_cookies() {
    let server = MockServer::start().await;
    base(&server).await;
    Mock::given(method("GET"))
        .and(path("/navpage.do"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", "/now/nav/ui/home")
                .insert_header("Set-Cookie", "route=node2; Path=/"),
        )
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/now/nav/ui/home"))
        .and(header("Cookie", "JSESSIONID=initial-secret; route=node2"))
        .respond_with(ResponseTemplate::new(200).set_body_string(bootstrap()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/ui/concoursepicker/current"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pickers("global")))
        .mount(&server)
        .await;
    let (_dir, config, store) = config(&server.uri());
    command("ro", &config, &store)
        .args(["session", "show"])
        .assert()
        .success();
}

#[tokio::test]
async fn native_bootstrap_rejects_cross_origin_redirect_before_network() {
    let server = MockServer::start().await;
    let other = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/navpage.do"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", format!("{}/home", other.uri())),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&other)
        .await;
    let (_dir, config, store) = config(&server.uri());
    command("ro", &config, &store)
        .args(["session", "show"])
        .assert()
        .failure();
}
