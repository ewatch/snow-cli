use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use base64::Engine;
use serde_json::{Map, Value, json};

use crate::cli::args::{
    DEFAULT_SNU_FIELDS, OutputFormat, SnuArgs, SnuBrokerCommands, SnuCommands, SnuContextCommands,
    SnuTabCommands,
};
use crate::cli::io::{DEFAULT_MAX_STDIN_BYTES, read_to_string_limited};
use crate::cli::output::print_output;
use crate::snu::broker::{BrokerBridge, BrokerStatus};
use crate::snu::protocol::{SnuInstance, SnuMessage, redact_session_for_output, resolve_origin};

pub async fn handle(
    args: SnuArgs,
    instance: Option<&str>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    // Resolve the optional global `--instance` selector to a normalized origin up
    // front so every session-backed subcommand targets the same instance's
    // `g_ck`. When the SN-Utils tab is a portal to several instances, this picks
    // which one; omitting it uses the most recently active instance.
    let target_origin = match instance {
        Some(value) => Some(resolve_origin(value).ok_or_else(|| {
            anyhow!("invalid --instance value '{value}': expected a ServiceNow URL or host")
        })?),
        None => None,
    };
    match args.command {
        SnuCommands::CheckConnection {
            timeout_secs,
            verify,
        } => handle_check_connection(timeout_secs, verify, target_origin, output_format).await,
        SnuCommands::GetInstanceInfo { timeout_secs: _ } => {
            handle_get_instance_info(target_origin, output_format).await
        }
        SnuCommands::WaitToken { timeout_secs } => {
            let (_bridge, instance) =
                connect_and_wait_for_fresh_session(timeout_secs, target_origin).await?;
            print_output(&redact_session_for_output(&instance), output_format)
        }
        SnuCommands::Query {
            table,
            query,
            fields,
            limit,
            order_by,
            timeout_secs,
        } => {
            let (bridge, instance) =
                connect_and_wait_for_session(timeout_secs, target_origin).await?;
            let correlation_id = correlation_id("query");
            let query_string =
                build_table_query_string(&fields, limit, query.as_deref(), order_by.as_deref());
            let payload = json!({
                "action": "agentQueryRecords",
                "agentRequestId": correlation_id,
                "tableName": table,
                "queryString": query_string,
                "instance": instance,
                "appName": "snow-cli",
            });
            let response = bridge
                .send_action_and_wait(&payload, &correlation_id, timeout_secs)
                .await?;
            let value = json!({
                "table": response.extra.get("tableName").and_then(Value::as_str).unwrap_or(&table),
                "count": response.extra.get("count").and_then(Value::as_u64).unwrap_or_else(|| response.extra.get("records").and_then(Value::as_array).map(|a| a.len() as u64).unwrap_or(0)),
                "records": response.extra.get("records").cloned().unwrap_or_else(|| json!([])),
            });
            print_output(&value, output_format)
        }
        SnuCommands::Schema {
            table,
            timeout_secs,
        } => {
            let (bridge, instance) =
                connect_and_wait_for_session(timeout_secs, target_origin).await?;
            let correlation_id = correlation_id("schema");
            let payload = json!({
                "action": "requestTableStructure",
                "agentRequestId": correlation_id,
                "tableName": table,
                "instance": instance,
                "appName": "snow-cli",
            });
            let response = bridge
                .send_action_and_wait(&payload, &correlation_id, timeout_secs)
                .await?;
            print_response_value(response, output_format)
        }
        SnuCommands::ExecuteBgScript {
            file,
            code,
            timeout_secs,
        } => {
            let script = resolve_script(file, code)?;
            let (bridge, instance) =
                connect_and_wait_for_session(timeout_secs, target_origin).await?;
            let payload = json!({
                "action": "executeBackgroundScript",
                "content": script,
                "instance": instance,
                "appName": "snow-cli",
            });
            let response = bridge
                .send_action_and_wait_for_action(
                    &payload,
                    "responseFromBackgroundScript",
                    timeout_secs,
                )
                .await?;
            print_background_script_response(response, output_format)
        }
        SnuCommands::CreateRecord {
            table,
            data,
            scope,
            timeout_secs,
        } => {
            handle_create_record(
                table,
                data,
                scope,
                timeout_secs,
                target_origin,
                output_format,
            )
            .await
        }
        SnuCommands::AppMeta {
            app_id,
            timeout_secs,
        } => handle_app_meta(app_id, timeout_secs, target_origin, output_format).await,
        SnuCommands::ListTables { timeout_secs } => {
            handle_list_tables(timeout_secs, target_origin, output_format).await
        }
        SnuCommands::GetRecord {
            table,
            sys_id,
            fields,
            timeout_secs,
        } => {
            handle_get_record(
                table,
                sys_id,
                fields,
                timeout_secs,
                target_origin,
                output_format,
            )
            .await
        }
        SnuCommands::UpdateRecord {
            table,
            sys_id,
            data,
            field,
            content,
            await_confirmation,
            timeout_secs,
        } => {
            handle_update_record(
                table,
                sys_id,
                data,
                field,
                content,
                await_confirmation,
                timeout_secs,
                target_origin,
                output_format,
            )
            .await
        }
        SnuCommands::DeleteRecord {
            table,
            sys_id,
            query,
            confirm,
            limit,
            dry_run,
            timeout_secs,
        } => {
            handle_delete_record(
                DeleteRecordRequest {
                    table,
                    sys_id,
                    query,
                    confirm,
                    limit,
                    dry_run,
                },
                timeout_secs,
                target_origin,
                output_format,
            )
            .await
        }
        SnuCommands::Slash {
            command,
            url,
            tab_id,
            no_auto_run,
            timeout_secs,
        } => {
            let bridge = connect_bridge(
                timeout_secs,
                Some("snow-cli SN-Utils bridge connected. This command does not require /token."),
            )
            .await?;
            let correlation_id = correlation_id("slash");
            let payload = json!({
                "action": "runSlashCommand",
                "agentRequestId": correlation_id,
                "command": command,
                "url": url,
                "tabId": tab_id,
                "autoRun": !no_auto_run,
                "appName": "snow-cli",
            });
            let response = bridge
                .send_action_and_wait(&payload, &correlation_id, timeout_secs)
                .await?;
            print_response_value(response, output_format)
        }
        SnuCommands::Tab(tab_args) => {
            match tab_args.command {
                SnuTabCommands::Activate {
                    url,
                    reload,
                    wait_for_load,
                    open_if_not_found,
                    timeout_secs,
                } => {
                    let bridge = connect_bridge(
                    timeout_secs,
                    Some("snow-cli SN-Utils bridge connected. This command does not require /token."),
                )
                .await?;
                    let correlation_id = correlation_id("tab");
                    let payload = json!({
                        "action": "activateTab",
                        "agentRequestId": correlation_id,
                        "url": url,
                        "reload": reload,
                        "waitForLoad": wait_for_load,
                        "openIfNotFound": open_if_not_found,
                        "appName": "snow-cli",
                    });
                    let response = bridge
                        .send_action_and_wait(&payload, &correlation_id, timeout_secs)
                        .await?;
                    print_response_value(response, output_format)
                }
            }
        }
        SnuCommands::Context(context_args) => match context_args.command {
            SnuContextCommands::Switch {
                switch_type,
                value,
                no_reload_tab,
                tab_url,
                timeout_secs,
            } => {
                let (bridge, instance) =
                    connect_and_wait_for_session(timeout_secs, target_origin).await?;
                let correlation_id = correlation_id("context");
                let payload = json!({
                    "action": "switchContext",
                    "agentRequestId": correlation_id,
                    "switchType": switch_type.as_action_value(),
                    "value": value,
                    "reloadTab": !no_reload_tab,
                    "tabUrl": tab_url,
                    "instance": instance,
                    "appName": "snow-cli",
                });
                let response = bridge
                    .send_action_and_wait(&payload, &correlation_id, timeout_secs)
                    .await?;
                print_response_value(response, output_format)
            }
        },
        SnuCommands::Screenshot {
            url,
            tab_id,
            out_path,
            timeout_secs,
        } => {
            if url.is_none() && tab_id.is_none() {
                return Err(anyhow!("missing required option: --url or --tab-id"));
            }
            let bridge = connect_bridge(
                timeout_secs,
                Some("snow-cli SN-Utils bridge connected. This command does not require /token."),
            )
            .await?;
            let correlation_id = correlation_id("screenshot");
            let payload = json!({
                "action": "takeScreenshot",
                "agentRequestId": correlation_id,
                "url": url,
                "tabId": tab_id,
                "appName": "snow-cli",
            });
            let response = bridge
                .send_action_and_wait(&payload, &correlation_id, timeout_secs)
                .await?;
            let value = save_screenshot_response(response, out_path.as_deref())?;
            print_output(&value, output_format)
        }
        SnuCommands::AttachmentUpload {
            table,
            sys_id,
            file,
            content_type,
            timeout_secs,
        } => {
            let (bridge, instance) =
                connect_and_wait_for_session(timeout_secs, target_origin).await?;
            let file_path = PathBuf::from(&file);
            let bytes = std::fs::read(&file_path)
                .with_context(|| format!("failed to read attachment file: {file}"))?;
            let file_name = file_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| anyhow!("attachment file path has no valid file name: {file}"))?;
            let content_type =
                content_type.unwrap_or_else(|| guess_content_type(&file_path).to_string());
            let image_data = base64::engine::general_purpose::STANDARD.encode(bytes);
            let correlation_id = correlation_id("attachment");
            let payload = json!({
                "action": "uploadAttachment",
                "agentRequestId": correlation_id,
                "tableName": table,
                "recordSysId": sys_id,
                "fileName": file_name,
                "imageData": image_data,
                "contentType": content_type,
                "instance": instance,
                "appName": "snow-cli",
            });
            let response = bridge
                .send_action_and_wait(&payload, &correlation_id, timeout_secs)
                .await?;
            print_response_value(response, output_format)
        }
        SnuCommands::Broker(args) => {
            match args.command {
                SnuBrokerCommands::Status => {
                    let status = crate::snu::broker::broker_status().await?;
                    print_output(&status, output_format)
                }
                SnuBrokerCommands::Stop => {
                    crate::snu::broker::stop_broker().await?;
                    print_output(&json!({ "stopped": true }), output_format)
                }
                SnuBrokerCommands::Clear { instance } => {
                    let origin = match instance.as_deref() {
                    Some(value) => Some(resolve_origin(value).ok_or_else(|| {
                        anyhow!("invalid --instance value '{value}': expected a ServiceNow URL or host")
                    })?),
                    None => None,
                };
                    let cleared = crate::snu::broker::clear_broker_sessions(origin).await?;
                    print_output(
                        &json!({ "cleared": cleared, "cleared_count": cleared.len() }),
                        output_format,
                    )
                }
                SnuBrokerCommands::Serve => crate::snu::broker::run_broker_server().await,
            }
        }
    }
}

