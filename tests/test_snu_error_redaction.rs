//! Exercise a real broker, simulated helper, and CLI error rendering together.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Duration;

use assert_cmd::cargo::cargo_bin;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

const PRIVATE_ERROR: &str =
    "g_ck=csrf-secret Cookie: JSESSIONID=cookie-secret private customer data\u{1b}[31m";

#[tokio::test]
async fn helper_action_failure_is_redacted_through_broker_and_cli_stderr() {
    exercise_failure("plain").await;
}

#[tokio::test]
async fn transient_helper_failure_still_retries_without_refreshing_session() {
    exercise_failure("transient").await;
}

#[tokio::test]
async fn acl_failure_still_probes_and_preserves_the_valid_session() {
    exercise_failure("acl").await;
}

#[tokio::test]
async fn stale_token_failure_still_refreshes_and_retries() {
    exercise_failure("stale").await;
}

async fn exercise_failure(scenario: &str) {
    let ipc_reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let ws_reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let ipc_addr = ipc_reservation.local_addr().unwrap();
    let ws_addr = ws_reservation.local_addr().unwrap();
    drop((ipc_reservation, ws_reservation));

    // Plain typed action errors remain redacted even with sensitive debugging
    // enabled. ACL diagnostic details follow the existing opt-in HTTP contract.
    let sensitive = if scenario == "plain" { "1" } else { "0" };
    let mut broker = tokio::process::Command::new(cargo_bin!("snow-cli"))
        .args(["snu", "broker", "serve"])
        .env("SNOW_CLI_SNU_BROKER_ADDR", ipc_addr.to_string())
        .env("SNOW_CLI_SNU_WS_ADDR", ws_addr.to_string())
        .env("SNOW_CLI_SNU_BROKER_PERSIST", "0")
        .env("SNOW_CLI_DEBUG_HTTP_INCLUDE_SENSITIVE", sensitive)
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    tokio::time::timeout(Duration::from_secs(15), async {
        let mut helper = loop {
            if let Ok((helper, _)) = tokio_tungstenite::connect_async(format!("ws://{ws_addr}")).await {
                break helper;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let hello = helper.next().await.unwrap().unwrap();
        assert!(hello.to_text().unwrap().contains("hostHello"));
        helper.send(Message::Text(session_push("session-token").to_string().into())).await.unwrap();

        let cli = tokio::process::Command::new(cargo_bin!("snow-cli"))
            .args(["snu", "query", "incident", "--timeout-secs", "5"])
            .env("SNOW_CLI_SNU_BROKER_ADDR", ipc_addr.to_string())
            .env("SNOW_CLI_DEBUG_HTTP_INCLUDE_SENSITIVE", sensitive)
            .kill_on_drop(true)
            .output();
        let helper_reply = async {
            let mut actions = 0;
            let mut probes = 0;
            let mut refreshes = 0;
            loop {
                let frame = helper.next().await.unwrap().unwrap();
                let request: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
                let mut done = false;
                let response = match request["action"].as_str().unwrap() {
                    "bannerMessage" => {
                        if request.to_string().contains("rejected the saved session token") {
                            assert_eq!(scenario, "stale", "unexpected refresh: {scenario}");
                            refreshes += 1;
                            // Model a user pushing /token after the broker's
                            // banner; allow its session waiter to register first.
                            tokio::time::sleep(Duration::from_millis(30)).await;
                            helper.send(Message::Text(session_push("fresh-token").to_string().into())).await.unwrap();
                        }
                        continue;
                    }
                    "agentGetCapabilities" => capabilities_reply(&request),
                    "agentQueryRecords" if request["tableName"] == "sys_user" => {
                        probes += 1;
                        done = scenario == "acl";
                        json!({
                            "action": "agentQueryRecordsResponse",
                            "agentRequestId": request["agentRequestId"],
                            "success": scenario == "acl",
                            "error": if scenario == "stale" { Some(format!("HTTP 401 {PRIVATE_ERROR}")) } else { None }
                        })
                    }
                    "agentQueryRecords" => {
                        actions += 1;
                        let success = actions == 2;
                        let expected_token = if scenario == "stale" && success { "fresh-token" } else { "session-token" };
                        assert_eq!(request["instance"]["g_ck"], expected_token);
                        done = success || scenario == "plain";
                        let prefix = match scenario {
                            "transient" => "Failed to fetch",
                            "acl" => "HTTP 403",
                            "stale" => "HTTP 401",
                            _ => "Action rejected",
                        };
                        json!({
                            "action": "agentQueryRecordsResponse",
                            "agentRequestId": request["agentRequestId"], "success": success,
                            "error": if success { None } else { Some(format!("{prefix} {PRIVATE_ERROR}")) },
                            "records": [], "count": 0
                        })
                    }
                    other => panic!("unexpected helper action: {other}"),
                };
                helper.send(Message::Text(response.to_string().into())).await.unwrap();
                if done { break; }
            }
            assert_eq!(actions, if matches!(scenario, "transient" | "stale") { 2 } else { 1 });
            assert_eq!(probes, usize::from(matches!(scenario, "acl" | "stale")));
            assert_eq!(refreshes, usize::from(scenario == "stale"));
            // Keep the helper socket alive until the CLI consumes the reply.
            helper
        };
        let (output, _helper) = tokio::join!(cli, helper_reply);
        let output = output.unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        if matches!(scenario, "transient" | "stale") {
            assert!(output.status.success(), "{scenario}: {stderr}");
            let _: Value = serde_json::from_slice(&output.stdout).unwrap();
        } else {
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            let error: Value = serde_json::from_str(stderr.trim()).unwrap();
            assert_eq!(error["error"]["code"], if scenario == "acl" { "SNU_BROKER_ERROR" } else { "SNU_BRIDGE_ERROR" });
            assert!(error["error"]["message"].as_str().unwrap().contains("redacted"));
        }
        for secret in ["csrf-secret", "cookie-secret", "private customer data", "\\u001b", "\u{1b}"] {
            assert!(!stderr.contains(secret), "{scenario}: {stderr}");
        }
        let status = tokio::process::Command::new(cargo_bin!("snow-cli"))
            .args(["snu", "broker", "status"])
            .env("SNOW_CLI_SNU_BROKER_ADDR", ipc_addr.to_string())
            .kill_on_drop(true)
            .output().await.unwrap();
        assert!(status.status.success());
        let status: Value = serde_json::from_slice(&status.stdout).unwrap();
        assert_eq!(status["session_count"], 1, "session was evicted: {scenario}");
    }).await.expect("broker/helper/CLI exchange timed out");

    broker.kill().await.unwrap();
    let output = broker.wait_with_output().await.unwrap();
    let logs = String::from_utf8_lossy(&output.stderr);
    assert!(!logs.contains("csrf-secret"), "{logs}");
    assert!(!logs.contains("private customer data"), "{logs}");
}

fn session_push(token: &str) -> Value {
    json!({"instance": {"name": "dev", "url": "https://dev.service-now.com", "g_ck": token}})
}

fn capabilities_reply(request: &Value) -> Value {
    json!({
        "action": "agentGetCapabilitiesResponse",
        "agentRequestId": request["agentRequestId"], "success": true,
        "capabilities": {"protocolVersion": 1, "instanceSecurityGates": 1},
        "instanceGates": {"https://dev.service-now.com": {
            "revision": 1, "gates": {"restRequest": "auto"}
        }}
    })
}
