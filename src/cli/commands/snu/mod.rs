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

const NO_TOKEN_BANNER: &str =
    "snow-cli SN-Utils bridge connected. This command does not require /token.";

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
            let query_string =
                build_table_query_string(&fields, limit, query.as_deref(), order_by.as_deref());
            let mut fields = Map::new();
            fields.insert("tableName".to_string(), Value::String(table.clone()));
            fields.insert("queryString".to_string(), Value::String(query_string));
            fields.insert("instance".to_string(), serde_json::to_value(instance)?);
            let response = send_agent_action_with_prefix(
                &bridge,
                "query",
                "agentQueryRecords",
                fields,
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
            let mut fields = Map::new();
            fields.insert("tableName".to_string(), Value::String(table));
            fields.insert("instance".to_string(), serde_json::to_value(instance)?);
            let response = send_agent_action_with_prefix(
                &bridge,
                "schema",
                "requestTableStructure",
                fields,
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
            let response =
                send_background_script_action(&bridge, &instance, script, timeout_secs).await?;
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
            let bridge = connect_bridge(timeout_secs, Some(NO_TOKEN_BANNER)).await?;
            let mut fields = Map::new();
            fields.insert("command".to_string(), Value::String(command));
            fields.insert("url".to_string(), Value::String(url));
            fields.insert("tabId".to_string(), serde_json::to_value(tab_id)?);
            fields.insert("autoRun".to_string(), Value::Bool(!no_auto_run));
            let response = send_agent_action_with_prefix(
                &bridge,
                "slash",
                "runSlashCommand",
                fields,
                timeout_secs,
            )
            .await?;
            print_response_value(response, output_format)
        }
        SnuCommands::Rest(rest_args) => {
            handle_rest(rest_args.command, target_origin, output_format).await
        }
        SnuCommands::Page(page_args) => handle_page(page_args.command, output_format).await,
        SnuCommands::Records(records_args) => {
            handle_records_agent(records_args.command, target_origin, output_format).await
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
                let mut fields = Map::new();
                fields.insert("url".to_string(), Value::String(url));
                fields.insert("reload".to_string(), Value::Bool(reload));
                fields.insert("waitForLoad".to_string(), Value::Bool(wait_for_load));
                fields.insert("openIfNotFound".to_string(), Value::Bool(open_if_not_found));
                let response = send_agent_action_with_prefix(
                    &bridge,
                    "tab",
                    "activateTab",
                    fields,
                    timeout_secs,
                )
                .await?;
                print_response_value(response, output_format)
            }
        },
        SnuCommands::Context(context_args) => match context_args.command {
            SnuContextCommands::Get { timeout_secs } => {
                let (bridge, instance) =
                    connect_and_wait_for_session(timeout_secs, target_origin).await?;
                let mut fields = Map::new();
                fields.insert("instance".to_string(), serde_json::to_value(instance)?);
                let response = send_agent_action_with_prefix(
                    &bridge,
                    "context_get",
                    "agentGetContext",
                    fields,
                    timeout_secs,
                )
                .await?;
                print_response_value(response, output_format)
            }
            SnuContextCommands::Switch {
                switch_type,
                value,
                no_reload_tab,
                tab_url,
                timeout_secs,
            } => {
                let (bridge, instance) =
                    connect_and_wait_for_session(timeout_secs, target_origin).await?;
                let mut fields = Map::new();
                fields.insert(
                    "switchType".to_string(),
                    Value::String(switch_type.as_action_value().to_string()),
                );
                fields.insert("value".to_string(), Value::String(value));
                fields.insert("reloadTab".to_string(), Value::Bool(!no_reload_tab));
                fields.insert("tabUrl".to_string(), Value::String(tab_url));
                fields.insert("instance".to_string(), serde_json::to_value(instance)?);
                let response = send_agent_action_with_prefix(
                    &bridge,
                    "context",
                    "switchContext",
                    fields,
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
            let (bridge, instance) =
                connect_and_wait_for_session(timeout_secs, target_origin).await?;
            let mut fields = Map::new();
            fields.insert("url".to_string(), serde_json::to_value(url)?);
            fields.insert("tabId".to_string(), serde_json::to_value(tab_id)?);
            fields.insert("instance".to_string(), serde_json::to_value(instance)?);
            let response = send_agent_action_with_prefix(
                &bridge,
                "screenshot",
                "takeScreenshot",
                fields,
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
            let mut fields = Map::new();
            fields.insert("tableName".to_string(), Value::String(table));
            fields.insert("recordSysId".to_string(), Value::String(sys_id));
            fields.insert("fileName".to_string(), Value::String(file_name.to_string()));
            fields.insert("imageData".to_string(), Value::String(image_data));
            fields.insert("contentType".to_string(), Value::String(content_type));
            fields.insert("instance".to_string(), serde_json::to_value(instance)?);
            let response = send_agent_action_with_prefix(
                &bridge,
                "attachment",
                "uploadAttachment",
                fields,
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

mod agent;
mod browser;
mod mutations;
mod records;
mod response;
mod session;

use agent::*;
use browser::*;
use mutations::*;
use records::*;
use response::*;
use session::*;

#[cfg(test)]
mod tests;