async fn connect_bridge(timeout_secs: u64, banner: Option<&str>) -> anyhow::Result<BrokerBridge> {
    let bridge = BrokerBridge::connect_or_spawn().await?;
    tracing::debug!("SN-Utils broker connected");

    if let Some(message) = banner {
        let _ = bridge.send_banner(message, timeout_secs).await;
    }

    Ok(bridge)
}

async fn connect_and_wait_for_session(
    timeout_secs: u64,
    target_origin: Option<String>,
) -> anyhow::Result<(BrokerBridge, SnuInstance)> {
    let bridge = connect_bridge(
        timeout_secs,
        Some("snow-cli SN-Utils bridge connected. Run /token in a ServiceNow tab if the helper has not sent the browser session yet."),
    )
    .await?;
    let instance = bridge
        .wait_for_session(timeout_secs, false, target_origin)
        .await?;
    Ok((bridge, instance))
}

async fn connect_and_wait_for_fresh_session(
    timeout_secs: u64,
    target_origin: Option<String>,
) -> anyhow::Result<(BrokerBridge, SnuInstance)> {
    let bridge = connect_bridge(
        timeout_secs,
        Some("snow-cli SN-Utils bridge connected. Run /token in a ServiceNow tab to refresh the browser session metadata."),
    )
    .await?;
    let instance = bridge
        .wait_for_session(timeout_secs, true, target_origin)
        .await?;
    Ok((bridge, instance))
}

/// Report bridge/browser connectivity without hanging on the legacy
/// `{"command":"check_connection"}` payload, which the current SN-Utils
/// ScriptSync helper never answers. Instead we (1) ensure the broker is running,
/// (2) probe the helper tab with a live banner round-trip over the WebSocket —
/// which requires no `/token` and proves the tab is responsive — and (3) fold in
/// the broker's own session bookkeeping.
async fn handle_check_connection(
    timeout_secs: u64,
    verify: bool,
    target_origin: Option<String>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let bridge = BrokerBridge::connect_or_spawn().await?;
    // A successful banner send means the helper tab is connected and the socket
    // accepts writes. Failure (e.g. no helper tab within the connect timeout) is
    // reported as `connected: false` rather than propagated, so `check-connection`
    // always returns a useful snapshot instead of erroring out.
    let browser_responsive = bridge
        .send_banner(
            "snow-cli check-connection: SN-Utils bridge is responsive.",
            timeout_secs,
        )
        .await
        .is_ok();
    // `--verify` proves (or disproves) the cached g_ck against ServiceNow with a
    // cheap probe query. Verification failure is reported in the snapshot, not
    // propagated, so the connectivity half of the output always arrives.
    let verification = if verify {
        Some(bridge.verify_session(timeout_secs, target_origin).await)
    } else {
        None
    };
    let status = crate::snu::broker::broker_status().await?;
    print_output(
        &build_check_connection_result(&status, browser_responsive, verification),
        output_format,
    )
}

