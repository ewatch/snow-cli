use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use base64::Engine;
use serde_json::{Map, Value, json};

use crate::cli::args::{
    OutputFormat, SnuArgs, SnuBrokerCommands, SnuCommands, SnuContextCommands, SnuTabCommands,
};
use crate::cli::output::print_output;
use crate::cli::validation::{DEFAULT_MAX_STDIN_BYTES, read_to_string_limited};
use crate::snu::broker::BrokerBridge;
use crate::snu::protocol::{SnuInstance, SnuMessage, redact_session_for_output, resolve_origin};

mod record_ops;

const NO_TOKEN_BANNER: &str =
    "snow-cli SN-Utils bridge connected. This command does not require /token.";
const WAIT_TOKEN_BANNER: &str = "snow-cli SN-Utils bridge connected. Run /token in a ServiceNow tab if the helper has not sent the browser session yet.";
const REFRESH_TOKEN_BANNER: &str = "snow-cli SN-Utils bridge connected. Run /token in a ServiceNow tab to refresh the browser session metadata.";

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
            let query_string =
                build_table_query_string(&fields, limit, query.as_deref(), order_by.as_deref());
            let response = run_action(
                &bridge,
                "query",
                "agentQueryRecords",
                action_extra([
                    ("tableName", json!(&table)),
                    ("queryString", json!(query_string)),
                    ("instance", json!(instance)),
                ]),
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
            let (bridge, instance) =
                connect_and_wait_for_session(timeout_secs, target_origin).await?;
            let response = run_action(
                &bridge,
                "schema",
                "requestTableStructure",
                action_extra([("tableName", json!(&table)), ("instance", json!(instance))]),
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
            record_ops::handle_create_record(
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
        } => record_ops::handle_app_meta(app_id, timeout_secs, target_origin, output_format).await,
        SnuCommands::ListTables { timeout_secs } => {
            record_ops::handle_list_tables(timeout_secs, target_origin, output_format).await
        }
        SnuCommands::GetRecord {
            table,
            sys_id,
            fields,
            timeout_secs,
        } => {
            record_ops::handle_get_record(
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
            record_ops::handle_update_record(
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
            record_ops::handle_delete_record(
                record_ops::DeleteRecordRequest {
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
            let bridge = connect_bridge(timeout_secs, Some(NO_TOKEN_BANNER)).await?;
            let response = run_action(
                &bridge,
                "slash",
                "runSlashCommand",
                action_extra([
                    ("command", json!(command)),
                    ("url", json!(url)),
                    ("tabId", json!(tab_id)),
                    ("autoRun", json!(!no_auto_run)),
                ]),
                timeout_secs,
            )
            .await?;
            print_response_value(response, output_format)
        }
        SnuCommands::Tab(tab_args) => match tab_args.command {
            SnuTabCommands::Activate {
                url,
                reload,
                wait_for_load,
                open_if_not_found,
                timeout_secs,
            } => {
                let bridge = connect_bridge(timeout_secs, Some(NO_TOKEN_BANNER)).await?;
                let response = run_action(
                    &bridge,
                    "tab",
                    "activateTab",
                    action_extra([
                        ("url", json!(url)),
                        ("reload", json!(reload)),
                        ("waitForLoad", json!(wait_for_load)),
                        ("openIfNotFound", json!(open_if_not_found)),
                    ]),
                    timeout_secs,
                )
                .await?;
                print_response_value(response, output_format)
            }
        },
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
                let response = run_action(
                    &bridge,
                    "context",
                    "switchContext",
                    action_extra([
                        ("switchType", json!(switch_type.as_action_value())),
                        ("value", json!(value)),
                        ("reloadTab", json!(!no_reload_tab)),
                        ("tabUrl", json!(tab_url)),
                        ("instance", json!(instance)),
                    ]),
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
            let bridge = connect_bridge(timeout_secs, Some(NO_TOKEN_BANNER)).await?;
            let response = run_action(
                &bridge,
                "screenshot",
                "takeScreenshot",
                action_extra([("url", json!(url)), ("tabId", json!(tab_id))]),
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
            let response = run_action(
                &bridge,
                "attachment",
                "uploadAttachment",
                action_extra([
                    ("tableName", json!(table)),
                    ("recordSysId", json!(sys_id)),
                    ("fileName", json!(file_name)),
                    ("imageData", json!(image_data)),
                    ("contentType", json!(content_type)),
                    ("instance", json!(instance)),
                ]),
                timeout_secs,
            )
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

fn action_extra(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Map<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

async fn run_action(
    bridge: &BrokerBridge,
    correlation_prefix: &str,
    action: &str,
    mut extra: Map<String, Value>,
    timeout_secs: u64,
) -> anyhow::Result<SnuMessage> {
    let correlation_id = correlation_id(correlation_prefix);
    let mut payload = Map::new();
    payload.insert("action".to_string(), Value::String(action.to_string()));
    payload.insert(
        "agentRequestId".to_string(),
        Value::String(correlation_id.clone()),
    );
    payload.insert("appName".to_string(), Value::String("snow-cli".to_string()));
    payload.append(&mut extra);

    bridge
        .send_action_and_wait(&Value::Object(payload), &correlation_id, timeout_secs)
        .await
}

async fn connect_and_wait_for_session(
    timeout_secs: u64,
    target_origin: Option<String>,
) -> anyhow::Result<(BrokerBridge, SnuInstance)> {
    let bridge = connect_bridge(timeout_secs, Some(WAIT_TOKEN_BANNER)).await?;
    let instance = bridge
        .wait_for_session(timeout_secs, false, target_origin)
        .await?;
    Ok((bridge, instance))
}

async fn connect_and_wait_for_fresh_session(
    timeout_secs: u64,
    target_origin: Option<String>,
) -> anyhow::Result<(BrokerBridge, SnuInstance)> {
    let bridge = connect_bridge(timeout_secs, Some(REFRESH_TOKEN_BANNER)).await?;
    let instance = bridge
        .wait_for_session(timeout_secs, true, target_origin)
        .await?;
    Ok((bridge, instance))
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
