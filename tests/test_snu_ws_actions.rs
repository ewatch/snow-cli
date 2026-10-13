#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::Output;
use std::sync::mpsc;
use std::thread;

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::{Value, json};

fn session_instance() -> Value {
    json!({
        "name": "dev",
        "url": "https://dev.service-now.com",
        "g_ck": "secret-token",
        "scope": "global"
    })
}

fn fake_broker_response(request: &Value) -> Value {
    match request["kind"].as_str().unwrap() {
        "ping" | "send_banner" => json!({ "ok": true }),
        "wait_session" => json!({
            "ok": true,
            "instance": session_instance()
        }),
        "send_action" => {
            let payload = &request["payload"];
            let action = payload["action"].as_str().unwrap();
            let request_id = payload["agentRequestId"].as_str().unwrap();
            let message = match action {
                "agentQueryRecords" => json!({
                    "action": "agentQueryRecordsResponse",
                    "agentRequestId": request_id,
                    "success": true,
                    "tableName": payload["tableName"],
                    "count": 1,
                    "records": [
                        {"sys_id": "abc123", "number": "INC0010001"}
                    ]
                }),
                "requestTableStructure" => json!({
                    "action": "requestTableStructureResponse",
                    "agentRequestId": request_id,
                    "success": true,
                    "columns": ["sys_id", "number"]
                }),
                "takeScreenshot" => json!({
                    "action": "takeScreenshotResponse",
                    "agentRequestId": request_id,
                    "success": true,
                    "imageData": "iVBORw0KGgo=",
                    "fileName": "helper.png",
                    "url": "https://dev.service-now.com/nav_to.do",
                    "tabTitle": "ServiceNow"
                }),
                other => json!({
                    "action": format!("{other}Response"),
                    "agentRequestId": request_id,
                    "success": true
                }),
            };
            json!({ "ok": true, "message": message })
        }
        other => panic!("unexpected broker request kind: {other}"),
    }
}

fn run_snu_with_fake_broker(args: &[&str]) -> (Output, Vec<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            let response = fake_broker_response(&request);
            let is_send_action = request["kind"] == "send_action";
            tx.send(request).unwrap();
            writeln!(stream, "{response}").unwrap();
            if is_send_action {
                break;
            }
        }
    });

    let output = cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_SNU_BROKER_ADDR", addr.to_string())
        .args(args)
        .output()
        .unwrap();
    server.join().unwrap();
    let requests = rx.try_iter().collect();
    (output, requests)
}

