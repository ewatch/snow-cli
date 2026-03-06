use std::fs::File;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::cli::args::{DataArgs, DataCommands, OutputFormat};
use crate::cli::output;
use crate::client::pagination::PaginationConfig;
use crate::models::record::Record;

pub async fn handle(
    args: DataArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
) -> anyhow::Result<()> {
    match args.command {
        DataCommands::Export {
            table,
            query,
            fields,
            limit,
            order_by,
            out_path,
        } => {
            let export = ExportRequest {
                table,
                query,
                fields,
                limit,
                order_by,
                out_path,
            };
            handle_export(profile, format, instance, export).await
        }
        DataCommands::Validate { .. } => {
            anyhow::bail!("`data validate` is planned but not implemented yet")
        }
        DataCommands::Import { .. } => {
            anyhow::bail!("`data import` is planned but not implemented yet")
        }
    }
}

#[derive(Debug, Serialize)]
struct TableExportArtifact {
    version: u8,
    kind: &'static str,
    command: &'static str,
    instance: String,
    table: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fields: Option<Vec<String>>,
    exported_at_unix_s: u64,
    record_count: usize,
    records: Vec<Record>,
}

#[derive(Debug, Serialize)]
struct ExportSummary {
    kind: &'static str,
    command: &'static str,
    output_format: &'static str,
    instance: String,
    table: String,
    record_count: usize,
    out_path: String,
}

#[derive(Debug)]
struct ExportRequest {
    table: String,
    query: Option<String>,
    fields: Option<String>,
    limit: Option<usize>,
    order_by: Option<String>,
    out_path: Option<String>,
}

async fn handle_export(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    export: ExportRequest,
) -> anyhow::Result<()> {
    tracing::info!("Exporting records from table: {}", export.table);

    let mut client = crate::client::build_client(profile, instance)?;
    let pagination = PaginationConfig::default().with_limit(export.limit);

    let records = client
        .get_table_records(
            &export.table,
            export.query.as_deref(),
            export.fields.as_deref(),
            &pagination,
            export.order_by.as_deref(),
        )
        .await?;

    let artifact = TableExportArtifact {
        version: 1,
        kind: "table-export",
        command: "data export",
        instance: client.base_url().to_string(),
        table: export.table,
        query: export.query,
        fields: split_csv_fields(export.fields.as_deref()),
        exported_at_unix_s: current_unix_timestamp(),
        record_count: records.len(),
        records,
    };

    if let Some(out_path) = export.out_path {
        write_export_file(&artifact, format, &out_path)?;

        let summary = ExportSummary {
            kind: "export-result",
            command: "data export",
            output_format: output_format_name(format),
            instance: artifact.instance,
            table: artifact.table,
            record_count: artifact.record_count,
            out_path,
        };
        return output::print_output(&summary, format);
    }

    match format {
        OutputFormat::Json => output::print_output(&artifact, format),
        OutputFormat::Csv => output::print_records(&artifact.records, format),
    }
}

fn write_export_file(
    artifact: &TableExportArtifact,
    format: &OutputFormat,
    out_path: &str,
) -> anyhow::Result<()> {
    let mut file = File::create(out_path)?;
    match format {
        OutputFormat::Json => {
            serde_json::to_writer(&mut file, artifact)?;
            file.write_all(b"\n")?;
        }
        OutputFormat::Csv => output::write_records_csv(&artifact.records, &mut file)?,
    }

    Ok(())
}

fn split_csv_fields(fields: Option<&str>) -> Option<Vec<String>> {
    let fields = fields?
        .split(',')
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if fields.is_empty() {
        None
    } else {
        Some(fields)
    }
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn output_format_name(format: &OutputFormat) -> &'static str {
    match format {
        OutputFormat::Json => "json",
        OutputFormat::Csv => "csv",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_csv_fields_none() {
        assert_eq!(split_csv_fields(None), None);
    }

    #[test]
    fn test_split_csv_fields_trims_values() {
        assert_eq!(
            split_csv_fields(Some("sys_id, number, short_description")),
            Some(vec![
                "sys_id".to_string(),
                "number".to_string(),
                "short_description".to_string(),
            ])
        );
    }
}