/// Report instance metadata from the broker's session state (URL, origin,
/// captured `g_ck` presence, and scope) instead of the legacy
/// `{"command":"get_instance_info"}` payload the current helper never answers.
/// `connect_or_spawn` restarts the broker (reloading any persisted sessions) if
/// it is not already running.
async fn handle_get_instance_info(
    target_origin: Option<String>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let _bridge = BrokerBridge::connect_or_spawn().await?;
    let status = crate::snu::broker::broker_status().await?;
    let value = build_instance_info_result(&status, target_origin.as_deref())?;
    print_output(&value, output_format)
}

fn build_check_connection_result(
    status: &BrokerStatus,
    browser_responsive: bool,
    verification: Option<anyhow::Result<bool>>,
) -> Value {
    let mut result = json!({
        "connected": browser_responsive,
        "broker_running": true,
        "broker_version": status.version,
        "ipc_addr": status.ipc_addr,
        "browser_connected": status.browser_connected,
        "session_count": status.session_count,
        "latest_instance_url": status.latest_instance_url,
        "instances": status.instances,
    });
    if let (Some(verification), Some(object)) = (verification, result.as_object_mut()) {
        match verification {
            Ok(valid) => {
                object.insert("token_valid".to_string(), Value::Bool(valid));
                if !valid {
                    object.insert(
                        "hint".to_string(),
                        Value::String(
                            "ServiceNow rejected the cached session token. Run /token in a ServiceNow tab to refresh it.".to_string(),
                        ),
                    );
                }
            }
            Err(error) => {
                object.insert("token_valid".to_string(), Value::Null);
                object.insert("verify_error".to_string(), Value::String(error.to_string()));
            }
        }
    }
    result
}

fn build_instance_info_result(
    status: &BrokerStatus,
    target_origin: Option<&str>,
) -> anyhow::Result<Value> {
    let instance = match target_origin {
        Some(origin) => status
            .instances
            .iter()
            .find(|instance| instance.origin == origin)
            .ok_or_else(|| {
                anyhow!(
                    "no SN-Utils browser session for {origin}. Run /token in a ServiceNow tab for that instance first."
                )
            })?,
        None => status
            .instances
            .iter()
            .find(|instance| instance.is_latest)
            .or_else(|| status.instances.first())
            .ok_or_else(|| {
                anyhow!("no SN-Utils browser session yet. Run /token in a ServiceNow tab first.")
            })?,
    };
    Ok(json!({
        "url": instance.url,
        "origin": instance.origin,
        "has_g_ck": instance.has_g_ck,
        "scope": instance.scope,
        "is_latest": instance.is_latest,
        "browser_connected": status.browser_connected,
        "captured_at": instance.captured_at,
        "last_verified_at": instance.last_verified_at,
    }))
}

/// Read a JSON object payload from `--data` or, when absent, from stdin.
fn resolve_record_data(data: Option<String>) -> anyhow::Result<Map<String, Value>> {
    let raw = match data {
        Some(data) if !data.trim().is_empty() => data,
        _ => {
            let stdin = std::io::stdin();
            if stdin.is_terminal() {
                anyhow::bail!(
                    "No record data provided. Pass --data '<json>' or pipe a JSON object on stdin."
                );
            }
            read_to_string_limited(
                stdin.lock(),
                DEFAULT_MAX_STDIN_BYTES,
                "record data stdin input",
            )
            .context("failed to read record data from stdin")?
        }
    };
    parse_json_object(&raw, "data")
}

async fn handle_create_record(
    table: String,
    data: Option<String>,
    scope: Option<String>,
    timeout_secs: u64,
    target_origin: Option<String>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let payload = resolve_record_data(data)?;
    if payload.is_empty() {
        anyhow::bail!("record data must contain at least one field");
    }

    let (bridge, instance) = connect_and_wait_for_session(timeout_secs, target_origin).await?;
    let correlation_id = correlation_id("create");
    let response = bridge
        .send_action_and_wait(
            &json!({
                "action": "createRecord",
                "agentRequestId": correlation_id,
                "tableName": table,
                // SN-Utils' createRecord handler reads the record body from
                // `payload` (it rejects the message with "Missing payload or
                // tableName for createRecord" otherwise). `fields` is kept for
                // compatibility with helper builds that read that key instead.
                "payload": payload,
                "fields": payload,
                "scope": scope,
                "instance": instance,
                "appName": "snow-cli",
            }),
            &correlation_id,
            timeout_secs,
        )
        .await?;

    let record = response
        .extra
        .get("result")
        .or_else(|| response.extra.get("record"))
        .cloned()
        .unwrap_or_else(|| serde_json::to_value(&response).unwrap_or(Value::Null));
    let sys_id = record.get("sys_id").and_then(Value::as_str);
    print_output(
        &json!({
            "success": true,
            "table": table,
            "sys_id": sys_id,
            "record": record,
        }),
        output_format,
    )
}

async fn handle_app_meta(
    app_id: String,
    timeout_secs: u64,
    target_origin: Option<String>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let (bridge, instance) = connect_and_wait_for_session(timeout_secs, target_origin).await?;
    let correlation_id = correlation_id("app_meta");
    let response = bridge
        .send_action_and_wait(
            &json!({
                "action": "requestAppMeta",
                "agentRequestId": correlation_id,
                "appId": app_id,
                "instance": instance,
                "appName": "snow-cli",
            }),
            &correlation_id,
            timeout_secs,
        )
        .await?;
    print_output(
        &json!({ "app_id": app_id, "meta": response.extra }),
        output_format,
    )
}

