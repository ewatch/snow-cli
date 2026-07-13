#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use wiremock::matchers::{body_string_contains, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn api_key_config() -> (tempfile::TempDir, std::path::PathBuf) {
    common::create_temp_config(
        r#"
default_profile = "default"

[profiles.default]
instance = "https://placeholder.service-now.com"
auth_method = "api_key"
"#,
    )
}

async fn mount_scope_inventory_mocks(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_scope"))
        .and(query_param(
            "sysparm_query",
            "scope=x_my_app^ORsys_id=x_my_app",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "scope123", "scope": "x_my_app", "name": "My App", "version": "1.2.3"}
            ]
        })))
        .expect(1)
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_db_object"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"sys_id": "sys_db_object-1", "name": "x_my_app_table", "sys_scope": "scope123"}]
        })))
        .expect(1)
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_dictionary"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [{"sys_id": "dict-1", "element": "u_flag"}]
        })))
        .expect(1)
        .mount(server)
        .await;

    for table in [
        "sys_script_include",
        "sys_script",
        "sys_ui_action",
        "sys_ui_page",
        "sys_ui_policy",
        "sys_properties",
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/api/now/table/{table}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "result": [{"sys_id": format!("{table}-1"), "name": format!("{table}_name"), "sys_scope": "scope123"}]
            })))
            .expect(1)
            .mount(server)
            .await;
    }
}

async fn mount_scope_list_search_mocks(server: &MockServer, search: &str) {
    let scope_query =
        format!("scope={search}^ORsys_id={search}^ORscopeLIKE{search}^ORnameLIKE{search}");
    let plugin_query = format!("id={search}^ORidLIKE{search}^ORnameLIKE{search}");

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_scope"))
        .and(query_param("sysparm_query", &scope_query))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "scope-global", "scope": "global", "name": "Global", "version": ""},
                {"sys_id": "scope-platform", "scope": "sn_ot_incident_mgmt", "name": "OT Incident Management", "version": "1.0.0"},
                {"sys_id": "scope-custom", "scope": "x_acme_incident_tools", "name": "Acme Incident Tools", "version": "2.3.0"}
            ]
        })))
        .expect(1)
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_store_app"))
        .and(query_param("sysparm_query", &scope_query))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "store-1", "scope": "sn_ot_incident_mgmt", "name": "OT Incident Management", "version": "1.0.0"}
            ]
        })))
        .expect(1)
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/v_plugin"))
        .and(query_param("sysparm_query", &plugin_query))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": [
                {"sys_id": "plugin-1", "id": "com.snc.incident", "name": "Incident Management Plugin", "active": "true"}
            ]
        })))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn test_scope_inspect_basic_json() {
    let server = MockServer::start().await;
    mount_scope_inventory_mocks(&server).await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "scope", "inspect", "x_my_app"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"scope\":\"x_my_app\""))
        .stdout(predicate::str::contains("\"artifact_counts\""))
        .stdout(predicate::str::contains("\"total_artifacts\":8"));
}

#[tokio::test]
async fn test_scope_list_classifies_results_for_partial_search() {
    let server = MockServer::start().await;
    mount_scope_list_search_mocks(&server, "incident").await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "scope", "list", "incident"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"search\":\"incident\""))
        .stdout(predicate::str::contains("\"kind\":\"store_app\""))
        .stdout(predicate::str::contains("\"kind\":\"custom_app\""))
        .stdout(predicate::str::contains("\"kind\":\"plugin\""))
        .stdout(predicate::str::contains(
            "\"scope\":\"sn_ot_incident_mgmt\"",
        ));
}

#[tokio::test]
async fn test_scope_list_matches_exact_global_scope() {
    let server = MockServer::start().await;
    mount_scope_list_search_mocks(&server, "global").await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args(["--instance", &server.uri(), "scope", "list", "global"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"scope\":\"global\""))
        .stdout(predicate::str::contains("\"kind\":\"platform\""));
}

