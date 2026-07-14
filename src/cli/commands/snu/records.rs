use super::*;

/// Read a JSON object payload from `--data` or, when absent, from stdin.
pub(super) fn resolve_record_data(data: Option<String>) -> anyhow::Result<Map<String, Value>> {
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

pub(super) async fn handle_app_meta(
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

pub(super) async fn handle_list_tables(
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

pub(super) async fn handle_get_record(
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
pub(super) fn resolve_update_fields(
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

pub(super) fn snu_fields_warnings(fields: &[String], persisted: &Value) -> Vec<Value> {
    fields
        .iter()
        .filter(|field| matches!(persisted.get(field.as_str()), Some(Value::Null) | None))
        .map(|field| json!({"field": field, "warning": "field missing or empty after await"}))
        .collect()
}

pub(super) fn parse_json_object(input: &str, label: &str) -> anyhow::Result<Map<String, Value>> {
    let value: Value = serde_json::from_str(input)
        .with_context(|| format!("failed to parse {label} as JSON object"))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => anyhow::bail!("{label} must be a JSON object"),
    }
}

pub(super) async fn snu_fetch_persisted_record(
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

pub(super) async fn fetch_record_by_sys_id(
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

pub(super) struct QueryRecordsRequest<'a> {
    pub(super) table: &'a str,
    pub(super) fields: &'a str,
    pub(super) limit: u32,
    pub(super) query: Option<&'a str>,
    pub(super) order_by: Option<&'a str>,
}

pub(super) async fn query_records_via_bridge(
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

pub(super) fn build_table_query_string(
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
