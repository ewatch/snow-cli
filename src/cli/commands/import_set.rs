use std::io::IsTerminal;

use crate::cli::args::{ImportSetArgs, OutputFormat};
use crate::cli::io::{DEFAULT_MAX_STDIN_BYTES, read_to_string_limited};

#[derive(Debug, serde::Deserialize)]
struct ImportSetLoadResponse {
    #[serde(default)]
    import_set: Option<String>,
    #[serde(default)]
    staging_table: Option<String>,
    #[serde(default)]
    result: Vec<ImportSetResultEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ImportSetResultEntry {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    status_message: Option<String>,
    #[serde(default)]
    error_message: Option<String>,
    #[serde(flatten)]
    extra: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, serde::Serialize)]
struct ImportSetLoadOutput {
    command: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    import_set: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    staging_table: Option<String>,
    summary: ImportSetLoadSummary,
    result: Vec<ImportSetResultEntry>,
}

#[derive(Debug, Default, serde::Serialize)]
struct ImportSetLoadSummary {
    total: usize,
    inserted: usize,
    updated: usize,
    ignored: usize,
    error: usize,
    other: usize,
}

pub async fn handle(
    args: ImportSetArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    match args.command {
        crate::cli::args::ImportSetCommands::Load {
            table,
            data,
            fail_on_error,
        } => {
            tracing::info!("Loading data into staging table: {}", table);

            let body = read_data(data)?;
            let _: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| anyhow::anyhow!("Invalid JSON data: {e}"))?;

            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
            let path = format!("/api/now/import/{table}");
            let response: ImportSetLoadResponse = client.post_json(&path, &body).await?;
            let summary = summarize_results(&response.result);
            let output = ImportSetLoadOutput {
                command: "import-set load",
                import_set: response.import_set,
                staging_table: response.staging_table,
                summary,
                result: response.result,
            };

            crate::cli::output::print_output(&output, format)?;

            if fail_on_error && output.summary.error > 0 {
                anyhow::bail!(
                    "Import set load completed with {} row-level error(s). Re-run without --fail-on-error to inspect the structured response without failing the command.",
                    output.summary.error
                );
            }

            Ok(())
        }
        crate::cli::args::ImportSetCommands::Transform { sys_id } => {
            tracing::info!("Transforming import set: {}", sys_id);
            anyhow::bail!(
                "`import-set transform` is not implemented yet for import set '{}'. Live validation on the `sprint` instance showed that POST /api/now/import/{{table}} already ran the transform automatically, and a separate supported REST transform trigger has not been wired into the CLI yet.",
                sys_id
            )
        }
    }
}

fn summarize_results(entries: &[ImportSetResultEntry]) -> ImportSetLoadSummary {
    let mut summary = ImportSetLoadSummary {
        total: entries.len(),
        ..ImportSetLoadSummary::default()
    };

    for entry in entries {
        match entry
            .status
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("inserted") => summary.inserted += 1,
            Some("updated") => summary.updated += 1,
            Some("ignored") => summary.ignored += 1,
            Some("error") => summary.error += 1,
            Some(_) | None => summary.other += 1,
        }
    }

    summary
}

fn read_data(data: Option<String>) -> anyhow::Result<String> {
    let stdin = std::io::stdin();
    read_data_from(data, stdin.lock(), stdin.is_terminal())
}

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
             snow-cli import-set load u_import_table --data '{{\"name\":\"x\"}}' \
             | echo '{{\"name\":\"x\"}}' | snow-cli import-set load u_import_table"
        );
    }

    let buf = read_to_string_limited(reader, DEFAULT_MAX_STDIN_BYTES, "import-set stdin input")?;

    if buf.trim().is_empty() {
        anyhow::bail!(
            "No data received from stdin. Pipe valid JSON, for example: \
             echo '{{\"name\":\"x\"}}' | snow-cli import-set load u_import_table"
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
        let data =
            read_data_from(Some(r#"{"name":"x"}"#.to_string()), Cursor::new(b""), false).unwrap();
        assert_eq!(data, r#"{"name":"x"}"#);
    }

    #[test]
    fn test_read_data_from_stdin() {
        let data = read_data_from(None, Cursor::new(br#"{"name":"stdin"}"#), false).unwrap();
        assert_eq!(data, r#"{"name":"stdin"}"#);
    }

    #[test]
    fn test_read_data_from_tty_errors() {
        let result = read_data_from(None, Cursor::new(b""), true);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No data provided"));
    }

    #[test]
    fn test_summarize_results_counts_known_statuses() {
        let entries = vec![
            ImportSetResultEntry {
                status: Some("inserted".to_string()),
                status_message: None,
                error_message: None,
                extra: Default::default(),
            },
            ImportSetResultEntry {
                status: Some("updated".to_string()),
                status_message: None,
                error_message: None,
                extra: Default::default(),
            },
            ImportSetResultEntry {
                status: Some("ignored".to_string()),
                status_message: None,
                error_message: None,
                extra: Default::default(),
            },
            ImportSetResultEntry {
                status: Some("error".to_string()),
                status_message: None,
                error_message: Some("bad row".to_string()),
                extra: Default::default(),
            },
            ImportSetResultEntry {
                status: Some("skipped".to_string()),
                status_message: None,
                error_message: None,
                extra: Default::default(),
            },
        ];

        let summary = summarize_results(&entries);
        assert_eq!(summary.total, 5);
        assert_eq!(summary.inserted, 1);
        assert_eq!(summary.updated, 1);
        assert_eq!(summary.ignored, 1);
        assert_eq!(summary.error, 1);
        assert_eq!(summary.other, 1);
    }
}