fn successful_json(output: Output) -> Value {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn send_action_payload(requests: &[Value]) -> Value {
    requests
        .iter()
        .find(|request| request["kind"] == "send_action")
        .unwrap()["payload"]
        .clone()
}

fn assert_request_id_prefix(payload: &Value, prefix: &str) {
    let request_id = payload["agentRequestId"].as_str().unwrap();
    assert!(
        request_id.starts_with(&format!("snow_{prefix}_")),
        "agentRequestId {request_id:?} did not preserve prefix {prefix:?}"
    );
}

fn payload_without_request_id(mut payload: Value) -> Value {
    payload.as_object_mut().unwrap().remove("agentRequestId");
    payload
}

#[test]
fn snu_query_preserves_wire_payload_and_normalized_output() {
    let (output, requests) = run_snu_with_fake_broker(&[
        "snu",
        "query",
        "incident",
        "--query",
        "active=true",
        "--fields",
        "sys_id,number",
        "--limit",
        "3",
        "--order-by",
        "ORDERBYnumber",
        "--timeout-secs",
        "9",
    ]);
    let stdout = successful_json(output);
    assert_eq!(
        stdout,
        json!({
            "table": "incident",
            "count": 1,
            "records": [
                {"sys_id": "abc123", "number": "INC0010001"}
            ]
        })
    );

    let payload = send_action_payload(&requests);
    assert_request_id_prefix(&payload, "query");
    assert_eq!(
        payload_without_request_id(payload),
        json!({
            "action": "agentQueryRecords",
            "tableName": "incident",
            "queryString": "sysparm_fields=sys_id%2Cnumber&sysparm_limit=3&sysparm_query=active%3Dtrue%5EORDERBYnumber",
            "instance": session_instance(),
            "appName": "snow-cli"
        })
    );
}

#[test]
fn snu_schema_preserves_wire_payload_and_redacts_request_id_from_output() {
    let (output, requests) =
        run_snu_with_fake_broker(&["snu", "schema", "cmdb_ci", "--timeout-secs", "9"]);
    let stdout = successful_json(output);
    assert_eq!(stdout["action"], "requestTableStructureResponse");
    assert_eq!(stdout["success"], true);
    assert_eq!(stdout["columns"], json!(["sys_id", "number"]));
    assert!(stdout.get("agentRequestId").is_none());

    let payload = send_action_payload(&requests);
    assert_request_id_prefix(&payload, "schema");
    assert_eq!(
        payload_without_request_id(payload),
        json!({
            "action": "requestTableStructure",
            "tableName": "cmdb_ci",
            "instance": session_instance(),
            "appName": "snow-cli"
        })
    );
}

#[test]
fn snu_no_token_actions_preserve_banners_payloads_and_outputs() {
    let (slash_output, slash_requests) = run_snu_with_fake_broker(&[
        "snu",
        "slash",
        "/token",
        "--url",
        "https://dev.service-now.com/*",
        "--no-auto-run",
        "--timeout-secs",
        "9",
    ]);
    let slash_stdout = successful_json(slash_output);
    assert_eq!(slash_stdout["action"], "runSlashCommandResponse");
    assert!(slash_stdout.get("agentRequestId").is_none());
    assert_eq!(
        slash_requests
            .iter()
            .find(|request| request["kind"] == "send_banner")
            .unwrap()["message"],
        "snow-cli SN-Utils bridge connected. This command does not require /token."
    );
    let slash_payload = send_action_payload(&slash_requests);
    assert_request_id_prefix(&slash_payload, "slash");
    assert_eq!(
        payload_without_request_id(slash_payload),
        json!({
            "action": "runSlashCommand",
            "command": "/token",
            "url": "https://dev.service-now.com/*",
            "tabId": null,
            "autoRun": false,
            "appName": "snow-cli"
        })
    );

    let (tab_output, tab_requests) = run_snu_with_fake_broker(&[
        "snu",
        "tab",
        "activate",
        "https://dev.service-now.com/nav_to.do",
        "--reload",
        "--wait-for-load",
        "--open-if-not-found",
        "--timeout-secs",
        "9",
    ]);
    let tab_stdout = successful_json(tab_output);
    assert_eq!(tab_stdout["action"], "activateTabResponse");
    assert!(tab_stdout.get("agentRequestId").is_none());
    assert_eq!(
        tab_requests
            .iter()
            .find(|request| request["kind"] == "send_banner")
            .unwrap()["message"],
        "snow-cli SN-Utils bridge connected. This command does not require /token."
    );
    let tab_payload = send_action_payload(&tab_requests);
    assert_request_id_prefix(&tab_payload, "tab");
    assert_eq!(
        payload_without_request_id(tab_payload),
        json!({
            "action": "activateTab",
            "url": "https://dev.service-now.com/nav_to.do",
            "reload": true,
            "waitForLoad": true,
            "openIfNotFound": true,
            "appName": "snow-cli"
        })
    );
}

#[test]
fn snu_context_screenshot_and_attachment_preserve_wire_payloads_and_outputs() {
    let (context_get_output, context_get_requests) =
        run_snu_with_fake_broker(&["snu", "context", "get", "--timeout-secs", "9"]);
    let context_get_stdout = successful_json(context_get_output);
    assert_eq!(context_get_stdout["action"], "agentGetContextResponse");
    assert!(context_get_stdout.get("agentRequestId").is_none());
    let context_get_payload = send_action_payload(&context_get_requests);
    assert_request_id_prefix(&context_get_payload, "context_get");
    assert_eq!(
        payload_without_request_id(context_get_payload),
        json!({
            "action": "agentGetContext",
            "instance": session_instance(),
            "appName": "snow-cli"
        })
    );

    let (context_output, context_requests) = run_snu_with_fake_broker(&[
        "snu",
        "context",
        "switch",
        "application",
        "app123",
        "--no-reload-tab",
        "--tab-url",
        "https://dev.service-now.com/*",
        "--timeout-secs",
        "9",
    ]);
    let context_stdout = successful_json(context_output);
    assert_eq!(context_stdout["action"], "switchContextResponse");
    assert!(context_stdout.get("agentRequestId").is_none());
    let context_payload = send_action_payload(&context_requests);
    assert_request_id_prefix(&context_payload, "context");
    assert_eq!(
        payload_without_request_id(context_payload),
        json!({
            "action": "switchContext",
            "switchType": "application",
            "value": "app123",
            "reloadTab": false,
            "tabUrl": "https://dev.service-now.com/*",
            "instance": session_instance(),
            "appName": "snow-cli"
        })
    );

    let temp_dir = tempfile::tempdir().unwrap();
    let screenshot_path = temp_dir.path().join("screen.png");
    let screenshot_path_arg = screenshot_path.to_string_lossy().to_string();
    let (screenshot_output, screenshot_requests) = run_snu_with_fake_broker(&[
        "snu",
        "screenshot",
        "--tab-id",
        "42",
        "--out",
        &screenshot_path_arg,
        "--timeout-secs",
        "9",
    ]);
    let screenshot_stdout = successful_json(screenshot_output);
    assert_eq!(screenshot_stdout["saved"], true);
    assert_eq!(screenshot_stdout["file"], screenshot_path_arg);
    assert_eq!(
        std::fs::read(&screenshot_path).unwrap(),
        b"\x89PNG\r\n\x1a\n"
    );
    let screenshot_payload = send_action_payload(&screenshot_requests);
    assert_request_id_prefix(&screenshot_payload, "screenshot");
    assert_eq!(
        payload_without_request_id(screenshot_payload),
        json!({
            "action": "takeScreenshot",
            "url": null,
            "tabId": 42,
            "instance": session_instance(),
            "appName": "snow-cli"
        })
    );

    let attachment_path = temp_dir.path().join("note.txt");
    std::fs::write(&attachment_path, b"hello").unwrap();
    let attachment_path_arg = attachment_path.to_string_lossy().to_string();
    let (attachment_output, attachment_requests) = run_snu_with_fake_broker(&[
        "snu",
        "attachment-upload",
        "incident",
        "abc123",
        "--file",
        &attachment_path_arg,
        "--content-type",
        "text/custom",
        "--timeout-secs",
        "9",
    ]);
    let attachment_stdout = successful_json(attachment_output);
    assert_eq!(attachment_stdout["action"], "uploadAttachmentResponse");
    assert!(attachment_stdout.get("agentRequestId").is_none());
    let attachment_payload = send_action_payload(&attachment_requests);
    assert_request_id_prefix(&attachment_payload, "attachment");
    assert_eq!(
        payload_without_request_id(attachment_payload),
        json!({
            "action": "uploadAttachment",
            "tableName": "incident",
            "recordSysId": "abc123",
            "fileName": "note.txt",
            "imageData": "aGVsbG8=",
            "contentType": "text/custom",
            "instance": session_instance(),
            "appName": "snow-cli"
        })
    );
}
