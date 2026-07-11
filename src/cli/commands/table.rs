use std::io::IsTerminal;

use crate::cli::args::{OutputFormat, TableArgs, TableCommands};
use crate::cli::output;
use crate::cli::truncation;
use crate::cli::validation::{
    DEFAULT_MAX_STDIN_BYTES, read_to_string_limited, validate_path_segment, validate_table_name,
};
use crate::client::pagination::PaginationConfig;
use crate::models::record::SingleRecordResponse;

pub async fn handle(
    args: TableArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    match args.command {
        TableCommands::List {
            table,
            query,
            fields,
            limit,
            all,
            order_by,
            full,
        } => {
            tracing::info!("Listing records from table: {}", table);
            validate_table_name(&table)?;

            let effective_limit = if all {
                None
            } else {
                Some(limit.unwrap_or(DEFAULT_LIST_LIMIT))
            };
            let effective_fields = resolve_list_fields(&table, fields.as_deref());

            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
            let pagination = PaginationConfig::default().with_limit(effective_limit);

            let mut result = client
                .get_table_records_with_meta(
                    &table,
                    query.as_deref(),
                    effective_fields.as_deref(),
                    &pagination,
                    order_by.as_deref(),
                )
                .await?;

            if !full {
                result.fields_truncated = truncation::truncate_record_fields(
                    &mut result.records,
                    truncation::DEFAULT_FIELD_CHAR_LIMIT,
                );
            }

            output::print_table_list(&result, format)?;
            Ok(())
        }

        TableCommands::Get {
            table,
            sys_id,
            fields,
            full,
        } => {
            tracing::info!("Getting record {} from table: {}", sys_id, table);
            validate_table_name(&table)?;
            validate_path_segment("sys_id", &sys_id)?;

            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

            let path = format!("/api/now/table/{table}/{sys_id}");
            let mut params = Vec::new();
            if let Some(ref f) = fields {
                params.push(("sysparm_fields", f.as_str()));
            }

            let mut response: SingleRecordResponse = if params.is_empty() {
                client.get_json(&path).await?
            } else {
                client.get_json_with_params(&path, &params).await?
            };

            if !full {
                truncation::truncate_record_fields(
                    std::slice::from_mut(&mut response.result),
                    truncation::DEFAULT_FIELD_CHAR_LIMIT,
                );
            }

            output::print_record(&response.result, format)?;
            Ok(())
        }

        TableCommands::Create { table, data } => {
            tracing::info!("Creating record in table: {}", table);
            validate_table_name(&table)?;

            let body = read_data(data)?;
            // Validate that the body is valid JSON
            let _: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| anyhow::anyhow!("Invalid JSON data: {e}"))?;

            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

            let path = format!("/api/now/table/{table}");
            let response: SingleRecordResponse = client.post_json(&path, &body).await?;

            output::print_record(&response.result, format)?;
            Ok(())
        }

        TableCommands::Update {
            table,
            sys_id,
            data,
        } => {
            tracing::info!("Updating record {} in table: {}", sys_id, table);
            validate_table_name(&table)?;
            validate_path_segment("sys_id", &sys_id)?;

            let body = read_data(data)?;
            // Validate that the body is valid JSON
            let _: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| anyhow::anyhow!("Invalid JSON data: {e}"))?;

            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

            let path = format!("/api/now/table/{table}/{sys_id}");
            let response: SingleRecordResponse = client.patch_json(&path, &body).await?;

            output::print_record(&response.result, format)?;
            Ok(())
        }

        TableCommands::Delete { table, sys_id, yes } => {
            tracing::info!("Deleting record {} from table: {}", sys_id, table);
            validate_table_name(&table)?;
            validate_path_segment("sys_id", &sys_id)?;

            if !yes {
                let stdin = std::io::stdin();
                if !stdin.is_terminal() {
                    anyhow::bail!(
                        "Delete requires confirmation. Re-run with --yes for non-interactive use: \
                         snow-cli table delete {table} {sys_id} --yes"
                    );
                }

                eprint!("Delete record {sys_id} from table {table}? [y/N] ");
                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer)?;
                if !answer.trim().eq_ignore_ascii_case("y") {
                    eprintln!("Aborted.");
                    return Ok(());
                }
            }

            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

            let path = format!("/api/now/table/{table}/{sys_id}");
            client.delete(&path).await?;

            output::print_status(&format!("Record {sys_id} deleted from {table}"), format)?;
            Ok(())
        }

        TableCommands::Schema {
            table,
            extended,
            include_inherited,
        } => {
            handle_schema(
                profile,
                format,
                instance,
                timeout_secs,
                &table,
                extended,
                include_inherited,
            )
            .await
        }
    }
}