async fn handle_list_tables(
    timeout_secs: u64,
    target_origin: Option<String>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let (bridge, instance) = connect_and_wait_for_session(timeout_secs, target_origin).await?;
    let response = query_records_via_bridge(
        &bridge,
        &instance,
        QueryRecordsRequest {
            table: "sys_db_object",
            fields: "name",
            limit: 10_000,
            query: Some("nameISNOTEMPTY"),
            order_by: Some("ORDERBYname"),
        },
        timeout_secs,
    )
    .await?;

    let tables = response
        .extra
        .get("records")
        .and_then(Value::as_array)
        .map(|records| {
            records
                .iter()
                .filter_map(|record| {
                    record
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    print_output(&json!({ "tables": tables }), output_format)
}

async fn handle_get_record(
    table: String,
    sys_id: String,
    fields: Option<String>,
    timeout_secs: u64,
    target_origin: Option<String>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let (bridge, instance) = connect_and_wait_for_session(timeout_secs, target_origin).await?;
    let fields = fields
        .as_deref()
        .filter(|fields| !fields.trim().is_empty())
        .unwrap_or(DEFAULT_SNU_FIELDS);
    let sys_id_query = format!("sys_id={sys_id}");
    let response = query_records_via_bridge(
        &bridge,
        &instance,
        QueryRecordsRequest {
            table: &table,
            fields,
            limit: 1,
            query: Some(&sys_id_query),
            order_by: None,
        },
        timeout_secs,
    )
    .await?;
    let record = response
        .extra
        .get("records")
        .and_then(Value::as_array)
        .and_then(|records| records.first())
        .cloned()
        .unwrap_or(Value::Null);
    print_output(
        &json!({ "table": table, "sys_id": sys_id, "record": record }),
        output_format,
    )
}

/// Build the field/value map for an update from either `--data` (JSON object)
/// or the single-field `--field`/`--content` convenience pair.
fn resolve_update_fields(
    data: Option<String>,
    field: Option<String>,
    content: Option<String>,
) -> anyhow::Result<Map<String, Value>> {
    match (data, field, content) {
        (Some(data), _, _) => {
            let object = parse_json_object(&data, "data")?;
            if object.is_empty() {
                anyhow::bail!("--data must contain at least one key/value pair");
            }
            Ok(object)
        }
        (None, Some(field), Some(content)) => {
            let mut object = Map::new();
            object.insert(field, Value::String(content));
            Ok(object)
        }
        (None, _, _) => {
            anyhow::bail!("provide either --data '<json object>' or both --field and --content")
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_update_record(
    table: String,
    sys_id: String,
    data: Option<String>,
    field: Option<String>,
    content: Option<String>,
    await_confirmation: bool,
    timeout_secs: u64,
    target_origin: Option<String>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let fields_object = resolve_update_fields(data, field, content)?;
    let requested_fields: Vec<String> = fields_object.keys().cloned().collect();
    // Mutations run as a server-side background script over the SN-Utils bridge
    // (the same `executeBackgroundScript` channel that `execute-bg-script` uses).
    // A direct `X-UserToken` REST call cannot work because the broker only
    // captures the `g_ck`, not the session cookies ServiceNow validates it
    // against, so the request is always rejected. The generated GlideRecord
    // script prints a machine-parseable JSON result we parse for success.
    let (bridge, instance) = connect_and_wait_for_session(timeout_secs, target_origin).await?;
    let script = build_update_script(&table, &sys_id, &fields_object);
    let response = run_bg_mutation(&bridge, &instance, &script, timeout_secs).await?;

    if !await_confirmation {
        return print_output(
            &json!({
                "success": true,
                "table": table,
                "sys_id": sys_id,
                "fields": requested_fields,
                "response": response,
            }),
            output_format,
        );
    }

    let requested_field_refs: Vec<&str> = requested_fields.iter().map(|s| s.as_str()).collect();
    let persisted = snu_fetch_persisted_record(
        &bridge,
        &instance,
        timeout_secs,
        &table,
        &sys_id,
        &requested_field_refs,
    )
    .await?;
    let warnings = snu_fields_warnings(&requested_fields, &persisted);
    print_output(
        &json!({
            "success": true,
            "awaited": true,
            "table": table,
            "sys_id": sys_id,
            "fields": requested_fields,
            "persisted": persisted,
            "warnings": warnings,
            "response": response,
        }),
        output_format,
    )
}

struct DeleteRecordRequest {
    table: String,
    sys_id: Option<String>,
    query: Option<String>,
    confirm: bool,
    limit: Option<u32>,
    dry_run: bool,
}

async fn handle_delete_record(
    request: DeleteRecordRequest,
    timeout_secs: u64,
    target_origin: Option<String>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let (bridge, instance) = connect_and_wait_for_session(timeout_secs, target_origin).await?;

    if let Some(sys_id) = request.sys_id {
        if request.dry_run {
            let record = fetch_record_by_sys_id(
                &bridge,
                &instance,
                timeout_secs,
                &request.table,
                &sys_id,
                "sys_id,number,short_description,name",
            )
            .await?;
            return print_output(
                &json!({
                    "dry_run": true,
                    "table": request.table,
                    "sys_id": sys_id,
                    "record": record,
                }),
                output_format,
            );
        }

        let script = build_delete_script(&request.table, &sys_id);
        let response = run_bg_mutation(&bridge, &instance, &script, timeout_secs).await?;
        return print_output(
            &json!({
                "deleted": true,
                "table": request.table,
                "sys_id": sys_id,
                "response": response,
            }),
            output_format,
        );
    }

    let query = request
        .query
        .ok_or_else(|| anyhow!("missing required option: --sys-id or --query"))?;
    let limit = request
        .limit
        .ok_or_else(|| anyhow!("missing required option for bulk delete: --limit"))?;
    if limit == 0 {
        anyhow::bail!("--limit must be greater than 0");
    }
    if !request.confirm {
        anyhow::bail!("bulk delete requires --confirm");
    }

    let response = query_records_via_bridge(
        &bridge,
        &instance,
        QueryRecordsRequest {
            table: &request.table,
            fields: "sys_id,number,short_description,name",
            limit,
            query: Some(&query),
            order_by: None,
        },
        timeout_secs,
    )
    .await?;
    let records = response
        .extra
        .get("records")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if request.dry_run {
        return print_output(
            &json!({
                "dry_run": true,
                "table": request.table,
                "query": query,
                "limit": limit,
                "records": records,
            }),
            output_format,
        );
    }

    // Delete the matching records in a single server-side background script so we
    // get one real acknowledgement instead of N cookie-less REST calls. The
    // script re-runs the same encoded query under the same limit and reports the
    // sys_ids it actually deleted; anything the preview matched but the script
    // did not delete is surfaced as a failure.
    let matched_sys_ids: Vec<String> = records
        .iter()
        .filter_map(|record| record.get("sys_id").and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    let script = build_bulk_delete_script(&request.table, &query, limit);
    let result = run_bg_mutation(&bridge, &instance, &script, timeout_secs).await?;
    let deleted: Vec<String> = result
        .get("deleted")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let failed: Vec<Value> = matched_sys_ids
        .iter()
        .filter(|sys_id| !deleted.contains(*sys_id))
        .map(|sys_id| json!({"sys_id": sys_id, "error": "not deleted by server script"}))
        .collect();

    print_output(
        &json!({
            "deleted_count": deleted.len(),
            "failed_count": failed.len(),
            "deleted": deleted,
            "failed": failed,
            "table": request.table,
            "query": query,
            "limit": limit,
        }),
        output_format,
    )
}

fn snu_fields_warnings(fields: &[String], persisted: &Value) -> Vec<Value> {
    fields
        .iter()
        .filter(|field| matches!(persisted.get(field.as_str()), Some(Value::Null) | None))
        .map(|field| json!({"field": field, "warning": "field missing or empty after await"}))
        .collect()
}

fn parse_json_object(input: &str, label: &str) -> anyhow::Result<Map<String, Value>> {
    let value: Value = serde_json::from_str(input)
        .with_context(|| format!("failed to parse {label} as JSON object"))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => anyhow::bail!("{label} must be a JSON object"),
    }
}

async fn snu_fetch_persisted_record(
    bridge: &BrokerBridge,
    instance: &SnuInstance,
    timeout_secs: u64,
    table: &str,
    sys_id: &str,
    field_names: &[&str],
) -> anyhow::Result<Value> {
    let fields = if field_names.is_empty() {
        "sys_id".to_string()
    } else {
        field_names.join(",")
    };
    fetch_record_by_sys_id(bridge, instance, timeout_secs, table, sys_id, &fields).await
}

async fn fetch_record_by_sys_id(
    bridge: &BrokerBridge,
    instance: &SnuInstance,
    timeout_secs: u64,
    table: &str,
    sys_id: &str,
    fields: &str,
) -> anyhow::Result<Value> {
    let sys_id_query = format!("sys_id={sys_id}");
    let response = query_records_via_bridge(
        bridge,
        instance,
        QueryRecordsRequest {
            table,
            fields,
            limit: 1,
            query: Some(&sys_id_query),
            order_by: None,
        },
        timeout_secs,
    )
    .await?;
    Ok(response
        .extra
        .get("records")
        .and_then(Value::as_array)
        .and_then(|records| records.first())
        .cloned()
        .unwrap_or(Value::Null))
}

struct QueryRecordsRequest<'a> {
    table: &'a str,
    fields: &'a str,
    limit: u32,
    query: Option<&'a str>,
    order_by: Option<&'a str>,
}

async fn query_records_via_bridge(
    bridge: &BrokerBridge,
    instance: &SnuInstance,
    request: QueryRecordsRequest<'_>,
    timeout_secs: u64,
) -> anyhow::Result<SnuMessage> {
    let correlation_id = correlation_id("query");
    let query_string = build_table_query_string(
        request.fields,
        request.limit,
        request.query,
        request.order_by,
    );
    bridge
        .send_action_and_wait(
            &json!({
                "action": "agentQueryRecords",
                "agentRequestId": correlation_id,
                "tableName": request.table,
                "queryString": query_string,
                "instance": instance,
                "appName": "snow-cli",
            }),
            &correlation_id,
            timeout_secs,
        )
        .await
}

/// Marker prefix the generated mutation scripts print in front of their JSON
/// result line, so we can pick it out of the background-script output regardless
/// of any other logging the server emits around it.
const MUTATION_RESULT_MARKER: &str = "__SNOW_CLI_RESULT__:";

/// Serialize a JSON value into a form that is safe to embed as a literal inside
/// generated JavaScript. `serde_json` already escapes quotes, backslashes and
/// control characters; we additionally escape U+2028/U+2029, which are valid in
/// JSON strings but are line terminators in JavaScript and would otherwise break
/// a string literal in the Rhino engine ServiceNow runs.
fn js_json_literal(value: &Value) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "null".to_string())
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// Build a server-side background script that updates a single record via
/// `GlideRecord` and prints a machine-parseable JSON result. User-supplied
/// values are embedded as a JSON literal (never string-concatenated) so quotes,
/// backslashes, newlines and unicode cannot break out of the script.
fn build_update_script(table: &str, sys_id: &str, fields: &Map<String, Value>) -> String {
    let table_lit = js_json_literal(&Value::String(table.to_string()));
    let sys_id_lit = js_json_literal(&Value::String(sys_id.to_string()));
    let fields_lit = js_json_literal(&Value::Object(fields.clone()));
    format!(
        r#"(function() {{
  var __table = {table_lit};
  var __sysId = {sys_id_lit};
  var __fields = {fields_lit};
  var __gr = new GlideRecord(__table);
  if (!__gr.get(__sysId)) {{
    gs.print("{MUTATION_RESULT_MARKER}" + JSON.stringify({{ success: false, action: "update", table: __table, sys_id: __sysId, updated: 0, error: "record not found" }}));
    return;
  }}
  for (var __key in __fields) {{
    if (__fields.hasOwnProperty(__key)) {{ __gr.setValue(__key, __fields[__key]); }}
  }}
  __gr.update();
  gs.print("{MUTATION_RESULT_MARKER}" + JSON.stringify({{ success: true, action: "update", table: __table, sys_id: __gr.getUniqueValue(), updated: 1 }}));
}})();"#
    )
}

/// Build a server-side background script that deletes a single record by sys_id.
fn build_delete_script(table: &str, sys_id: &str) -> String {
    let table_lit = js_json_literal(&Value::String(table.to_string()));
    let sys_id_lit = js_json_literal(&Value::String(sys_id.to_string()));
    format!(
        r#"(function() {{
  var __table = {table_lit};
  var __sysId = {sys_id_lit};
  var __gr = new GlideRecord(__table);
  if (!__gr.get(__sysId)) {{
    gs.print("{MUTATION_RESULT_MARKER}" + JSON.stringify({{ success: false, action: "delete", table: __table, sys_id: __sysId, deleted: 0, error: "record not found" }}));
    return;
  }}
  __gr.deleteRecord();
  gs.print("{MUTATION_RESULT_MARKER}" + JSON.stringify({{ success: true, action: "delete", table: __table, sys_id: __sysId, deleted: 1 }}));
}})();"#
    )
}

