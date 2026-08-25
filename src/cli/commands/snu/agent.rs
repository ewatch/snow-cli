use super::*;
use crate::cli::args::{
    SnuPageCommands, SnuPageTargetArgs, SnuRecordsCommands, SnuRestCommands, SnuRestReadArgs,
    SnuRestWriteArgs,
};

pub(super) async fn send_agent_action(
    bridge: &BrokerBridge,
    action: &str,
    mut fields: Map<String, Value>,
    timeout_secs: u64,
) -> anyhow::Result<SnuMessage> {
    let correlation_id = correlation_id(action);
    fields.insert("action".to_string(), Value::String(action.to_string()));
    fields.insert(
        "agentRequestId".to_string(),
        Value::String(correlation_id.clone()),
    );
    fields.insert("appName".to_string(), Value::String("snow-cli".to_string()));
    bridge
        .send_action_and_wait(&Value::Object(fields), &correlation_id, timeout_secs)
        .await
}

struct RestRequestParts {
    method: &'static str,
    endpoint: String,
    query_params: Vec<String>,
    data: Option<Value>,
    timeout_secs: u64,
}

pub(super) async fn handle_rest(
    command: SnuRestCommands,
    target_origin: Option<String>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let request = match command {
        SnuRestCommands::Get(request) => rest_read_parts("GET", request),
        SnuRestCommands::Delete(request) => rest_read_parts("DELETE", request),
        SnuRestCommands::Post(request) => rest_write_parts("POST", request)?,
        SnuRestCommands::Put(request) => rest_write_parts("PUT", request)?,
        SnuRestCommands::Patch(request) => rest_write_parts("PATCH", request)?,
    };
    validate_rest_endpoint(&request.endpoint)?;
    let query_params = parse_query_params(&request.query_params)?;
    let (bridge, instance) =
        connect_and_wait_for_session(request.timeout_secs, target_origin).await?;
    let mut fields = Map::new();
    fields.insert("instance".to_string(), serde_json::to_value(instance)?);
    fields.insert("endpoint".to_string(), Value::String(request.endpoint));
    fields.insert(
        "method".to_string(),
        Value::String(request.method.to_string()),
    );
    fields.insert("queryParams".to_string(), Value::Object(query_params));
    if let Some(data) = request.data {
        fields.insert("body".to_string(), data);
    }
    let response = send_agent_action(&bridge, "agentRestApi", fields, request.timeout_secs).await?;
    print_response_value(response, output_format)
}

fn rest_read_parts(method: &'static str, request: SnuRestReadArgs) -> RestRequestParts {
    RestRequestParts {
        method,
        endpoint: request.endpoint,
        query_params: request.query_params,
        data: None,
        timeout_secs: request.timeout_secs,
    }
}

fn rest_write_parts(
    method: &'static str,
    request: SnuRestWriteArgs,
) -> anyhow::Result<RestRequestParts> {
    let data = request
        .data
        .map(|raw| serde_json::from_str(&raw).context("failed to parse --data as JSON"))
        .transpose()?;
    Ok(RestRequestParts {
        method,
        endpoint: request.endpoint,
        query_params: request.query_params,
        data,
        timeout_secs: request.timeout_secs,
    })
}

fn validate_rest_endpoint(endpoint: &str) -> anyhow::Result<()> {
    if !endpoint.starts_with('/') || endpoint.starts_with("//") {
        anyhow::bail!("REST endpoint must be an instance-relative path beginning with one '/'");
    }
    Ok(())
}

fn parse_query_params(params: &[String]) -> anyhow::Result<Map<String, Value>> {
    let mut result = Map::new();
    for param in params {
        let (key, value) = param
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid --query-param '{param}': expected key=value"))?;
        if key.is_empty() {
            anyhow::bail!("invalid --query-param '{param}': key cannot be empty");
        }
        result.insert(key.to_string(), Value::String(value.to_string()));
    }
    Ok(result)
}