/// Bounded default record count when `--limit` and `--all` are both omitted.
pub(crate) const DEFAULT_LIST_LIMIT: usize = 20;

/// Compact default field projection for `table list` when `--fields` is omitted.
///
/// ServiceNow silently drops field names a table does not have, so the
/// fallback set for unknown tables is safe: it only narrows the response
/// to whichever of these common identifying fields exist.
fn default_list_fields(table: &str) -> &'static str {
    match table {
        "task" | "incident" | "problem" | "change_request" | "change_task" | "sc_task"
        | "sc_req_item" | "sc_request" => {
            "sys_id,number,short_description,state,priority,assigned_to,sys_updated_on"
        }
        "sys_user" => "sys_id,user_name,name,email,active,sys_updated_on",
        "sys_user_group" => "sys_id,name,description,manager,active",
        "kb_knowledge" => "sys_id,number,short_description,workflow_state,sys_updated_on",
        t if t == "cmdb_ci" || t.starts_with("cmdb_ci_") => {
            "sys_id,name,sys_class_name,operational_status,sys_updated_on"
        }
        _ => "sys_id,number,name,short_description,state,sys_updated_on",
    }
}

/// Resolve the effective `sysparm_fields` for `table list`.
///
/// Caller-supplied `--fields` is authoritative; `"*"` requests every field
/// (no projection); omission falls back to the compact default set.
fn resolve_list_fields(table: &str, fields: Option<&str>) -> Option<String> {
    match fields {
        Some("*") => None,
        Some(f) => Some(f.to_string()),
        None => Some(default_list_fields(table).to_string()),
    }
}

