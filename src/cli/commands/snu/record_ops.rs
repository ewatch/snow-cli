use std::io::IsTerminal;

use anyhow::{Context, anyhow};
use reqwest::{Client, Method};
use serde_json::{Map, Value, json};

use crate::cli::args::{DEFAULT_SNU_FIELDS, OutputFormat};
use crate::cli::output::print_output;
use crate::cli::validation::{DEFAULT_MAX_STDIN_BYTES, read_to_string_limited};
use crate::snu::broker::BrokerBridge;
use crate::snu::protocol::{SnuInstance, normalize_origin};

pub(super) async fn handle_create_record(
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

    let (bridge, instance) =
        super::connect_and_wait_for_session(timeout_secs, target_origin).await?;
    let response = super::run_action(
        &bridge,
        "create",
        "createRecord",
        super::action_extra([
            ("tableName", json!(&table)),
            // SN-Utils' createRecord handler reads the record body from
            // `payload` (it rejects the message with "Missing payload or
            // tableName for createRecord" otherwise). `fields` is kept for
            // compatibility with helper builds that read that key instead.
            ("payload", json!(&payload)),
            ("fields", json!(&payload)),
            ("scope", json!(scope)),
            ("instance", json!(instance)),
        ]),
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

pub(super) async fn handle_app_meta(
    app_id: String,
    timeout_secs: u64,
    target_origin: Option<String>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let (bridge, instance) =
        super::connect_and_wait_for_session(timeout_secs, target_origin).await?;
    let response = super::run_action(
        &bridge,
        "app_meta",
        "requestAppMeta",
        super::action_extra([("appId", json!(&app_id)), ("instance", json!(instance))]),
        timeout_secs,
    )
    .await?;
    print_output(
        &json!({ "app_id": app_id, "meta": response.extra }),
        output_format,
    )
}

pub(super) async fn handle_list_tables(
    timeout_secs: u64,
    target_origin: Option<String>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let (bridge, instance) =
        super::connect_and_wait_for_session(timeout_secs, target_origin).await?;
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

pub(super) async fn handle_get_record(
    table: String,
    sys_id: String,
    fields: Option<String>,
    timeout_secs: u64,
    target_origin: Option<String>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let (bridge, instance) =
        super::connect_and_wait_for_session(timeout_secs, target_origin).await?;
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

#[expect(
    clippy::too_many_arguments,
    reason = "SNU update-record currently forwards clap fields until mutation requests are modeled"
)]
pub(super) async fn handle_update_record(
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
    let (bridge, mut instance) =
        super::connect_and_wait_for_session(timeout_secs, target_origin).await?;
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

pub(super) struct DeleteRecordRequest {
    pub table: String,
    pub sys_id: Option<String>,
    pub query: Option<String>,
    pub confirm: bool,
    pub limit: Option<u32>,
    pub dry_run: bool,
}

pub(super) async fn handle_delete_record(
    request: DeleteRecordRequest,
    timeout_secs: u64,
    target_origin: Option<String>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let (bridge, mut instance) =
        super::connect_and_wait_for_session(timeout_secs, target_origin).await?;
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
) -> anyhow::Result<crate::snu::protocol::SnuMessage> {
    let query_string = super::build_table_query_string(
        request.fields,
        request.limit,
        request.query,
        request.order_by,
    );
    super::run_action(
        bridge,
        "query",
        "agentQueryRecords",
        super::action_extra([
            ("tableName", json!(request.table)),
            ("queryString", json!(query_string)),
            ("instance", json!(instance)),
        ]),
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