/// Build a server-side background script that deletes every record matching an
/// encoded query, capped by `limit`, and prints the sys_ids it removed.
fn build_bulk_delete_script(table: &str, query: &str, limit: u32) -> String {
    let table_lit = js_json_literal(&Value::String(table.to_string()));
    let query_lit = js_json_literal(&Value::String(query.to_string()));
    let limit_lit = js_json_literal(&Value::from(limit));
    format!(
        r#"(function() {{
  var __table = {table_lit};
  var __query = {query_lit};
  var __limit = {limit_lit};
  var __gr = new GlideRecord(__table);
  __gr.addEncodedQuery(__query);
  __gr.setLimit(__limit);
  __gr.query();
  var __deleted = [];
  while (__gr.next()) {{
    var __id = __gr.getUniqueValue();
    if (__gr.deleteRecord()) {{ __deleted.push(__id); }}
  }}
  gs.print("{MUTATION_RESULT_MARKER}" + JSON.stringify({{ success: true, action: "deleteBulk", table: __table, query: __query, limit: __limit, deleted_count: __deleted.length, deleted: __deleted }}));
}})();"#
    )
}

/// Run a generated mutation script over the SN-Utils `executeBackgroundScript`
/// bridge (the proven channel) and return the parsed JSON result the script
/// printed. Errors out when the script reported `success: false`.
async fn run_bg_mutation(
    bridge: &BrokerBridge,
    instance: &SnuInstance,
    script: &str,
    timeout_secs: u64,
) -> anyhow::Result<Value> {
    let response = bridge
        .send_action_and_wait_for_action(
            &json!({
                "action": "executeBackgroundScript",
                "content": script,
                "instance": instance,
                "appName": "snow-cli",
            }),
            "responseFromBackgroundScript",
            timeout_secs,
        )
        .await?;
    let data = response
        .extra
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("SN-Utils background script response did not contain data"))?;
    let result = parse_mutation_result(data)?;
    if result.get("success").and_then(Value::as_bool) == Some(false) {
        let error = result
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("mutation reported failure");
        return Err(anyhow!("ServiceNow mutation failed: {error}"));
    }
    Ok(result)
}

