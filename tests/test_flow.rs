#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Wiremock-backed integration tests for `flow show`.

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FLOW_ID: &str = "2f33fe56731310108ef62d2b04f6a749";
const SNAPSHOT_ID: &str = "9604ba96731310108ef62d2b04f6a7b9";
const FLOW_FIELDS: &str =
    "sys_id,name,internal_name,type,status,active,sys_scope.scope,description,latest_snapshot";

fn api_key_config(instance: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    common::create_temp_config(&format!(
        "default_profile = \"default\"\n[profiles.default]\ninstance = \"{instance}\"\nauth_method = \"api_key\"\n"
    ))
}

fn run(
    binary: &str,
    server: &MockServer,
    config: &std::path::Path,
    args: &[&str],
) -> std::process::Output {
    let mut command = match binary {
        "snow-cli-ro" => cargo_bin_cmd!("snow-cli-ro"),
        _ => cargo_bin_cmd!("snow-cli"),
    };
    command
        .env("SNOW_CLI_CONFIG", config)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .env_remove("SNOW_CLI_OUTPUT")
        .arg("--instance")
        .arg(server.uri())
        .args(args)
        .output()
        .unwrap()
}

fn flow_row(sys_id: &str, name: &str, internal_name: &str) -> serde_json::Value {
    serde_json::json!({
        "sys_id": sys_id,
        "name": name,
        "internal_name": internal_name,
        "type": "flow",
        "status": "published",
        "active": "true",
        "sys_scope.scope": "global",
        "description": "Process a Change Request in Authorize.",
        "latest_snapshot": SNAPSHOT_ID
    })
}

async fn mount(server: &MockServer, table: &str, query: &str, rows: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(format!("/api/now/table/{table}")))
        .and(query_param("sysparm_query", query))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": rows})))
        .expect(1)
        .mount(server)
        .await;
}

fn component(
    flow: &str,
    sys_id: &str,
    ui: &str,
    parent: &str,
    order: &str,
    name_field: &str,
    name: &str,
) -> serde_json::Value {
    serde_json::json!({
        "sys_id": sys_id,
        "flow": {"link": "x", "value": flow},
        "ui_id": ui,
        "parent_ui_id": parent,
        "order": order,
        "comment": "",
        name_field: name
    })
}