pub(super) async fn handle_page(
    command: SnuPageCommands,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let (action, fields, timeout_secs) = match command {
        SnuPageCommands::FormState { fields, target } => {
            let mut payload = page_target_fields(&target);
            if !fields.is_empty() {
                payload.insert(
                    "fields".to_string(),
                    Value::Array(fields.into_iter().map(Value::String).collect()),
                );
            }
            ("agentGetFormState", payload, target.timeout_secs)
        }
        SnuPageCommands::SetField {
            field,
            value,
            display_value,
            target,
        } => {
            let mut payload = page_target_fields(&target);
            payload.insert("field".to_string(), Value::String(field));
            payload.insert("value".to_string(), Value::String(value));
            if let Some(display_value) = display_value {
                payload.insert("displayValue".to_string(), Value::String(display_value));
            }
            ("agentSetField", payload, target.timeout_secs)
        }
        SnuPageCommands::RunUiAction {
            action_name,
            no_suppress_dialogs,
            target,
        } => {
            let mut payload = page_target_fields(&target);
            payload.insert("uiAction".to_string(), Value::String(action_name));
            payload.insert(
                "suppressDialogs".to_string(),
                Value::Bool(!no_suppress_dialogs),
            );
            ("agentRunUiAction", payload, target.timeout_secs)
        }
        SnuPageCommands::Click {
            selector,
            no_suppress_dialogs,
            target,
        } => {
            let mut payload = page_target_fields(&target);
            payload.insert("selector".to_string(), Value::String(selector));
            payload.insert(
                "suppressDialogs".to_string(),
                Value::Bool(!no_suppress_dialogs),
            );
            ("agentClickElement", payload, target.timeout_secs)
        }
        SnuPageCommands::Navigate {
            url,
            tab_id,
            new_tab,
            find_url,
            no_wait_for_load,
            keep_unsaved_guard,
            timeout_secs,
        } => {
            let mut payload = Map::new();
            payload.insert("url".to_string(), Value::String(url));
            if let Some(tab_id) = tab_id {
                payload.insert("tabId".to_string(), Value::Number(tab_id.into()));
            }
            payload.insert("newTab".to_string(), Value::Bool(new_tab));
            payload.insert("findUrl".to_string(), Value::String(find_url));
            payload.insert("waitForLoad".to_string(), Value::Bool(!no_wait_for_load));
            payload.insert(
                "discardUnsaved".to_string(),
                Value::Bool(!keep_unsaved_guard),
            );
            ("agentNavigate", payload, timeout_secs)
        }
    };
    let bridge = connect_bridge(timeout_secs, None).await?;
    let response = send_agent_action(&bridge, action, fields, timeout_secs).await?;
    print_response_value(response, output_format)
}

fn page_target_fields(target: &SnuPageTargetArgs) -> Map<String, Value> {
    let mut fields = Map::new();
    fields.insert("url".to_string(), Value::String(target.url.clone()));
    if let Some(tab_id) = target.tab_id {
        fields.insert("tabId".to_string(), Value::Number(tab_id.into()));
    }
    fields
}

pub(super) async fn handle_records_agent(
    command: SnuRecordsCommands,
    target_origin: Option<String>,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    match command {
        SnuRecordsCommands::ParentOptions {
            table,
            query,
            fields,
            name_field,
            limit,
            timeout_secs,
        } => {
            let (bridge, instance) =
                connect_and_wait_for_session(timeout_secs, target_origin).await?;
            let query_string = build_table_query_string(&fields, limit, query.as_deref(), None);
            let mut payload = Map::new();
            payload.insert("instance".to_string(), serde_json::to_value(instance)?);
            payload.insert("tableName".to_string(), Value::String(table));
            payload.insert("queryString".to_string(), Value::String(query_string));
            payload.insert("nameField".to_string(), Value::String(name_field));
            let response =
                send_agent_action(&bridge, "agentGetParentOptions", payload, timeout_secs).await?;
            print_response_value(response, output_format)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_params_require_key_value_and_preserve_equals_in_values() {
        let parsed = parse_query_params(&[
            "sysparm_query=active=true".to_string(),
            "sysparm_limit=10".to_string(),
        ])
        .unwrap();
        assert_eq!(parsed["sysparm_query"], "active=true");
        assert!(parse_query_params(&["invalid".to_string()]).is_err());
    }

    #[test]
    fn rest_endpoint_must_be_instance_relative() {
        assert!(validate_rest_endpoint("/api/now/table/incident").is_ok());
        assert!(validate_rest_endpoint("https://evil.example/api").is_err());
        assert!(validate_rest_endpoint("//evil.example/api").is_err());
    }
}