/// Extract the JSON result a mutation script printed. The helper tab forwards
/// the raw `sys.scripts.do` output, so the marker's JSON can arrive wrapped in
/// the page's HTML (`{...}<BR/></PRE></BODY></HTML>`) and with HTML-escaped
/// entities; both are stripped before parsing, and anything after the first
/// complete JSON value is ignored. A marker-less response that looks like a
/// login page means ServiceNow no longer honors the browser session.
fn parse_mutation_result(data: &str) -> anyhow::Result<Value> {
    let decoded = decode_html_entities(data);
    if let Some(idx) = decoded.rfind(MUTATION_RESULT_MARKER) {
        let after = decoded[idx + MUTATION_RESULT_MARKER.len()..].trim_start();
        return first_json_value(after)
            .with_context(|| format!("failed to parse SN-Utils mutation result as JSON: {after}"));
    }
    // sys.scripts.do answers a logged-out session with a redirect the helper
    // forwards as empty output — observed live, so treat it as the auth signal
    // it is rather than a generic parse failure.
    if decoded.trim().is_empty() {
        anyhow::bail!(
            "SN-Utils returned empty background-script output, which usually means the browser session on the instance has expired. Log in again if needed, run /token in a ServiceNow tab, and retry."
        );
    }
    if looks_like_login_page(&decoded) {
        anyhow::bail!(
            "SN-Utils session appears to be logged out: the background script returned a login page instead of a result. Run /token in a ServiceNow tab for this instance and retry."
        );
    }
    let trimmed = decoded.trim();
    first_json_value(trimmed).with_context(|| {
        format!("SN-Utils background script did not return a parseable mutation result: {trimmed}")
    })
}

/// Parse the first complete JSON value in `text`, ignoring whatever trails it
/// (typically the `<BR/></PRE>...` HTML the script output is embedded in).
fn first_json_value(text: &str) -> anyhow::Result<Value> {
    let mut stream = serde_json::Deserializer::from_str(text).into_iter::<Value>();
    match stream.next() {
        Some(Ok(value)) => Ok(value),
        Some(Err(error)) => Err(error.into()),
        None => Err(anyhow!("empty response")),
    }
}

/// Minimal decode for the HTML entities `sys.scripts.do` escapes in script
/// output. `&amp;` is decoded last so double-escaped input cannot re-expand.
fn decode_html_entities(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Heuristic for a ServiceNow login page returned where script output was
/// expected — the signature of a browser session ServiceNow no longer accepts.
fn looks_like_login_page(text: &str) -> bool {
    let lower = text.to_lowercase();
    (lower.contains("<html") || lower.contains("<!doctype"))
        && (lower.contains("login")
            || lower.contains("logged out")
            || lower.contains("not authenticated")
            || lower.contains("user_name"))
}

fn correlation_id(prefix: &str) -> String {
    format!("snow_{prefix}_{}", uuid::Uuid::new_v4().simple())
}

fn build_table_query_string(
    fields: &str,
    limit: u32,
    query: Option<&str>,
    order_by: Option<&str>,
) -> String {
    let mut pairs = vec![
        format!("sysparm_fields={}", urlencoding::encode(fields)),
        format!("sysparm_limit={limit}"),
    ];

    let combined_query = match (query, order_by) {
        (Some(query), Some(order_by)) if !query.is_empty() && !order_by.is_empty() => {
            Some(format!("{query}^{order_by}"))
        }
        (Some(query), _) if !query.is_empty() => Some(query.to_string()),
        (_, Some(order_by)) if !order_by.is_empty() => Some(order_by.to_string()),
        _ => None,
    };

    if let Some(query) = combined_query {
        pairs.push(format!("sysparm_query={}", urlencoding::encode(&query)));
    }

    pairs.join("&")
}

fn print_response_value(response: SnuMessage, output_format: &OutputFormat) -> anyhow::Result<()> {
    let mut value = serde_json::to_value(&response)?;
    if let Value::Object(map) = &mut value {
        map.remove("agentRequestId");
    }
    print_output(&value, output_format)
}

fn print_background_script_response(
    response: SnuMessage,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let data = response
        .extra
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("SN-Utils background script response did not contain data"))?;

    match output_format {
        OutputFormat::Json | OutputFormat::Text => match serde_json::from_str::<Value>(data) {
            Ok(json) => println!("{}", serde_json::to_string_pretty(&json)?),
            Err(_) => println!("{}", data),
        },
        OutputFormat::Csv => {
            println!("{}", data);
        }
        OutputFormat::Jsonl => match serde_json::from_str::<Value>(data) {
            Ok(json) => print_output(&json, output_format)?,
            Err(_) => println!("{}", data),
        },
        OutputFormat::Toon => match serde_json::from_str::<Value>(data) {
            Ok(json) => print_output(&json, output_format)?,
            Err(_) => println!("{}", data),
        },
        OutputFormat::Auto => match serde_json::from_str::<Value>(data) {
            Ok(json) => print_output(&json, output_format)?,
            Err(_) => println!("{}", data),
        },
    }

    Ok(())
}

