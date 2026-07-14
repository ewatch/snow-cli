use super::*;

pub(super) struct DeleteRecordRequest {
    pub(super) table: String,
    pub(super) sys_id: Option<String>,
    pub(super) query: Option<String>,
    pub(super) confirm: bool,
    pub(super) limit: Option<u32>,
    pub(super) dry_run: bool,
}

pub(super) async fn handle_delete_record(
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
/// Marker prefix the generated mutation scripts print in front of their JSON
/// result line, so we can pick it out of the background-script output regardless
/// of any other logging the server emits around it.
pub(super) const MUTATION_RESULT_MARKER: &str = "__SNOW_CLI_RESULT__:";

/// Serialize a JSON value into a form that is safe to embed as a literal inside
/// generated JavaScript. `serde_json` already escapes quotes, backslashes and
/// control characters; we additionally escape U+2028/U+2029, which are valid in
/// JSON strings but are line terminators in JavaScript and would otherwise break
/// a string literal in the Rhino engine ServiceNow runs.
pub(super) fn js_json_literal(value: &Value) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "null".to_string())
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// Build a server-side background script that updates a single record via
/// `GlideRecord` and prints a machine-parseable JSON result. User-supplied
/// values are embedded as a JSON literal (never string-concatenated) so quotes,
/// backslashes, newlines and unicode cannot break out of the script.
pub(super) fn build_update_script(
    table: &str,
    sys_id: &str,
    fields: &Map<String, Value>,
) -> String {
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
pub(super) fn build_delete_script(table: &str, sys_id: &str) -> String {
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
pub(super) fn build_bulk_delete_script(table: &str, query: &str, limit: u32) -> String {
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
pub(super) async fn run_bg_mutation(
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
pub(super) fn parse_mutation_result(data: &str) -> anyhow::Result<Value> {
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
pub(super) fn first_json_value(text: &str) -> anyhow::Result<Value> {
    let mut stream = serde_json::Deserializer::from_str(text).into_iter::<Value>();
    match stream.next() {
        Some(Ok(value)) => Ok(value),
        Some(Err(error)) => Err(error.into()),
        None => Err(anyhow!("empty response")),
    }
}

/// Minimal decode for the HTML entities `sys.scripts.do` escapes in script
/// output. `&amp;` is decoded last so double-escaped input cannot re-expand.
pub(super) fn decode_html_entities(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Heuristic for a ServiceNow login page returned where script output was
/// expected — the signature of a browser session ServiceNow no longer accepts.
pub(super) fn looks_like_login_page(text: &str) -> bool {
    let lower = text.to_lowercase();
    (lower.contains("<html") || lower.contains("<!doctype"))
        && (lower.contains("login")
            || lower.contains("logged out")
            || lower.contains("not authenticated")
            || lower.contains("user_name"))
}

pub(super) fn correlation_id(prefix: &str) -> String {
    format!("snow_{prefix}_{}", uuid::Uuid::new_v4().simple())
}