#[tokio::test]
async fn test_scope_list_filters_by_kind() {
    let server = MockServer::start().await;
    mount_scope_list_search_mocks(&server, "incident").await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "scope",
            "list",
            "incident",
            "--kind",
            "plugin",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"kind\":\"plugin\""))
        .stdout(predicate::str::contains("com.snc.incident"))
        .stdout(predicate::str::contains("\"kind\":\"store_app\"").not())
        .stdout(predicate::str::contains("\"kind\":\"custom_app\"").not());
}

#[tokio::test]
async fn test_scope_list_text_output_groups_rows() {
    let server = MockServer::start().await;
    mount_scope_list_search_mocks(&server, "incident").await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--output",
            "text",
            "--instance",
            &server.uri(),
            "scope",
            "list",
            "incident",
            "--kind",
            "store-app",
            "--kind",
            "plugin",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Search: incident"))
        .stdout(predicate::str::contains("Kinds: store_app, plugin"))
        .stdout(predicate::str::contains("STORE_APP"))
        .stdout(predicate::str::contains("PLUGIN"))
        .stdout(predicate::str::contains("OT Incident Management"))
        .stdout(predicate::str::contains("com.snc.incident"));
}

#[tokio::test]
async fn test_scope_list_text_output_includes_optional_columns() {
    let server = MockServer::start().await;
    mount_scope_list_search_mocks(&server, "incident").await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--output",
            "text",
            "--instance",
            &server.uri(),
            "scope",
            "list",
            "incident",
            "--show-source-table",
            "--show-sys-id",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("sys_scope"))
        .stdout(predicate::str::contains("v_plugin"))
        .stdout(predicate::str::contains("scope-platform"))
        .stdout(predicate::str::contains("plugin-1"));
}

#[tokio::test]
async fn test_scope_inventory_json() {
    let server = MockServer::start().await;
    mount_scope_inventory_mocks(&server).await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "scope",
            "inventory",
            "x_my_app",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"rows\""))
        .stdout(predicate::str::contains("\"artifact_type\":\"tables\""))
        .stdout(predicate::str::contains(
            "\"source_table\":\"sys_db_object\"",
        ));
}

#[tokio::test]
async fn test_scope_inventory_csv() {
    let server = MockServer::start().await;
    mount_scope_inventory_mocks(&server).await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--output",
            "csv",
            "--instance",
            &server.uri(),
            "scope",
            "inventory",
            "x_my_app",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "scope,scope_sys_id,category,artifact_type,source_table,sys_id,name",
        ))
        .stdout(predicate::str::contains(
            "data_model_logic,tables,sys_db_object,sys_db_object-1,x_my_app_table",
        ));
}

#[tokio::test]
async fn test_scope_inspect_unknown_scope_fails() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/sys_scope"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "result": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "scope",
            "inspect",
            "does_not_exist",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Scope 'does_not_exist' was not found",
        ));
}

