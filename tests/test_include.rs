#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Wiremock-backed integration tests for `include show`.

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn api_key_config(instance: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    common::create_temp_config(&format!(
        "default_profile = \"default\"\n[profiles.default]\ninstance = \"{instance}\"\nauth_method = \"api_key\"\n"
    ))
}

const HANDLER_SCRIPT: &str = "var ChangeRequestStateHandler = Class.create();\n// Customize here.\nChangeRequestStateHandler.prototype = Object.extendsObject(ChangeRequestStateHandlerSNC, {\n    initialize: function(gr) {\n        ChangeRequestStateHandlerSNC.prototype.initialize.call(this, gr);\n    },\n    type: 'ChangeRequestStateHandler'\n});";

const SNC_SCRIPT: &str = "var ChangeRequestStateHandlerSNC = Class.create();\nChangeRequestStateHandlerSNC.prototype = {\n    initialize: function(gr) {\n        this._resetModel(gr);\n    },\n    canMoveTo: function(state) {\n        return this._model.canMoveTo(state);\n    },\n    _resetModel: function(gr) {\n        this._model = new Model_normal(gr);\n    },\n    type: 'ChangeRequestStateHandlerSNC'\n};";

fn include_row(name: &str, sys_id: &str, scope: &str, script: &str) -> serde_json::Value {
    serde_json::json!({
        "sys_id": sys_id,
        "name": name,
        "api_name": format!("{scope}.{name}"),
        "active": "true",
        "access": "public",
        "client_callable": "false",
        "sys_scope": {"link": "x", "value": "abc"},
        "sys_scope.scope": scope,
        "script": script
    })
}

