use crate::cli::args::{CodesearchArgs, CodesearchCommands, OutputFormat};
use crate::cli::output;
use crate::cli::truncation::{self, PreviewReport};

pub async fn handle(
    args: CodesearchArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    match args.command {
        CodesearchCommands::Search {
            query,
            source_table,
            scope,
            limit,
            current_scope,
            search_group,
            full,
        } => {
            tracing::info!(query = %query, "Searching code");

            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

            let limit_str = limit.to_string();
            let search_all_scopes = !current_scope && scope.is_none();
            let search_all_scopes_str = search_all_scopes.to_string();

            let mut params: Vec<(&str, &str)> = vec![
                ("term", &query),
                ("limit", &limit_str),
                ("search_all_scopes", &search_all_scopes_str),
                ("search_group", &search_group),
            ];

            if let Some(ref t) = source_table {
                params.push(("table", t.as_str()));
            }

            if let Some(ref s) = scope {
                params.push(("current_app", s.as_str()));
            }

            let response = client
                .get_with_params("/api/sn_codesearch/code_search/search", &params)
                .await?;

            let response_body = response.text().await?;

            tracing::debug!(body_len = response_body.len(), "Code search response");

            render_response(&response_body, format, full)?;

            Ok(())
        }
    }
}

fn render_response(body: &str, format: &OutputFormat, full: bool) -> anyhow::Result<()> {
    let Ok(mut json) = serde_json::from_str::<serde_json::Value>(body) else {
        return output::print_output(&body, format);
    };

    if let Some(groups) = json
        .get_mut("result")
        .and_then(serde_json::Value::as_array_mut)
        && !groups.is_empty()
        && groups.iter().all(|group| {
            group
                .get("hits")
                .and_then(serde_json::Value::as_array)
                .is_some()
        })
    {
        let searched_tables = groups.len();
        groups.retain(|group| {
            group
                .get("hits")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|hits| !hits.is_empty())
        });
        if matches!(format, OutputFormat::Csv) {
            return output::print_records(&flatten_group_hits(groups), format);
        }
        let preview = preview_codesearch_output(&mut json, format, full);
        if let Some(object) = json.as_object_mut() {
            object.insert("searched_tables".to_string(), searched_tables.into());
            insert_preview(object, &preview)?;
        }
        return output::print_output(&json, format);
    }

    let preview = preview_codesearch_output(&mut json, format, full);

    if let Ok(table_resp) =
        serde_json::from_value::<crate::models::record::TableResponse>(json.clone())
    {
        if preview.is_empty() {
            output::print_records(&table_resp.result, format)
        } else {
            if let Some(object) = json.as_object_mut() {
                insert_preview(object, &preview)?;
            }
            output::print_output(&json, format)
        }
    } else {
        if let Some(object) = json.as_object_mut() {
            insert_preview(object, &preview)?;
        }
        output::print_output(&json, format)
    }
}

fn preview_codesearch_output(
    json: &mut serde_json::Value,
    format: &OutputFormat,
    full: bool,
) -> PreviewReport {
    if !full && supports_preview_metadata(format) {
        truncation::preview_codesearch_fields(
            json,
            truncation::DEFAULT_FIELD_CHAR_LIMIT,
            "rerun this command with --full",
        )
    } else {
        PreviewReport::default()
    }
}

fn supports_preview_metadata(format: &OutputFormat) -> bool {
    !matches!(format, OutputFormat::Csv)
}

fn insert_preview(
    object: &mut serde_json::Map<String, serde_json::Value>,
    preview: &PreviewReport,
) -> anyhow::Result<()> {
    if !preview.is_empty() {
        object.insert("preview".to_string(), serde_json::to_value(preview)?);
    }
    Ok(())
}

fn flatten_group_hits(groups: &[serde_json::Value]) -> Vec<crate::models::record::Record> {
    groups
        .iter()
        .flat_map(|group| {
            let record_type = group.get("recordType").cloned();
            group
                .get("hits")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .map(move |hit| {
                    let mut fields = hit.as_object().cloned().unwrap_or_else(|| {
                        serde_json::Map::from_iter([("value".to_string(), hit.clone())])
                    });
                    if let Some(record_type) = &record_type {
                        fields.insert("recordType".to_string(), record_type.clone());
                    }
                    crate::models::record::Record {
                        fields: fields.into_iter().collect(),
                    }
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codesearch_args_construction() {
        let args = CodesearchArgs {
            command: CodesearchCommands::Search {
                query: "GlideRecord".to_string(),
                source_table: Some("sys_script_include".to_string()),
                scope: Some("x_my_app".to_string()),
                limit: 500,
                current_scope: false,
                search_group: "sn_devstudio.Studio Search Group".to_string(),
                full: false,
            },
        };
        match args.command {
            CodesearchCommands::Search {
                query,
                source_table,
                scope,
                limit,
                current_scope,
                search_group,
                full,
            } => {
                assert_eq!(query, "GlideRecord");
                assert_eq!(source_table, Some("sys_script_include".to_string()));
                assert_eq!(scope, Some("x_my_app".to_string()));
                assert_eq!(limit, 500);
                assert!(!current_scope);
                assert_eq!(search_group, "sn_devstudio.Studio Search Group");
                assert!(!full);
            }
        }
    }
}