/// `table schema` — Fetch column metadata from sys_dictionary.
///
/// Queries sys_dictionary for the given table to retrieve column names, types,
/// and labels. With `--extended`, also shows required, read-only, max_length,
/// default_value, and reference table. With `--include-inherited`, queries
/// parent tables as well (e.g., `incident` extends `task`).
async fn handle_schema(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    table: &str,
    extended: bool,
    include_inherited: bool,
) -> anyhow::Result<()> {
    tracing::info!("Fetching schema for table: {}", table);
    validate_table_name(table)?;

    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

    // Build the base query: get columns for this table (exclude table-level metadata rows)
    let query = if include_inherited {
        // Use INSTANCEOF to get the table and all parent tables
        format!("nameINSTANCEOF{table}^elementISNOTEMPTY^element!=sys_tags")
    } else {
        format!("name={table}^elementISNOTEMPTY^element!=sys_tags")
    };

    // Select fields based on compact vs extended mode
    let fields = if extended {
        "element,internal_type,column_label,mandatory,read_only,display,max_length,default_value,reference,name"
    } else {
        "element,internal_type,column_label,name"
    };

    let pagination = crate::client::pagination::PaginationConfig::default()
        .with_page_size(500)
        .with_limit(None);

    let records = client
        .get_table_records(
            "sys_dictionary",
            Some(&query),
            Some(fields),
            &pagination,
            Some("name,element"),
        )
        .await?;

    if records.is_empty() {
        output::print_status(
            &format!("No columns found for table '{table}'. Verify the table name."),
            format,
        )?;
        return Ok(());
    }

    // Build schema entries from the dictionary records
    let entries: Vec<SchemaEntry> = records
        .iter()
        .map(|r| SchemaEntry {
            column: r.get_str("element").unwrap_or("").to_string(),
            r#type: field_value_as_text(r, "internal_type").unwrap_or_default(),
            label: r.get_str("column_label").unwrap_or("").to_string(),
            table: if include_inherited {
                Some(r.get_str("name").unwrap_or("").to_string())
            } else {
                None
            },
            required: if extended {
                Some(r.get_str("mandatory").unwrap_or("false") == "true")
            } else {
                None
            },
            read_only: if extended {
                Some(r.get_str("read_only").unwrap_or("false") == "true")
            } else {
                None
            },
            display: if extended {
                Some(r.get_str("display").unwrap_or("false") == "true")
            } else {
                None
            },
            max_length: if extended {
                r.get_str("max_length").map(|s| s.to_string())
            } else {
                None
            },
            default_value: if extended {
                r.get_str("default_value").map(|s| s.to_string())
            } else {
                None
            },
            reference: if extended {
                let val = field_value_as_text(r, "reference").unwrap_or_default();
                if val.is_empty() { None } else { Some(val) }
            } else {
                None
            },
        })
        .collect();

    print_schema(&entries, format)?;
    Ok(())
}

/// A single column's schema metadata.
#[derive(Debug, serde::Serialize)]
struct SchemaEntry {
    column: String,
    r#type: String,
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    table: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    read_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_length: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
}

/// Print schema entries in the requested format.
fn print_schema(entries: &[SchemaEntry], format: &OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string(entries)?;
            println!("{json}");
        }
        OutputFormat::Csv => {
            let mut writer = csv::Writer::from_writer(std::io::stdout());
            for entry in entries {
                writer.serialize(entry)?;
            }
            writer.flush()?;
        }
        OutputFormat::Jsonl | OutputFormat::Toon | OutputFormat::Auto => {
            output::print_list(entries, format)?
        }
        OutputFormat::Text => {
            let json = serde_json::to_string_pretty(entries)?;
            println!("{json}");
        }
    }
    Ok(())
}

fn field_value_as_text(record: &crate::models::record::Record, field: &str) -> Option<String> {
    match record.fields.get(field) {
        Some(serde_json::Value::String(text)) => Some(text.clone()),
        Some(serde_json::Value::Object(map)) => map
            .get("value")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned),
        _ => None,
    }
}

/// Read JSON data from `--data` flag or stdin.
///
/// If `data` is `Some`, use it directly.
/// If `None`, read from stdin (but only if stdin is not a TTY,
/// to avoid hanging waiting for interactive input).
fn read_data(data: Option<String>) -> anyhow::Result<String> {
    let stdin = std::io::stdin();
    read_data_from(data, stdin.lock(), stdin.is_terminal())
}