#[tokio::test]
async fn test_scope_move_file_dry_run_returns_preview() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/sys.scripts.modern.do"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header(
                    "Set-Cookie",
                    "JSESSIONID=move-file-session; Path=/; HttpOnly",
                )
                .set_body_string("<script>window.g_ck = 'move-file-gck';</script>"),
        )
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/sys.scripts.do"))
        .and(header("Cookie", "JSESSIONID=move-file-session"))
        .and(header("X-UserToken", "move-file-gck"))
        .and(body_string_contains("sys_script_include"))
        .and(body_string_contains("6816f79cc0a8016401c5a33be04be441"))
        .and(body_string_contains("x_target_app"))
        .and(body_string_contains("%22dryRun%22%3Atrue"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<HTML><BODY>{"ok":true,"dry_run":true,"table":"sys_script_include","sys_id":"6816f79cc0a8016401c5a33be04be441","changed_fields":["sys_scope","sys_package","sys_name","sys_update_name","api_name","path"],"warnings":["Field script contains source scope identifiers and was not rewritten automatically."],"requires_confirmation":true,"before":{"sys_scope":"scope-old","sys_package":"scope-old","sys_name":"x_source_app_demo","sys_update_name":"x_source_app_demo","api_name":"x_source_app.Demo","path":"/x_source_app/demo"},"after":{"sys_scope":"scope-new","sys_package":"scope-new","sys_name":"x_target_app_demo","sys_update_name":"x_target_app_demo","api_name":"x_target_app.Demo","path":"/x_target_app/demo"},"source_scope":{"sys_id":"scope-old","scope":"x_source_app","name":"Source App","version":"1.0.0"},"target_scope":{"sys_id":"scope-new","scope":"x_target_app","name":"Target App","version":"1.0.0"}}</BODY></HTML>"#,
        ))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "scope",
            "move-file",
            "sys_script_include",
            "6816f79cc0a8016401c5a33be04be441",
            "--target-scope",
            "x_target_app",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"dry_run\":true"))
        .stdout(predicate::str::contains("x_target_app_demo"))
        .stdout(predicate::str::contains("requires_confirmation"));
}

#[tokio::test]
async fn test_scope_move_file_dry_run_parses_real_script_wrapper() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/sys.scripts.modern.do"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", "JSESSIONID=wrapped-session; Path=/; HttpOnly")
                .set_body_string("<script>window.g_ck = 'wrapped-gck';</script>"),
        )
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/sys.scripts.do"))
        .and(header("Cookie", "JSESSIONID=wrapped-session"))
        .and(header("X-UserToken", "wrapped-gck"))
        .and(body_string_contains("sys_script_include"))
        .and(body_string_contains("6816f79cc0a8016401c5a33be04be441"))
        .and(body_string_contains("x_target_app"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<HTML><BODY>[0:00:00.013] Script completed in scope global: script
*** Script: {"ok":true,"dry_run":true,"table":"sys_script_include","sys_id":"6816f79cc0a8016401c5a33be04be441","changed_fields":["sys_scope"],"warnings":[],"requires_confirmation":false,"before":{"sys_scope":"scope-old"},"after":{"sys_scope":"scope-new"},"source_scope":{"sys_id":"scope-old","scope":"x_source_app","name":"Source App","version":"1.0.0"},"target_scope":{"sys_id":"scope-new","scope":"x_target_app","name":"Target App","version":"1.0.0"}}
</BODY></HTML>"#,
        ))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "scope",
            "move-file",
            "sys_script_include",
            "6816f79cc0a8016401c5a33be04be441",
            "--target-scope",
            "x_target_app",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"dry_run\":true"))
        .stdout(predicate::str::contains(
            "\"changed_fields\":[\"sys_scope\"]",
        ))
        .stdout(predicate::str::contains("\"requires_confirmation\":false"));
}

#[tokio::test]
async fn test_scope_move_file_requires_yes_for_risky_records() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/sys.scripts.modern.do"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", "JSESSIONID=warn-session; Path=/; HttpOnly")
                .set_body_string("<script>window.g_ck = 'warn-gck';</script>"),
        )
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/sys.scripts.do"))
        .and(header("Cookie", "JSESSIONID=warn-session"))
        .and(header("X-UserToken", "warn-gck"))
        .and(body_string_contains("sys_script_include"))
        .and(body_string_contains("6816f79cc0a8016401c5a33be04be441"))
        .and(body_string_contains("x_target_app"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<HTML><BODY>{"ok":false,"dry_run":false,"table":"sys_script_include","sys_id":"6816f79cc0a8016401c5a33be04be441","warnings":["Field script contains source scope identifiers and was not rewritten automatically."],"requires_confirmation":true,"error":"Risky record contains additional scope-coupled values. Re-run with --yes after reviewing warnings."}</BODY></HTML>"#,
        ))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "scope",
            "move-file",
            "sys_script_include",
            "6816f79cc0a8016401c5a33be04be441",
            "--target-scope",
            "x_target_app",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Risky record contains additional scope-coupled values",
        ))
        .stderr(predicate::str::contains(
            "Field script contains source scope identifiers",
        ));
}