fn save_screenshot_response(response: SnuMessage, out_path: Option<&str>) -> anyhow::Result<Value> {
    let image_data = response
        .extra
        .get("imageData")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("SN-Utils screenshot response did not contain imageData"))?;
    let file_name = response
        .extra
        .get("fileName")
        .and_then(Value::as_str)
        .unwrap_or("screenshot.png");
    let path = out_path
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(file_name));
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(image_data)
        .context("failed to decode SN-Utils screenshot imageData")?;
    std::fs::write(&path, bytes)
        .with_context(|| format!("failed to write screenshot: {}", path.display()))?;

    Ok(json!({
        "saved": true,
        "file": path,
        "url": response.extra.get("url").cloned().or_else(|| response.extra.get("tabUrl").cloned()),
        "tabTitle": response.extra.get("tabTitle").cloned(),
    }))
}

fn resolve_script(file: Option<String>, code: Option<String>) -> anyhow::Result<String> {
    resolve_script_from(
        file,
        code,
        std::io::stdin().lock(),
        std::io::stdin().is_terminal(),
    )
}

fn resolve_script_from<R: std::io::Read>(
    file: Option<String>,
    code: Option<String>,
    reader: R,
    is_tty: bool,
) -> anyhow::Result<String> {
    if let Some(c) = code {
        if c.trim().is_empty() {
            anyhow::bail!("Empty script provided via --code.");
        }
        return Ok(c);
    }

    if let Some(path) = file {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("Failed to read script file '{}': {}", path, e))?;
        if content.trim().is_empty() {
            anyhow::bail!("Script file '{}' is empty.", path);
        }
        return Ok(content);
    }

    if is_tty {
        anyhow::bail!(
            "No script provided. Use --code '<script>', --file <path>, or pipe script to stdin."
        );
    }

    let buf = read_to_string_limited(reader, DEFAULT_MAX_STDIN_BYTES, "script stdin input")?;

    if buf.trim().is_empty() {
        anyhow::bail!("No script received from stdin.");
    }

    Ok(buf)
}