/// Internal implementation that accepts a generic reader and TTY flag.
///
/// This enables testing without relying on actual stdin behavior.
fn read_data_from<R: std::io::Read>(
    data: Option<String>,
    reader: R,
    is_tty: bool,
) -> anyhow::Result<String> {
    if let Some(d) = data {
        return Ok(d);
    }

    if is_tty {
        anyhow::bail!(
            "No data provided. Use --data or pipe JSON. Examples: \
             snow-cli table create incident --data '{{\"short_description\":\"Disk alert\"}}' \
             | echo '{{\"short_description\":\"Disk alert\"}}' | snow-cli table create incident"
        );
    }

    let buf = read_to_string_limited(reader, DEFAULT_MAX_STDIN_BYTES, "JSON stdin input")?;

    if buf.trim().is_empty() {
        anyhow::bail!(
            "No data received from stdin. Pipe valid JSON, for example: \
             echo '{{\"short_description\":\"Disk alert\"}}' | snow-cli table create incident"
        );
    }

    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_read_data_from_flag() {
        let data = read_data_from(
            Some(r#"{"key":"value"}"#.to_string()),
            Cursor::new(b""),
            false,
        )
        .unwrap();
        assert_eq!(data, r#"{"key":"value"}"#);
    }

    #[test]
    fn test_read_data_from_flag_ignores_stdin() {
        // Even when stdin has data, --data flag takes precedence
        let data = read_data_from(
            Some(r#"{"flag":"data"}"#.to_string()),
            Cursor::new(b"stdin data"),
            false,
        )
        .unwrap();
        assert_eq!(data, r#"{"flag":"data"}"#);
    }

    #[test]
    fn test_read_data_from_stdin_pipe() {
        let input = r#"{"piped":"data"}"#;
        let data = read_data_from(None, Cursor::new(input.as_bytes()), false).unwrap();
        assert_eq!(data, r#"{"piped":"data"}"#);
    }

    #[test]
    fn test_read_data_from_tty_no_data_errors() {
        let result = read_data_from(None, Cursor::new(b""), true);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No data provided"));
    }

    #[test]
    fn test_read_data_from_empty_stdin_errors() {
        let result = read_data_from(None, Cursor::new(b""), false);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No data received from stdin")
        );
    }

    #[test]
    fn test_read_data_from_whitespace_stdin_errors() {
        let result = read_data_from(None, Cursor::new(b"   \n  \t  "), false);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No data received from stdin")
        );
    }

    #[test]
    fn test_read_data_from_stdin_multiline() {
        let input = "{\n  \"key\": \"value\"\n}";
        let data = read_data_from(None, Cursor::new(input.as_bytes()), false).unwrap();
        assert_eq!(data, input);
    }

    #[test]
    fn test_resolve_list_fields_explicit_fields_are_authoritative() {
        assert_eq!(
            resolve_list_fields("incident", Some("sys_id,number")),
            Some("sys_id,number".to_string())
        );
    }

    #[test]
    fn test_resolve_list_fields_star_requests_all_fields() {
        assert_eq!(resolve_list_fields("incident", Some("*")), None);
    }

    #[test]
    fn test_resolve_list_fields_defaults_to_compact_projection() {
        let fields = resolve_list_fields("incident", None).unwrap();
        assert!(fields.contains("sys_id"));
        assert!(fields.contains("number"));
        assert!(fields.contains("short_description"));
        // Compact: a handful of columns, not the whole record
        assert!(fields.split(',').count() <= 8);
    }

    #[test]
    fn test_default_list_fields_table_aware() {
        assert!(default_list_fields("sys_user").contains("user_name"));
        assert!(default_list_fields("cmdb_ci_server").contains("sys_class_name"));
        assert!(default_list_fields("change_request").contains("number"));
        // Unknown tables get the conservative fallback
        assert!(default_list_fields("x_custom_table").contains("sys_id"));
        assert!(default_list_fields("x_custom_table").contains("sys_updated_on"));
    }

    #[test]
    fn test_field_value_as_text_supports_string_and_link_object() {
        let string_record = crate::models::record::Record {
            fields: std::collections::HashMap::from([(
                "internal_type".to_string(),
                serde_json::json!("string"),
            )]),
        };
        assert_eq!(
            field_value_as_text(&string_record, "internal_type"),
            Some("string".to_string())
        );

        let object_record = crate::models::record::Record {
            fields: std::collections::HashMap::from([(
                "internal_type".to_string(),
                serde_json::json!({
                    "link": "https://example.com/api/now/table/sys_glide_object?name=reference",
                    "value": "reference"
                }),
            )]),
        };
        assert_eq!(
            field_value_as_text(&object_record, "internal_type"),
            Some("reference".to_string())
        );
    }
}