// --- Move-file scope transition E2E tests ---

/// Mount minimal script-runner mocks for move-file tests.
/// Returns a mock that accepts any script body and responds with the supplied JSON.
async fn mount_move_file_mocks(
    server: &MockServer,
    session_id: &str,
    gck: &str,
    result_json: &str,
) {
    Mock::given(method("GET"))
        .and(path("/sys.scripts.modern.do"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header(
                    "Set-Cookie",
                    format!("JSESSIONID={session_id}; Path=/; HttpOnly"),
                )
                .set_body_string(format!("<script>window.g_ck = '{gck}';</script>")),
        )
        .expect(1)
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/sys.scripts.do"))
        .and(header("Cookie", format!("JSESSIONID={session_id}")))
        .and(header("X-UserToken", gck))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(format!("<HTML><BODY>{result_json}</BODY></HTML>")),
        )
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn test_scope_move_file_custom_scope_to_custom_scope_dry_run() {
    // Named (x_*) scope → named (x_*) scope: the established baseline scenario.
    let server = MockServer::start().await;

    let result_json = serde_json::json!({
        "ok": true,
        "dry_run": true,
        "table": "sys_script_include",
        "sys_id": "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6",
        "changed_fields": ["sys_scope", "sys_package", "sys_name", "api_name"],
        "warnings": [],
        "requires_confirmation": false,
        "before": {
            "sys_scope": "scope-src",
            "sys_package": "scope-src",
            "sys_name": "x_source_MyScript",
            "api_name": "x_source.MyScript"
        },
        "after": {
            "sys_scope": "scope-tgt",
            "sys_package": "scope-tgt",
            "sys_name": "x_target_MyScript",
            "api_name": "x_target.MyScript"
        },
        "source_scope": {
            "sys_id": "scope-src",
            "scope": "x_source",
            "name": "Source App",
            "version": "1.0.0"
        },
        "target_scope": {
            "sys_id": "scope-tgt",
            "scope": "x_target",
            "name": "Target App",
            "version": "2.0.0"
        }
    })
    .to_string();

    mount_move_file_mocks(&server, "move-c2c-session", "move-c2c-gck", &result_json).await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
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
        .success()
        .stdout(predicate::str::contains("\"dry_run\":true"))
        .stdout(predicate::str::contains("\"ok\":true"))
        .stdout(predicate::str::contains("x_target_MyScript"))
        .stdout(predicate::str::contains("\"requires_confirmation\":false"));
}

#[tokio::test]
async fn test_scope_move_file_global_to_named_scope_dry_run() {
    // Global scope → named (x_*) scope: file in global scope being scoped into a custom app.
    // The script should allow global as source (x_* restriction removed).
    let server = MockServer::start().await;

    // When source is global, names don't carry a scope prefix, so no renaming happens.
    let result_json = serde_json::json!({
        "ok": true,
        "dry_run": true,
        "table": "sys_script_include",
        "sys_id": "0102030405060708090a0b0c0d0e0f10",
        "changed_fields": ["sys_scope", "sys_package"],
        "warnings": [
            "Field script contains source scope identifiers and was not rewritten automatically."
        ],
        "requires_confirmation": true,
        "before": {
            "sys_scope": "global-scope-id",
            "sys_package": "global-scope-id",
            "sys_name": "GlobalHelper",
            "api_name": ""
        },
        "after": {
            "sys_scope": "scope-custom",
            "sys_package": "scope-custom",
            "sys_name": "GlobalHelper",
            "api_name": ""
        },
        "source_scope": {
            "sys_id": "global-scope-id",
            "scope": "global",
            "name": "Global",
            "version": ""
        },
        "target_scope": {
            "sys_id": "scope-custom",
            "scope": "x_myapp",
            "name": "My App",
            "version": "1.0.0"
        }
    })
    .to_string();

    mount_move_file_mocks(&server, "move-g2n-session", "move-g2n-gck", &result_json).await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "scope",
            "move-file",
            "sys_script_include",
            "0102030405060708090a0b0c0d0e0f10",
            "--target-scope",
            "x_myapp",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"dry_run\":true"))
        .stdout(predicate::str::contains("\"ok\":true"))
        // Source scope is global
        .stdout(predicate::str::contains("\"scope\":\"global\""))
        // Target scope is custom
        .stdout(predicate::str::contains("\"scope\":\"x_myapp\""))
        // requires_confirmation=true because of warnings
        .stdout(predicate::str::contains("\"requires_confirmation\":true"));
}