async fn mount_flow_lookup(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_hub_flow"))
        .and(query_param(
            "sysparm_query",
            "internal_name=Change - Normal - Authorize^ORname=Change - Normal - Authorize",
        ))
        .and(query_param("sysparm_fields", FLOW_FIELDS))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [flow_row(FLOW_ID, "Change - Normal - Authorize", "change__normal__authorize")]
        })))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn flow_show_builds_the_ordered_tree_with_labels_in_six_requests() {
    let server = MockServer::start().await;
    mount_flow_lookup(&server).await;
    let flow_in = format!("flowIN{FLOW_ID},{SNAPSHOT_ID}");
    mount(
        &server,
        "sys_hub_trigger_instance",
        &flow_in,
        serde_json::json!([
            {"sys_id": "t1", "flow": FLOW_ID, "name": "Created or Updated", "trigger_type": "record_create_or_update"},
            {"sys_id": "t2", "flow": SNAPSHOT_ID, "name": "Created or Updated", "trigger_type": "record_create_or_update"}
        ]),
    )
    .await;
    let logic = "logic_definition.name";
    mount(
        &server,
        "sys_hub_flow_logic",
        &flow_in,
        serde_json::json!([
            component(
                FLOW_ID,
                "l1",
                "par",
                "",
                "1",
                logic,
                "Do the following in Parallel"
            ),
            component(FLOW_ID, "l2", "b1", "par", "2", logic, "Parallel Branch"),
            component(FLOW_ID, "l3", "if1", "b1", "5", logic, "If"),
            component(FLOW_ID, "l4", "b2", "par", "11", logic, "Parallel Branch"),
            // The snapshot's copy of the same step is ignored.
            component(
                SNAPSHOT_ID,
                "s1",
                "par",
                "",
                "1",
                logic,
                "Do the following in Parallel"
            )
        ]),
    )
    .await;
    let action = "action_type.name";
    mount(
        &server,
        "sys_hub_action_instance",
        &flow_in,
        serde_json::json!([
            component(
                FLOW_ID,
                "a1",
                "wait",
                "b2",
                "11➛12",
                action,
                "Wait For Condition"
            ),
            component(
                FLOW_ID,
                "a2",
                "check",
                "b1",
                "2➛3",
                action,
                "Check Change for User Approval"
            ),
            component(FLOW_ID, "a3", "upd", "if1", "6", action, "Update Record"),
            component(
                FLOW_ID,
                "a4",
                "policy",
                "b1",
                "4",
                action,
                "Apply Change Approval Policy"
            )
        ]),
    )
    .await;
    mount(
        &server,
        "sys_hub_sub_flow_instance",
        &flow_in,
        serde_json::json!([]),
    )
    .await;
    mount(
        &server,
        "sys_variable_value",
        "document_keyINt1,l1,l2,l3,l4^variable.elementINcondition_name,table,condition",
        serde_json::json!([
            {"document_key": "l3", "variable.element": "condition_name", "value": "Skipped"},
            {"document_key": "t1", "variable.element": "table", "value": "change_request"},
            {"document_key": "t1", "variable.element": "condition", "value": "state=-3"}
        ]),
    )
    .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run(
        "snow-cli-ro",
        &server,
        &config,
        &["flow", "show", "Change - Normal - Authorize"],
    );
    assert!(output.status.success(), "{output:?}");
    let view: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(view["flow"]["internal_name"], "change__normal__authorize");
    assert_eq!(view["flow"]["scope"], "global");
    assert_eq!(
        view["triggers"],
        serde_json::json!([{
            "name": "Created or Updated",
            "type": "record_create_or_update",
            "table": "change_request",
            "condition": "state=-3"
        }])
    );
    assert_eq!(
        view["structure"],
        serde_json::json!({"mode": "parallel", "top_level": ["Do the following in Parallel"], "branches": 2})
    );
    let steps: Vec<String> = view["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| {
            format!(
                "{} {} {} {}",
                step["path"].as_str().unwrap(),
                step["kind"].as_str().unwrap(),
                step["type"].as_str().unwrap(),
                step["note"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        steps,
        [
            "1 logic Do the following in Parallel ",
            "1.1 logic Parallel Branch ",
            "1.1.1 action Check Change for User Approval ",
            "1.1.2 action Apply Change Approval Policy ",
            "1.1.3 logic If Skipped",
            "1.1.3.1 action Update Record ",
            "1.2 logic Parallel Branch ",
            "1.2.1 action Wait For Condition ",
        ]
    );
    assert_eq!(view["action_types"].as_array().unwrap().len(), 4);
    assert_eq!(view["source"], "flow");
    assert!(view.get("warnings").is_none());
}

#[tokio::test]
async fn flow_show_falls_back_to_v2_tables_and_reports_ambiguity() {
    let server = MockServer::start().await;
    mount(
        &server,
        "sys_hub_flow",
        &format!("sys_id={FLOW_ID}"),
        serde_json::json!([flow_row(FLOW_ID, "Sub", "sub")]),
    )
    .await;
    let flow_in = format!("flowIN{FLOW_ID},{SNAPSHOT_ID}");
    for table in [
        "sys_hub_trigger_instance",
        "sys_hub_flow_logic",
        "sys_hub_action_instance",
        "sys_hub_sub_flow_instance",
        "sys_hub_trigger_instance_v2",
        "sys_hub_flow_logic_instance_v2",
    ] {
        mount(&server, table, &flow_in, serde_json::json!([])).await;
    }
    mount(
        &server,
        "sys_hub_action_instance_v2",
        &flow_in,
        serde_json::json!([component(
            FLOW_ID,
            "a1",
            "ask",
            "",
            "1",
            "action_type.name",
            "Ask For Approval"
        )]),
    )
    .await;
    mount(
        &server,
        "sys_hub_sub_flow_instance_v2",
        &flow_in,
        serde_json::json!([component(
            FLOW_ID,
            "s1",
            "sub",
            "",
            "2",
            "subflow.name",
            "Install Application"
        )]),
    )
    .await;
    mount(
        &server,
        "sys_hub_flow",
        "internal_name=Dup^ORname=Dup",
        serde_json::json!([
            flow_row("11111111111111111111111111111111", "Dup", "dup_a"),
            flow_row("22222222222222222222222222222222", "Dup", "dup_b")
        ]),
    )
    .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run("snow-cli", &server, &config, &["flow", "show", FLOW_ID]);
    assert!(output.status.success(), "{output:?}");
    let view: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(view["source"], "v2");
    assert!(view.get("triggers").is_none());
    assert_eq!(view["steps"][0]["type"], "Ask For Approval");
    assert_eq!(view["steps"][1]["kind"], "subflow");
    assert_eq!(view["steps"][1]["type"], "Install Application");
    assert_eq!(view["structure"]["mode"], "sequential");

    let output = run("snow-cli", &server, &config, &["flow", "show", "Dup"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "VALIDATION_ERROR");
    assert_eq!(error["error"]["issues"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn flow_show_keeps_the_tree_when_labels_fail() {
    let server = MockServer::start().await;
    mount_flow_lookup(&server).await;
    let flow_in = format!("flowIN{FLOW_ID},{SNAPSHOT_ID}");
    mount(
        &server,
        "sys_hub_trigger_instance",
        &flow_in,
        serde_json::json!([]),
    )
    .await;
    mount(
        &server,
        "sys_hub_flow_logic",
        &flow_in,
        serde_json::json!([component(
            FLOW_ID,
            "l1",
            "if1",
            "",
            "1",
            "logic_definition.name",
            "If"
        )]),
    )
    .await;
    for table in ["sys_hub_action_instance", "sys_hub_sub_flow_instance"] {
        mount(&server, table, &flow_in, serde_json::json!([])).await;
    }
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_variable_value"))
        .respond_with(ResponseTemplate::new(500).set_body_string("gateway"))
        .mount(&server)
        .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run(
        "snow-cli",
        &server,
        &config,
        &["flow", "show", "Change - Normal - Authorize"],
    );
    assert!(output.status.success(), "{output:?}");
    let view: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(view["steps"][0]["type"], "If");
    assert_eq!(view["warnings"].as_array().unwrap().len(), 1);
}

#[test]
fn flow_help_is_available_in_both_binaries() {
    for mut command in [cargo_bin_cmd!("snow-cli"), cargo_bin_cmd!("snow-cli-ro")] {
        let output = command.args(["flow", "show", "--help"]).output().unwrap();
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(
            help.contains("flow show 'Change - Normal - Authorize'"),
            "{help}"
        );
    }
}
