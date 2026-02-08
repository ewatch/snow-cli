use std::io::IsTerminal;

use crate::cli::args::{OutputFormat, TableArgs, TableCommands};
use crate::cli::output;
use crate::client::pagination::PaginationConfig;
use crate::models::record::SingleRecordResponse;

pub async fn handle(
    args: TableArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
) -> anyhow::Result<()> {
    match args.command {
        TableCommands::List {
            table,
            query,
            fields,
            limit,
            order_by,
        } => {
            tracing::info!("Listing records from table: {}", table);

            let mut client = crate::client::build_client(profile, instance)?;
            let pagination = PaginationConfig::default().with_limit(limit);

            let records = client
                .get_table_records(
                    &table,
                    query.as_deref(),
                    fields.as_deref(),
                    &pagination,
                    order_by.as_deref(),
                )
                .await?;

            output::print_records(&records, format)?;
            Ok(())
        }

        TableCommands::Get {
            table,
            sys_id,
            fields,
        } => {
            tracing::info!("Getting record {} from table: {}", sys_id, table);

            let mut client = crate::client::build_client(profile, instance)?;

            let path = format!("/api/now/table/{table}/{sys_id}");
            let mut params = Vec::new();
            if let Some(ref f) = fields {
                params.push(("sysparm_fields", f.as_str()));
            }

            let response: SingleRecordResponse = if params.is_empty() {
                client.get_json(&path).await?
            } else {
                client.get_json_with_params(&path, &params).await?
            };

            output::print_record(&response.result, format)?;
            Ok(())
        }

        TableCommands::Create { table, data } => {
            tracing::info!("Creating record in table: {}", table);

            let body = read_data(data)?;
            // Validate that the body is valid JSON
            let _: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| anyhow::anyhow!("Invalid JSON data: {e}"))?;

            let mut client = crate::client::build_client(profile, instance)?;

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

            let body = read_data(data)?;
            // Validate that the body is valid JSON
            let _: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| anyhow::anyhow!("Invalid JSON data: {e}"))?;

            let mut client = crate::client::build_client(profile, instance)?;

            let path = format!("/api/now/table/{table}/{sys_id}");
            let response: SingleRecordResponse = client.patch_json(&path, &body).await?;

            output::print_record(&response.result, format)?;
            Ok(())
        }

        TableCommands::Delete { table, sys_id, yes } => {
            tracing::info!("Deleting record {} from table: {}", sys_id, table);

            if !yes {
                let stdin = std::io::stdin();
                if !stdin.is_terminal() {
                    anyhow::bail!(
                        "Delete requires confirmation. Use --yes to skip, or run interactively."
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

            let mut client = crate::client::build_client(profile, instance)?;

            let path = format!("/api/now/table/{table}/{sys_id}");
            client.delete(&path).await?;

            output::print_status(&format!("Record {sys_id} deleted from {table}"), format)?;
            Ok(())
        }
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
    mut reader: R,
    is_tty: bool,
) -> anyhow::Result<String> {
    if let Some(d) = data {
        return Ok(d);
    }

    if is_tty {
        anyhow::bail!("No data provided. Use --data '<json>' or pipe JSON to stdin.");
    }

    let mut buf = String::new();
    reader.read_to_string(&mut buf)?;

    if buf.trim().is_empty() {
        anyhow::bail!("No data received from stdin.");
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
}