fn guess_content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "json" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "zip" => "application/zip",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snu::broker::InstanceSummary;
    use crate::snu::protocol::normalize_origin;
    use std::io::Cursor;

    fn instance_summary(url: &str, is_latest: bool) -> InstanceSummary {
        InstanceSummary {
            url: url.to_string(),
            origin: normalize_origin(url).unwrap(),
            has_g_ck: true,
            scope: Some("global".to_string()),
            is_latest,
            captured_at: Some(1_700_000_000),
            last_verified_at: None,
        }
    }

    fn broker_status(instances: Vec<InstanceSummary>, browser_connected: bool) -> BrokerStatus {
        let latest = instances
            .iter()
            .find(|i| i.is_latest)
            .map(|i| i.url.clone());
        BrokerStatus {
            version: "0.0.0-test".to_string(),
            ipc_addr: "127.0.0.1:1979".to_string(),
            browser_connected,
            session_count: instances.len(),
            latest_instance_url: latest,
            instances,
            idle_timeout_secs: 1800,
        }
    }

    #[test]
    fn check_connection_result_reflects_probe_and_state() {
        let status = broker_status(
            vec![instance_summary("https://dev.service-now.com", true)],
            true,
        );
        let value = build_check_connection_result(&status, true, None);
        assert_eq!(value["connected"], true);
        assert_eq!(value["broker_running"], true);
        assert_eq!(value["browser_connected"], true);
        assert_eq!(value["session_count"], 1);
        assert_eq!(value["latest_instance_url"], "https://dev.service-now.com");
        assert_eq!(value["instances"].as_array().unwrap().len(), 1);
        assert!(value.get("token_valid").is_none());
    }

    #[test]
    fn check_connection_result_marks_unresponsive_probe_disconnected() {
        let status = broker_status(Vec::new(), false);
        let value = build_check_connection_result(&status, false, None);
        assert_eq!(value["connected"], false);
        assert_eq!(value["session_count"], 0);
    }

    #[test]
    fn check_connection_result_reports_token_validity() {
        let status = broker_status(
            vec![instance_summary("https://dev.service-now.com", true)],
            true,
        );
        let valid = build_check_connection_result(&status, true, Some(Ok(true)));
        assert_eq!(valid["token_valid"], true);
        assert!(valid.get("hint").is_none());

        let dead = build_check_connection_result(&status, true, Some(Ok(false)));
        assert_eq!(dead["token_valid"], false);
        assert!(
            dead["hint"]
                .as_str()
                .is_some_and(|hint| hint.contains("/token"))
        );

        let failed = build_check_connection_result(&status, true, Some(Err(anyhow!("no session"))));
        assert_eq!(failed["token_valid"], Value::Null);
        assert_eq!(failed["verify_error"], "no session");
    }

    #[test]
    fn instance_info_result_defaults_to_latest_session() {
        let status = broker_status(
            vec![
                instance_summary("https://a.service-now.com", false),
                instance_summary("https://b.service-now.com", true),
            ],
            true,
        );
        let value = build_instance_info_result(&status, None).unwrap();
        assert_eq!(value["url"], "https://b.service-now.com");
        assert_eq!(value["has_g_ck"], true);
        assert_eq!(value["scope"], "global");
        assert_eq!(value["is_latest"], true);
    }

    #[test]
    fn instance_info_result_selects_requested_origin() {
        let status = broker_status(
            vec![
                instance_summary("https://a.service-now.com", false),
                instance_summary("https://b.service-now.com", true),
            ],
            true,
        );
        let origin = normalize_origin("https://a.service-now.com").unwrap();
        let value = build_instance_info_result(&status, Some(&origin)).unwrap();
        assert_eq!(value["url"], "https://a.service-now.com");
    }

    #[test]
    fn instance_info_result_errors_without_session() {
        let status = broker_status(Vec::new(), false);
        let err = build_instance_info_result(&status, None).unwrap_err();
        assert!(err.to_string().contains("Run /token"));
    }

    #[test]
    fn instance_info_result_errors_for_unknown_origin() {
        let status = broker_status(
            vec![instance_summary("https://a.service-now.com", true)],
            true,
        );
        let err = build_instance_info_result(&status, Some("https://gone.service-now.com:443"))
            .unwrap_err();
        assert!(err.to_string().contains("no SN-Utils browser session for"));
    }

    #[test]
    fn query_string_encodes_fields_and_query() {
        let qs = build_table_query_string(
            "sys_id,number",
            5,
            Some("active=true"),
            Some("ORDERBYnumber"),
        );
        assert_eq!(
            qs,
            "sysparm_fields=sys_id%2Cnumber&sysparm_limit=5&sysparm_query=active%3Dtrue%5EORDERBYnumber"
        );
    }

    #[test]
    fn resolve_script_from_code_takes_precedence() {
        let script = resolve_script_from(
            Some("file.js".into()),
            Some("gs.info('from code')".into()),
            Cursor::new(b"ignored"),
            false,
        )
        .unwrap();
        assert_eq!(script, "gs.info('from code')");
    }

    #[test]
    fn resolve_script_from_stdin() {
        let script =
            resolve_script_from(None, None, Cursor::new(b"gs.info('stdin')"), false).unwrap();
        assert_eq!(script, "gs.info('stdin')");
    }

    #[test]
    fn resolve_script_from_tty_no_input_errors() {
        let err = resolve_script_from(None, None, Cursor::new(b""), true).unwrap_err();
        assert!(err.to_string().contains("No script provided"));
    }

    fn fields_from(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn js_json_literal_escapes_quotes_backslashes_and_newlines() {
        let value = Value::String("a\"b\\c\nd\tE".to_string());
        let literal = js_json_literal(&value);
        // Valid JSON (and thus a valid JS literal) that round-trips exactly.
        let back: Value = serde_json::from_str(&literal).unwrap();
        assert_eq!(back, value);
        assert!(
            !literal.contains('\n'),
            "raw newline must not leak: {literal}"
        );
    }

    #[test]
    fn js_json_literal_escapes_js_line_separators() {
        let value = Value::String("a\u{2028}b\u{2029}c".to_string());
        let literal = js_json_literal(&value);
        assert!(!literal.contains('\u{2028}'));
        assert!(!literal.contains('\u{2029}'));
        assert!(literal.contains("\\u2028"));
        assert!(literal.contains("\\u2029"));
    }

    #[test]
    fn update_script_embeds_fields_safely() {
        let fields = fields_from(&[
            (
                "short_description",
                Value::String("line1\nline2 \"q\"".into()),
            ),
            (
                "script",
                Value::String("gs.info('hi'); var x = \"</y>\";".into()),
            ),
            ("count", Value::from(3)),
            ("emoji", Value::String("héllo 🚀".into())),
        ]);
        let script = build_update_script("incident", "abc'123\\", &fields);
        // No raw newline from user data should appear outside the template lines.
        assert!(script.contains("new GlideRecord(__table)"));
        assert!(script.contains(MUTATION_RESULT_MARKER));
        assert!(script.contains("__gr.update()"));
        // The embedded fields literal must be valid JSON round-tripping the input.
        let start = script.find("var __fields = ").unwrap() + "var __fields = ".len();
        let rest = &script[start..];
        let end = rest.find(";\n").unwrap();
        let parsed: Value = serde_json::from_str(rest[..end].trim()).unwrap();
        assert_eq!(parsed, Value::Object(fields));
    }

    #[test]
    fn delete_scripts_are_generated() {
        let single = build_delete_script("incident", "abc123");
        assert!(single.contains("__gr.deleteRecord()"));
        assert!(single.contains(MUTATION_RESULT_MARKER));

        let bulk = build_bulk_delete_script("incident", "active=true^ORDERBYnumber", 25);
        assert!(bulk.contains("addEncodedQuery(__query)"));
        assert!(bulk.contains("setLimit(__limit)"));
        assert!(bulk.contains("var __limit = 25;"));
    }

    #[test]
    fn parse_mutation_result_extracts_marked_line() {
        let data = format!(
            "*** Script: running\n{MUTATION_RESULT_MARKER}{{\"success\":true,\"updated\":1}}\ndone\n"
        );
        let result = parse_mutation_result(&data).unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["updated"], 1);
    }

    #[test]
    fn parse_mutation_result_uses_last_marker() {
        let data = format!(
            "{MUTATION_RESULT_MARKER}{{\"success\":false}}\n{MUTATION_RESULT_MARKER}{{\"success\":true}}"
        );
        let result = parse_mutation_result(&data).unwrap();
        assert_eq!(result["success"], true);
    }

    #[test]
    fn parse_mutation_result_falls_back_to_whole_output() {
        let result = parse_mutation_result("  {\"success\":true,\"deleted\":1}  ").unwrap();
        assert_eq!(result["deleted"], 1);
    }

    #[test]
    fn parse_mutation_result_errors_on_garbage() {
        assert!(parse_mutation_result("not json at all").is_err());
    }

    #[test]
    fn parse_mutation_result_strips_html_wrapper() {
        // Live-observed shape: sys.scripts.do wraps the printed line in the
        // page's HTML, so the JSON is followed by <BR/> and closing tags.
        let data = format!(
            "*** Script: {MUTATION_RESULT_MARKER}{{\"success\":true,\"action\":\"update\",\"updated\":1}}<BR/></PRE><HR/></BODY></HTML>"
        );
        let value = parse_mutation_result(&data).unwrap();
        assert_eq!(value["success"], true);
        assert_eq!(value["updated"], 1);
    }

    #[test]
    fn parse_mutation_result_decodes_html_entities() {
        let data = format!(
            "{MUTATION_RESULT_MARKER}{{&quot;success&quot;:true,&quot;action&quot;:&quot;delete&quot;,&quot;deleted&quot;:1}}<BR/>"
        );
        let value = parse_mutation_result(&data).unwrap();
        assert_eq!(value["success"], true);
        assert_eq!(value["deleted"], 1);
    }

    #[test]
    fn parse_mutation_result_maps_empty_output_to_expired_session() {
        let error = parse_mutation_result("  \n ").unwrap_err().to_string();
        assert!(
            error.contains("/token"),
            "error should point at /token: {error}"
        );
        assert!(
            error.contains("expired"),
            "error should mention expiry: {error}"
        );
    }

    #[test]
    fn parse_mutation_result_detects_login_page() {
        let data = "<html><head><title>Login</title></head><body><form><input name=\"user_name\"/></form></body></html>";
        let error = parse_mutation_result(data).unwrap_err().to_string();
        assert!(
            error.contains("/token"),
            "error should point at /token: {error}"
        );
        assert!(
            error.contains("logged out"),
            "error should say the session is logged out: {error}"
        );
    }

    #[test]
    fn guesses_common_content_types() {
        assert_eq!(guess_content_type(Path::new("a.png")), "image/png");
        assert_eq!(
            guess_content_type(Path::new("a.unknown")),
            "application/octet-stream"
        );
    }
}