#[tokio::test]
async fn test_scope_move_file_named_to_global_scope_dry_run() {
    // Named (x_*) scope → global scope: re-scoping a custom app file back to global.
    let server = MockServer::start().await;

    let result_json = serde_json::json!({
        "ok": true,
        "dry_run": true,
        "table": "sys_script_include",
        "sys_id": "1112131415161718191a1b1c1d1e1f20",
        "changed_fields": ["sys_scope", "sys_package", "sys_name", "api_name"],
        "warnings": [],
        "requires_confirmation": false,
        "before": {
            "sys_scope": "scope-custom",
            "sys_package": "scope-custom",
            "sys_name": "x_myapp_Helper",
            "api_name": "x_myapp.Helper"
        },
        "after": {
            "sys_scope": "global-scope-id",
            "sys_package": "global-scope-id",
            "sys_name": "x_myapp_Helper",
            "api_name": "x_myapp.Helper"
        },
        "source_scope": {
            "sys_id": "scope-custom",
            "scope": "x_myapp",
            "name": "My App",
            "version": "1.0.0"
        },
        "target_scope": {
            "sys_id": "global-scope-id",
            "scope": "global",
            "name": "Global",
            "version": ""
        }
    })
    .to_string();

    mount_move_file_mocks(&server, "move-n2g-session", "move-n2g-gck", &result_json).await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "scope",
            "move-file",
            "sys_script_include",
            "1112131415161718191a1b1c1d1e1f20",
            "--target-scope",
            "global",
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"dry_run\":true"))
        .stdout(predicate::str::contains("\"ok\":true"))
        // Source scope is custom
        .stdout(predicate::str::contains("\"scope\":\"x_myapp\""))
        // Target scope is global
        .stdout(predicate::str::contains("\"scope\":\"global\""))
        .stdout(predicate::str::contains("\"requires_confirmation\":false"));
}

#[tokio::test]
async fn test_scope_move_file_global_to_global_rejected() {
    // Moving a file from global to global (same scope) should fail.
    let server = MockServer::start().await;

    let result_json = serde_json::json!({
        "ok": false,
        "dry_run": true,
        "table": "sys_script_include",
        "sys_id": "0102030405060708090a0b0c0d0e0f10",
        "warnings": [],
        "requires_confirmation": false,
        "error": "Source and target scope are the same."
    })
    .to_string();

    mount_move_file_mocks(&server, "move-g2g-session", "move-g2g-gck", &result_json).await;

    let (_dir, config_path) = api_key_config();

    cargo_bin_cmd!("snow-cli")
        .env("SNOW_CLI_CONFIG", &config_path)
        .env("SNOW_CLI_API_TOKEN", "test-api-token")
        .args([
            "--instance",
            &server.uri(),
            "scope",
            "move-file",
            "sys_script_include",
            "0102030405060708090a0b0c0d0e0f10",
            "--target-scope",
            "global",
            "--dry-run",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Source and target scope are the same",
        ));
}
