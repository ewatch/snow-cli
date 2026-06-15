use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use base64::Engine;
use reqwest::{Client, Method};
use serde_json::{Map, Value, json};

use crate::cli::args::{
    OutputFormat, SnuArgs, SnuCommands, SnuContextCommands, SnuTabCommands, SnuDaemonCommands,
};
use crate::cli::output::print_output;
use crate::cli::validation::{DEFAULT_MAX_STDIN_BYTES, read_to_string_limited};
use crate::snu::bridge::SnuBridge;
use crate::snu::daemon;
use crate::snu::daemon::{DaemonRequest, DaemonResponse};
use crate::snu::protocol::{SnuInstance, SnuMessage, redact_session_for_output};
use crate::snu::session_cache;

pub async fn handle(args: SnuArgs, output_format: &OutputFormat) -> anyhow::Result<()> {
    match args.command {
        SnuCommands::Daemon(daemon_cmd) => {
            handle_daemon(daemon_cmd, output_format).await
        }
        SnuCommands::CheckConnection { timeout_secs } => {
            let mut bridge = connect_bridge(timeout_secs, None).await?;
            let payload = json!({
                "id": "0",
                "command": "check_connection",
            });
            let response = bridge.send_payload_and_wait(&payload, timeout_secs).await?;
            print_response_value(response, output_format)
        }
        SnuCommands::GetInstanceInfo { timeout_secs } => {
            let mut bridge = connect_bridge(timeout_secs, None).await?;
            let payload = json!({
                "id": "2",
                "command": "get_instance_info",
            });
            let response = bridge.send_payload_and_wait(&payload, timeout_secs).await?;
            print_response_value(response, output_format)
        }
        SnuCommands::WaitToken { timeout_secs } => {
            let (_bridge, instance) = connect_and_wait_for_fresh_session(timeout_secs).await?;
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
            let (mut bridge, instance) = connect_and_wait_for_session(timeout_secs).await?;
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
                .send_action_and_wait(
                    &payload,
                    payload["agentRequestId"].as_str().unwrap(),
                    timeout_secs,
                )
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
            let (mut bridge, instance) = connect_and_wait_for_session(timeout_secs).await?;
            let correlation_id = correlation_id("schema");
            let payload = json!({
                "action": "requestTableStructure",
                "agentRequestId": correlation_id,
                "tableName": table,
                "instance": instance,
                "appName": "snow-cli",
            });
            let response = bridge
                .send_action_and_wait(
                    &payload,
                    payload["agentRequestId"].as_str().unwrap(),
                    timeout_secs,
                )
                .await?;
            print_response_value(response, output_format)
        }
        SnuCommands::ExecuteBgScript {
            file,
            code,
            timeout_secs,
        } => {
            let script = resolve_script(file, code)?;
            let (mut bridge, instance) = connect_and_wait_for_session(timeout_secs).await?;
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
        SnuCommands::ListTables { timeout_secs } => {
            handle_list_tables(timeout_secs, output_format).await
        }
        SnuCommands::GetRecord {
            table,
            sys_id,
            fields,
            timeout_secs,
        } => handle_get_record(table, sys_id, fields, timeout_secs, output_format).await,
        SnuCommands::UpdateRecord {
            table,
            sys_id,
            field,
            content,
            await_confirmation,
            timeout_secs,
        } => {
            handle_update_record(
                table,
                sys_id,
                field,
                content,
                await_confirmation,
                timeout_secs,
                output_format,
            )
            .await
        }
        SnuCommands::UpdateRecordBatch {
            table,
            sys_id,
            fields,
            await_confirmation,
            timeout_secs,
        } => {
            handle_update_record_batch(
                table,
                sys_id,
                fields,
                await_confirmation,
                timeout_secs,
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
            let mut bridge = connect_bridge(
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
                .send_action_and_wait(
                    &payload,
                    payload["agentRequestId"].as_str().unwrap(),
                    timeout_secs,
                )
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
                    let mut bridge = connect_bridge(
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
                        .send_action_and_wait(
                            &payload,
                            payload["agentRequestId"].as_str().unwrap(),
                            timeout_secs,
                        )
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
                let (mut bridge, instance) = connect_and_wait_for_session(timeout_secs).await?;
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
                    .send_action_and_wait(
                        &payload,
                        payload["agentRequestId"].as_str().unwrap(),
                        timeout_secs,
                    )
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
            let mut bridge = connect_bridge(
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
                .send_action_and_wait(
                    &payload,
                    payload["agentRequestId"].as_str().unwrap(),
                    timeout_secs,
                )
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
            let (mut bridge, instance) = connect_and_wait_for_session(timeout_secs).await?;
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
                .send_action_and_wait(
                    &payload,
                    payload["agentRequestId"].as_str().unwrap(),
                    timeout_secs,
                )
                .await?;
            print_response_value(response, output_format)
        }
    }
}

async fn handle_daemon(cmd: SnuDaemonCommands, output_format: &OutputFormat) -> anyhow::Result<()> {
    match cmd {
        SnuDaemonCommands::Start { timeout_secs } => {
            // If already running, error
            if daemon::is_running() {
                let state = daemon::read_state();
                anyhow::bail!(
                    "bridge daemon is already running (PID {})",
                    state.as_ref().map(|s| s.pid).unwrap_or(0)
                );
            }
            // Run the daemon (blocks until shutdown)
            daemon::run_daemon(timeout_secs).await
        }
        SnuDaemonCommands::Stop => {
            daemon::stop_daemon().await?;
            print_output(&json!({"status": "stopped"}), output_format)
        }
        SnuDaemonCommands::Status => {
            if daemon::is_running() {
                let resp = daemon::send_request(&DaemonRequest {
                    id: "status".into(),
                    cmd: "status".into(),
                    payload: json!({}),
                }).await?;
                if resp.success {
                    print_output(&resp.data.unwrap_or(json!({"running": true})), output_format)
                } else {
                    print_output(&json!({"running": true}), output_format)
                }
            } else {
                print_output(&json!({"running": false}), output_format)
            }
        }
    }
}

/// Try to perform a bridge action through the daemon.
/// Returns Ok(Some(response)) if daemon handled it, Ok(None) if daemon not available.
async fn daemon_bridge_action(
    payload: &serde_json::Value,
    _timeout_secs: u64,
) -> anyhow::Result<Option<DaemonResponse>> {
    if !daemon::is_running() {
        return Ok(None);
    }

    let request = DaemonRequest {
        id: payload.get("agentRequestId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("action_{}", uuid::Uuid::new_v4().simple())),
        cmd: "bridge-action".into(),
        payload: payload.clone(),
    };

    match daemon::send_request(&request).await {
        Ok(resp) => {
            if resp.success {
                Ok(Some(resp))
            } else {
                Err(anyhow!("bridge action failed: {}", resp.error.unwrap_or_default()))
            }
        }
        Err(e) => {
            tracing::warn!("daemon bridge action failed: {e}, falling back to direct bridge");
            Ok(None)
        }
    }
}

/// Try to get a session through the daemon.
/// Returns Ok(Some(instance)) if daemon provided it, Ok(None) if daemon not available.
async fn daemon_get_session(timeout_secs: u64) -> anyhow::Result<Option<SnuInstance>> {
    if !daemon::is_running() {
        return Ok(None);
    }

    // First check cached session on the daemon side
    let status_req = DaemonRequest {
        id: "session".into(),
        cmd: "session".into(),
        payload: json!({}),
    };

    match daemon::send_request(&status_req).await {
        Ok(resp) if resp.success => {
            if let Some(data) = &resp.data
                && data.get("has_session").and_then(Value::as_bool).unwrap_or(false)
            {
                let url = data.get("instance_url").and_then(Value::as_str).unwrap_or("");
                let name = data.get("instance_name").and_then(Value::as_str).unwrap_or("");
                return Ok(Some(SnuInstance {
                    name: name.to_string(),
                    url: url.to_string(),
                    g_ck: Some("cached".to_string()),
                    scope: data.get("scope").and_then(Value::as_str).map(str::to_string),
                }));
            }
            // No session cached, ask daemon to wait for /token
            let wait_req = DaemonRequest {
                id: "wait-for-session".into(),
                cmd: "wait-for-session".into(),
                payload: json!({"timeout_secs": timeout_secs}),
            };
            match daemon::send_request(&wait_req).await {
                Ok(wait_resp) if wait_resp.success => {
                    // Session is now cached, load from local keychain
                    if let Ok(Some(cached)) = session_cache::load_session() {
                        return Ok(Some(cached.instance));
                    }
                    Ok(None)
                }
                Ok(wait_resp) => {
                    Err(anyhow!("daemon failed to get session: {}", wait_resp.error.unwrap_or_default()))
                }
                Err(e) => {
                    tracing::warn!("daemon wait-for-session failed: {e}");
                    Ok(None)
                }
            }
        }
        Ok(_) => Ok(None),
        Err(e) => {
            tracing::warn!("daemon session check failed: {e}");
            Ok(None)
        }
    }
}

async fn connect_bridge(timeout_secs: u64, banner: Option<&str>) -> anyhow::Result<SnuBridge> {
    let mut bridge = SnuBridge::accept(timeout_secs).await?;
    tracing::debug!(peer_addr = %bridge.peer_addr(), "SN-Utils helper connected");

    if let Some(message) = banner {
        let _ = bridge.send_banner(message).await;
    }

    Ok(bridge)
}

async fn connect_and_wait_for_session(
    timeout_secs: u64,
) -> anyhow::Result<(SnuBridge, SnuInstance)> {
    let cached_session = session_cache::load_session()?;
    let banner = if cached_session.is_some() {
        None
    } else {
        Some(
            "snow-cli SN-Utils bridge connected. Reusing the cached browser token when available; run `snow-cli snu wait-token` to refresh it.",
        )
    };
    let bridge = connect_bridge(timeout_secs, banner).await?;

    if let Some(cached) = cached_session {
        tracing::debug!(
            instance_url = %cached.instance.url,
            saved_at_unix_secs = cached.saved_at_unix_secs,
            "Using cached SN-Utils browser session token"
        );
        return Ok((bridge, cached.instance));
    }

    let mut bridge = bridge;
    let instance = bridge.wait_for_session(timeout_secs).await?;
    session_cache::store_session(&instance)?;
    Ok((bridge, instance))
}

async fn connect_and_wait_for_fresh_session(
    timeout_secs: u64,
) -> anyhow::Result<(SnuBridge, SnuInstance)> {
    let mut bridge = connect_bridge(
        timeout_secs,
        Some("snow-cli SN-Utils bridge connected. Run /token in a ServiceNow tab to cache the browser token."),
    )
    .await?;
    let instance = bridge.wait_for_session(timeout_secs).await?;
    session_cache::store_session(&instance)?;
    Ok((bridge, instance))
}

async fn handle_list_tables(timeout_secs: u64, output_format: &OutputFormat) -> anyhow::Result<()> {
    let (_bridge, instance) = connect_and_wait_for_session(timeout_secs).await?;
    let response = snu_request_json(
        &instance,
        timeout_secs,
        Method::GET,
        "/api/now/table/sys_db_object?sysparm_fields=name&sysparm_query=nameISNOTEMPTY^ORDERBYname&sysparm_limit=10000",
        None,
    )
    .await?;

    let tables = response
        .get("result")
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
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let (_bridge, instance) = connect_and_wait_for_session(timeout_secs).await?;
    let path = match fields.as_deref() {
        Some(fields) if !fields.trim().is_empty() => format!(
            "/api/now/table/{table}/{sys_id}?sysparm_fields={}",
            urlencoding::encode(fields)
        ),
        _ => format!("/api/now/table/{table}/{sys_id}"),
    };
    let response = snu_request_json(&instance, timeout_secs, Method::GET, &path, None).await?;
    let record = response.get("result").cloned().unwrap_or(Value::Null);
    print_output(
        &json!({ "table": table, "sys_id": sys_id, "record": record }),
        output_format,
    )
}

async fn handle_update_record(
    table: String,
    sys_id: String,
    field: String,
    content: String,
    await_confirmation: bool,
    timeout_secs: u64,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let (_bridge, instance) = connect_and_wait_for_session(timeout_secs).await?;
    let mut body = Map::new();
    body.insert(field.clone(), Value::String(content.clone()));
    let path = format!("/api/now/table/{table}/{sys_id}");
    let response = snu_request_json(
        &instance,
        timeout_secs,
        Method::PUT,
        &path,
        Some(Value::Object(body)),
    )
    .await?;

    if !await_confirmation {
        return print_output(
            &json!({
                "success": true,
                "table": table,
                "sys_id": sys_id,
                "field": field,
                "response": response,
            }),
            output_format,
        );
    }

    let persisted =
        snu_fetch_persisted_record(&instance, timeout_secs, &path, &[field.as_str()]).await?;
    let warnings = snu_field_warnings(&field, &persisted);
    print_output(
        &json!({
            "success": true,
            "awaited": true,
            "table": table,
            "sys_id": sys_id,
            "field": field,
            "persisted": persisted,
            "warnings": warnings,
            "response": response,
        }),
        output_format,
    )
}

async fn handle_update_record_batch(
    table: String,
    sys_id: String,
    fields: String,
    await_confirmation: bool,
    timeout_secs: u64,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let fields_object = parse_json_object(&fields, "fields")?;
    if fields_object.is_empty() {
        anyhow::bail!("--fields must contain at least one key/value pair");
    }

    let requested_fields: Vec<String> = fields_object.keys().cloned().collect();
    let (_bridge, instance) = connect_and_wait_for_session(timeout_secs).await?;
    let path = format!("/api/now/table/{table}/{sys_id}");
    let response = snu_request_json(
        &instance,
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
    let persisted =
        snu_fetch_persisted_record(&instance, timeout_secs, &path, &requested_field_refs).await?;
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
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let (_bridge, instance) = connect_and_wait_for_session(timeout_secs).await?;

    if let Some(sys_id) = request.sys_id {
        let path = format!("/api/now/table/{}/{}", request.table, sys_id);
        if request.dry_run {
            let response =
                snu_request_json(&instance, timeout_secs, Method::GET, &path, None).await?;
            return print_output(
                &json!({
                    "dry_run": true,
                    "table": request.table,
                    "sys_id": sys_id,
                    "record": response.get("result").cloned().unwrap_or(Value::Null),
                }),
                output_format,
            );
        }

        let response =
            snu_request_json(&instance, timeout_secs, Method::DELETE, &path, None).await?;
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

    let query_path = format!(
        "/api/now/table/{table}?sysparm_fields=sys_id,number,short_description,name&sysparm_query={}&sysparm_limit={limit}",
        urlencoding::encode(&query),
        table = request.table
    );
    let response =
        snu_request_json(&instance, timeout_secs, Method::GET, &query_path, None).await?;
    let records = response
        .get("result")
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
        match snu_request_json(&instance, timeout_secs, Method::DELETE, &record_path, None).await {
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

fn snu_field_warnings(field: &str, persisted: &Value) -> Vec<Value> {
    match persisted.get(field) {
        Some(Value::Null) | None => {
            vec![json!({"field": field, "warning": "field missing or empty after await"})]
        }
        _ => Vec::new(),
    }
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
    instance: &SnuInstance,
    timeout_secs: u64,
    path: &str,
    field_names: &[&str],
) -> anyhow::Result<Value> {
    let field_list = field_names.join(",");
    let path = if field_list.is_empty() {
        path.to_string()
    } else if path.contains('?') {
        format!("{path}&sysparm_fields={}", urlencoding::encode(&field_list))
    } else {
        format!("{path}?sysparm_fields={}", urlencoding::encode(&field_list))
    };
    let response = snu_request_json(instance, timeout_secs, Method::GET, &path, None).await?;
    Ok(response.get("result").cloned().unwrap_or(Value::Null))
}

async fn snu_request_json(
    instance: &SnuInstance,
    timeout_secs: u64,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> anyhow::Result<Value> {
    let token = instance
        .g_ck
        .as_deref()
        .ok_or_else(|| anyhow!("SN-Utils session is missing a cached g_ck token"))?;
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