async fn mount_include(server: &MockServer, query: &str, rows: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_script_include"))
        .and(query_param("sysparm_query", query))
        .and(query_param(
            "sysparm_fields",
            "sys_id,name,api_name,active,access,client_callable,sys_scope,sys_scope.scope,script",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": rows})))
        .mount(server)
        .await;
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

#[tokio::test]
async fn include_show_chain_follows_parsed_extends_and_greps_each_link() {
    let server = MockServer::start().await;
    mount_include(
        &server,
        "name=ChangeRequestStateHandler",
        serde_json::json!([include_row(
            "ChangeRequestStateHandler",
            "11111111111111111111111111111111",
            "global",
            HANDLER_SCRIPT
        )]),
    )
    .await;
    // The parent exists in two scopes; the child's scope wins.
    mount_include(
        &server,
        "name=ChangeRequestStateHandlerSNC",
        serde_json::json!([
            include_row(
                "ChangeRequestStateHandlerSNC",
                "33333333333333333333333333333333",
                "x_other",
                "var X = Class.create();"
            ),
            include_row(
                "ChangeRequestStateHandlerSNC",
                "22222222222222222222222222222222",
                "global",
                SNC_SCRIPT
            ),
        ]),
    )
    .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run(
        "snow-cli-ro",
        &server,
        &config,
        &[
            "include",
            "show",
            "ChangeRequestStateHandler",
            "--chain",
            "--grep",
            "canMoveTo",
            "--context",
            "1",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let view: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let chain = view["includes"].as_array().unwrap();
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0]["extends"], "ChangeRequestStateHandlerSNC");
    assert_eq!(chain[0]["extends_source"], "parsed");
    assert_eq!(
        chain[0]["outline"],
        serde_json::json!(["4-6 initialize(gr)"])
    );
    assert_eq!(chain[0]["grep"]["match_count"], 0);
    assert_eq!(chain[1]["sys_id"], "22222222222222222222222222222222");
    assert!(chain[1]["extends"].is_null());
    assert_eq!(
        chain[1]["outline"],
        serde_json::json!([
            "3-5 initialize(gr)",
            "6-8 canMoveTo(state)",
            "9-11 _resetModel(gr)"
        ])
    );
    assert_eq!(view["references"], serde_json::json!(["Model_normal"]));
    assert_eq!(
        chain[1]["grep"]["lines"],
        serde_json::json!([
            "5-    },",
            "6:    canMoveTo: function(state) {",
            "7:        return this._model.canMoveTo(state);",
            "8-    },"
        ])
    );
    assert!(chain[1].get("script").is_none());
}

#[tokio::test]
async fn include_show_single_object_with_script_and_missing_parent_marker() {
    let server = MockServer::start().await;
    mount_include(
        &server,
        "api_name=global.ChangeRequestStateHandler",
        serde_json::json!([include_row(
            "ChangeRequestStateHandler",
            "11111111111111111111111111111111",
            "global",
            HANDLER_SCRIPT
        )]),
    )
    .await;
    mount_include(
        &server,
        "name=ChangeRequestStateHandlerSNC",
        serde_json::json!([]),
    )
    .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run(
        "snow-cli",
        &server,
        &config,
        &["include", "show", "global.ChangeRequestStateHandler"],
    );
    assert!(output.status.success(), "{output:?}");
    let view: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(view.is_object());
    assert_eq!(view["script"], HANDLER_SCRIPT);
    assert_eq!(view["scope"], "global");
    assert_eq!(view["active"], true);

    let output = run(
        "snow-cli",
        &server,
        &config,
        &[
            "include",
            "show",
            "global.ChangeRequestStateHandler",
            "--chain",
            "--no-script",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let view: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let chain = view["includes"].as_array().unwrap();
    assert_eq!(chain.len(), 1);
    assert_eq!(chain[0]["extends_found"], false);
    assert!(chain[0].get("script").is_none());
}

const MODEL_SCRIPT: &str = "var Model_normal = Class.create();\nModel_normal.prototype = Object.extendsObject(ChangeRequestStateHandlerSNC, {\n    assess: {\n        nextState: [ \"authorize\" ],\n        authorize: {\n            canMove: function() { return true; }\n        }\n    },\n    type: 'Model_normal'\n});";

#[tokio::test]
async fn include_show_one_call_for_several_chains_with_source_of() {
    let server = MockServer::start().await;
    mount_include(
        &server,
        "name=ChangeRequestStateHandler",
        serde_json::json!([include_row(
            "ChangeRequestStateHandler",
            "11111111111111111111111111111111",
            "global",
            HANDLER_SCRIPT
        )]),
    )
    .await;
    // Expected exactly once: the second chain stops at the listed parent.
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_script_include"))
        .and(query_param(
            "sysparm_query",
            "name=ChangeRequestStateHandlerSNC",
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": [
                include_row(
                    "ChangeRequestStateHandlerSNC",
                    "22222222222222222222222222222222",
                    "global",
                    SNC_SCRIPT
                )
            ]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    mount_include(
        &server,
        "name=Model_normal",
        serde_json::json!([include_row(
            "Model_normal",
            "44444444444444444444444444444444",
            "global",
            MODEL_SCRIPT
        )]),
    )
    .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run(
        "snow-cli-ro",
        &server,
        &config,
        &[
            "include",
            "show",
            "ChangeRequestStateHandler",
            "Model_normal",
            "--chain",
            "--source-of",
            "canMoveTo,assess,nope",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let view: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let includes = view["includes"].as_array().unwrap();
    let names: Vec<&str> = includes
        .iter()
        .map(|include| include["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "ChangeRequestStateHandler",
            "ChangeRequestStateHandlerSNC",
            "Model_normal"
        ]
    );
    assert_eq!(
        includes[2]["outline"],
        serde_json::json!(["3-8 assess{nextState=[authorize],authorize}"])
    );
    assert_eq!(includes[1]["source_of"][0]["member"], "canMoveTo");
    assert_eq!(includes[1]["source_of"][0]["lines"], "6-8");
    assert_eq!(
        includes[1]["source_of"][0]["source"],
        "    canMoveTo: function(state) {\n        return this._model.canMoveTo(state);\n    },"
    );
    assert_eq!(includes[2]["source_of"][0]["lines"], "3-8");
    assert!(
        includes
            .iter()
            .all(|include| include.get("script").is_none())
    );
    assert_eq!(view["members_not_found"], serde_json::json!(["nope"]));
    // Model_normal is listed, so it is no longer a reference to fetch.
    assert_eq!(view["references"], serde_json::json!([]));
    assert!(view.get("truncated").is_none());
}

#[tokio::test]
async fn include_show_not_found_and_ambiguous_names() {
    let server = MockServer::start().await;
    mount_include(&server, "name=Nope", serde_json::json!([])).await;
    mount_include(
        &server,
        "name=Dup",
        serde_json::json!([
            include_row("Dup", "11111111111111111111111111111111", "global", ""),
            include_row("Dup", "22222222222222222222222222222222", "x_app", "")
        ]),
    )
    .await;
    let (_dir, config) = api_key_config(&server.uri());

    let output = run("snow-cli", &server, &config, &["include", "show", "Nope"]);
    assert_eq!(output.status.code(), Some(4), "{output:?}");
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "RECORD_NOT_FOUND");

    let output = run("snow-cli", &server, &config, &["include", "show", "Dup"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "VALIDATION_ERROR");
    assert_eq!(error["error"]["issues"].as_array().unwrap().len(), 2);
}

#[test]
fn include_help_is_available_in_both_binaries() {
    for mut command in [cargo_bin_cmd!("snow-cli"), cargo_bin_cmd!("snow-cli-ro")] {
        let output = command
            .args(["include", "show", "--help"])
            .output()
            .unwrap();
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("--chain"), "{help}");
        assert!(help.contains("--source-of"), "{help}");
        let examples = help.split("Examples:").nth(1).unwrap();
        assert!(
            examples
                .lines()
                .nth(1)
                .unwrap()
                .contains("include show ChangeRequestStateHandler --chain --source-of"),
            "{help}"
        );
        assert!(help.contains("--grep"), "{help}");
    }
}
