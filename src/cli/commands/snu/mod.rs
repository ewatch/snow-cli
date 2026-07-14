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

mod browser;
mod mutations;
mod records;
mod response;
mod session;

use browser::*;
use mutations::*;
use records::*;
use response::*;
use session::*;

#[cfg(test)]
mod tests;
