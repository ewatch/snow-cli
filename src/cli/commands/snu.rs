use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use base64::Engine;
use reqwest::{Client, Method};
use serde_json::{Map, Value, json};

use crate::cli::args::{
    DEFAULT_SNU_FIELDS, OutputFormat, SnuArgs, SnuBrokerCommands, SnuCommands, SnuContextCommands,
    SnuTabCommands,
};
use crate::cli::output::print_output;
use crate::cli::validation::{DEFAULT_MAX_STDIN_BYTES, read_to_string_limited};
use crate::snu::broker::BrokerBridge;
use crate::snu::protocol::{
    SnuInstance, SnuMessage, normalize_origin, redact_session_for_output, resolve_origin,
};

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
        SnuCommands::CheckConnection { timeout_secs } => {
            let bridge = connect_bridge(timeout_secs, None).await?;
            let payload = json!({
                "id": "0",
                "command": "check_connection",
            });
            let response = bridge.send_payload_and_wait(&payload, timeout_secs).await?;
            print_response_value(response, output_format)
        }
        SnuCommands::GetInstanceInfo { timeout_secs } => {
            let bridge = connect_bridge(timeout_secs, None).await?;
            let payload = json!({
                "id": "2",
                "command": "get_instance_info",
            });
            let response = bridge.send_payload_and_wait(&payload, timeout_secs).await?;
            print_response_value(response, output_format)
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
    // The bridge is used only for the optional read-back below: SN-Utils'
    // `updateRecord` helper sends no correlated success response over the
    // WebSocket, so the mutating PUT itself goes out over direct HTTP using the
    // browser session's `g_ck` that the broker captured.
    let (bridge, mut instance) = connect_and_wait_for_session(timeout_secs, target_origin).await?;
    let mut refreshed = false;
    let path = format!("/api/now/table/{table}/{sys_id}");
    let response = snu_http_request_with_refresh(
        &bridge,
        &mut instance,
        &mut refreshed,
        timeout_secs,
        Method::PUT,
        &path,
        Some(Value::Object(fields_object.clone())),
    )
    .await?;

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
    let (bridge, mut instance) = connect_and_wait_for_session(timeout_secs, target_origin).await?;
    let mut refreshed = false;

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

        let path = format!("/api/now/table/{}/{}", request.table, sys_id);
        let response = snu_http_request_with_refresh(
            &bridge,
            &mut instance,
            &mut refreshed,
            timeout_secs,
            Method::DELETE,
            &path,
            None,
        )
        .await?;
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

    let mut deleted = Vec::new();
    let mut failed = Vec::new();
    for record in records {
        let Some(record_sys_id) = record.get("sys_id").and_then(Value::as_str) else {
            failed.push(json!({"record": record, "error": "missing sys_id"}));
            continue;
        };
        let record_path = format!("/api/now/table/{}/{}", request.table, record_sys_id);
        match snu_http_request_with_refresh(
            &bridge,
            &mut instance,
            &mut refreshed,
            timeout_secs,
            Method::DELETE,
            &record_path,
            None,
        )
        .await
        {
            Ok(_) => deleted.push(record_sys_id.to_string()),
            Err(error) => failed.push(json!({"sys_id": record_sys_id, "error": error.to_string()})),
        }
    }

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

/// Perform a direct ServiceNow REST call authenticated with the browser
/// session's `g_ck` (replayed as `X-UserToken`).
///
/// Mutating record operations use this rather than the WebSocket bridge because
/// SN-Utils' ScriptSync helper does not give us a usable acknowledgement for
/// them: its `updateRecord` handler sends no correlated success response, and it
/// has no `deleteRecord` handler at all (the message falls through to
/// `updateRecord`). Both would therefore hang until the command timed out.
/// Reads/queries still go over the bridge, where SN-Utils does reply with a
/// matching `agentRequestId`.
async fn snu_http_request(
    instance: &SnuInstance,
    timeout_secs: u64,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> anyhow::Result<Value> {
    let token = instance
        .g_ck
        .as_deref()
        .ok_or_else(|| anyhow!("SN-Utils session is missing a g_ck token"))?;
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()?;
    let url = reqwest::Url::parse(&format!("{}{}", instance.url.trim_end_matches('/'), path))?;
    let mut request = client
        .request(method, url)
        .header(reqwest::header::ACCEPT, "application/json")
        .header("X-UserToken", token);
    if let Some(body) = body {
        request = request
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&body);
    }

    let response = request.send().await?;
    let status = response.status();
    let text = response.text().await?;

    if !status.is_success() {
        return Err(anyhow!(
            "ServiceNow request failed with HTTP {}: {}",
            status,
            if text.trim().is_empty() {
                "<empty response>"
            } else {
                &text
            }
        ));
    }

    if text.trim().is_empty() {
        return Ok(Value::Null);
    }

    serde_json::from_str(&text).with_context(|| format!("invalid JSON from ServiceNow: {text}"))
}

/// `true` when a direct-HTTP error looks like an expired/rejected `g_ck`.
fn is_stale_token_error(error: &anyhow::Error) -> bool {
    let text = error.to_string().to_lowercase();
    text.contains("http 401")
        || text.contains("http 403")
        || text.contains("not authenticated")
        || text.contains("unauthorized")
        || text.contains("forbidden")
}

/// Run a mutating direct-HTTP request, and if ServiceNow rejects the session's
/// `g_ck` as expired, ask the broker to capture a fresh one (which prompts the
/// user to re-run `/token`) and retry exactly once. `instance` is updated in
/// place so any follow-up call in the same command reuses the refreshed token,
/// and `refreshed` guards against refresh loops.
async fn snu_http_request_with_refresh(
    bridge: &BrokerBridge,
    instance: &mut SnuInstance,
    refreshed: &mut bool,
    timeout_secs: u64,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> anyhow::Result<Value> {
    match snu_http_request(instance, timeout_secs, method.clone(), path, body.clone()).await {
        Ok(value) => Ok(value),
        Err(error) if !*refreshed && is_stale_token_error(&error) => {
            tracing::info!(
                "ServiceNow rejected the cached SN-Utils token; refreshing it via the helper tab"
            );
            // Scope the refresh to this instance's own origin so a `/token` from
            // another tab can't redirect the retry to a different instance.
            let origin = normalize_origin(&instance.url);
            *instance = bridge.refresh_session(timeout_secs, origin).await?;
            *refreshed = true;
            snu_http_request(instance, timeout_secs, method, path, body).await
        }
        Err(error) => Err(error),
    }
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
    use std::io::Cursor;

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

    #[test]
    fn guesses_common_content_types() {
        assert_eq!(guess_content_type(Path::new("a.png")), "image/png");
        assert_eq!(
            guess_content_type(Path::new("a.unknown")),
            "application/octet-stream"
        );
    }
}
