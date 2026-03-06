use std::fs::File;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::cli::args::{DataArgs, DataCommands, OutputFormat};
use crate::cli::output;
use crate::client::pagination::PaginationConfig;
use crate::models::record::{Record, SingleRecordResponse};

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
        DataCommands::Validate { file } => {
            ensure_json_output(format, "data validate")?;
            handle_validate(profile, format, instance, &file).await
        }
        DataCommands::Import { file } => {
            ensure_json_output(format, "data import")?;
            handle_import(profile, format, instance, &file).await
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct TableExportArtifact {
    version: u8,
    kind: String,
    #[serde(default = "default_export_command")]
    command: String,
    instance: String,
    table: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fields: Option<Vec<String>>,
    #[serde(default)]
    exported_at_unix_s: u64,
    #[serde(default)]
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

#[derive(Debug, Serialize)]
struct ValidationReport {
    kind: &'static str,
    command: &'static str,
    dataset_kind: String,
    table: String,
    ready: bool,
    record_count: usize,
    field_count: usize,
    errors: Vec<ValidationIssue>,
    warnings: Vec<ValidationIssue>,
}

#[derive(Debug, Serialize)]
struct ValidationIssue {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    record_index: Option<usize>,
    message: String,
}

#[derive(Debug, Serialize)]
struct ImportReport {
    kind: &'static str,
    command: &'static str,
    strategy: &'static str,
    strategy_reason: &'static str,
    table: String,
    record_count: usize,
    created: usize,
    failed: usize,
    skipped: usize,
    validation: ImportValidationSummary,
    failures: Vec<ImportFailure>,
}

#[derive(Debug, Serialize)]
struct ImportValidationSummary {
    ready: bool,
    error_count: usize,
    warning_count: usize,
}

#[derive(Debug, Serialize)]
struct ImportFailure {
    record_index: usize,
    message: String,
}

#[derive(Debug, Clone)]
struct SchemaField {
    name: String,
    internal_type: String,
    mandatory: bool,
    read_only: bool,
    default_value: Option<String>,
}

#[derive(Debug)]
struct TableDefinition {
    name: String,
    super_class_sys_id: Option<String>,
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
        kind: "table-export".to_string(),
        command: default_export_command(),
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

async fn handle_validate(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    file: &str,
) -> anyhow::Result<()> {
    let artifact = read_dataset_file(file)?;
    let report = build_validation_report(profile, instance, &artifact).await?;
    output::print_output(&report, format)
}

async fn handle_import(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    file: &str,
) -> anyhow::Result<()> {
    let artifact = read_dataset_file(file)?;
    let report = build_validation_report(profile, instance, &artifact).await?;

    if !report.ready {
        anyhow::bail!(
            "Dataset validation failed for table '{}' with {} error(s)",
            report.table,
            report.errors.len()
        );
    }

    let mut client = crate::client::build_client(profile, instance)?;
    let path = format!("/api/now/table/{}", artifact.table);
    let mut created = 0usize;
    let mut failures = Vec::new();

    for (record_index, record) in artifact.records.iter().enumerate() {
        let body = serde_json::to_string(record)?;
        match client.post_json::<SingleRecordResponse>(&path, &body).await {
            Ok(_) => created += 1,
            Err(error) => failures.push(ImportFailure {
                record_index,
                message: error.to_string(),
            }),
        }
    }

    let import_report = ImportReport {
        kind: "import-result",
        command: "data import",
        strategy: "table_api",
        strategy_reason: "Import Set API bulk loading is not implemented yet, so the CLI used direct Table API create requests",
        table: artifact.table,
        record_count: artifact.record_count,
        created,
        failed: failures.len(),
        skipped: 0,
        validation: ImportValidationSummary {
            ready: true,
            error_count: report.errors.len(),
            warning_count: report.warnings.len(),
        },
        failures,
    };

    output::print_output(&import_report, format)?;

    if import_report.failed > 0 {
        anyhow::bail!(
            "Import completed with {} failed record(s) for table '{}'",
            import_report.failed,
            import_report.table
        );
    }

    Ok(())
}

async fn build_validation_report(
    profile: &str,
    instance: Option<&str>,
    artifact: &TableExportArtifact,
) -> anyhow::Result<ValidationReport> {
    let schema_fields = fetch_table_schema(profile, instance, &artifact.table).await?;
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if artifact.kind != "table-export" {
        errors.push(ValidationIssue {
            kind: "dataset_kind",
            field: None,
            record_index: None,
            message: format!(
                "Unsupported dataset kind '{}'; only 'table-export' is supported in v1",
                artifact.kind
            ),
        });
    }

    if schema_fields.is_empty() {
        errors.push(ValidationIssue {
            kind: "table",
            field: None,
            record_index: None,
            message: format!(
                "Table '{}' could not be resolved via sys_dictionary or has no readable columns",
                artifact.table
            ),
        });
    }

    let field_names = artifact_field_names(artifact);
    let schema_by_name = schema_fields
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect::<std::collections::HashMap<_, _>>();

    for field_name in &field_names {
        match schema_by_name.get(field_name.as_str()) {
            Some(field) => {
                if field.read_only || is_system_managed_field(field_name) {
                    errors.push(ValidationIssue {
                        kind: "field_not_writable",
                        field: Some(field_name.clone()),
                        record_index: None,
                        message: format!(
                            "Field '{}' is read-only or system-managed and cannot be imported with create-only Table API writes",
                            field_name
                        ),
                    });
                } else if is_unsupported_field_type(&field.internal_type) {
                    warnings.push(ValidationIssue {
                        kind: "unsupported_field_type",
                        field: Some(field_name.clone()),
                        record_index: None,
                        message: format!(
                            "Field '{}' uses internal type '{}' which may not import cleanly in v1",
                            field_name, field.internal_type
                        ),
                    });
                }
            }
            None => errors.push(ValidationIssue {
                kind: "unknown_field",
                field: Some(field_name.clone()),
                record_index: None,
                message: format!(
                    "Field '{}' does not exist on table '{}'",
                    field_name, artifact.table
                ),
            }),
        }
    }

    for required_field in schema_fields.iter().filter(|field| {
        field.mandatory
            && !field.read_only
            && field.default_value.as_deref().unwrap_or("").is_empty()
    }) {
        for (record_index, record) in artifact.records.iter().enumerate() {
            if !record.fields.contains_key(&required_field.name) {
                errors.push(ValidationIssue {
                    kind: "missing_required_field",
                    field: Some(required_field.name.clone()),
                    record_index: Some(record_index),
                    message: format!(
                        "Record {} is missing required field '{}'",
                        record_index, required_field.name
                    ),
                });
            }
        }
    }

    Ok(ValidationReport {
        kind: "validation-report",
        command: "data validate",
        dataset_kind: artifact.kind.clone(),
        table: artifact.table.clone(),
        ready: errors.is_empty(),
        record_count: artifact.records.len(),
        field_count: field_names.len(),
        errors,
        warnings,
    })
}

async fn fetch_table_schema(
    profile: &str,
    instance: Option<&str>,
    table: &str,
) -> anyhow::Result<Vec<SchemaField>> {
    let mut client = crate::client::build_client(profile, instance)?;
    let table_names = fetch_table_hierarchy(&mut client, table).await?;
    let query = if table_names.len() == 1 {
        format!(
            "name={}^elementISNOTEMPTY^element!=sys_tags",
            table_names[0]
        )
    } else {
        format!(
            "nameIN{}^elementISNOTEMPTY^element!=sys_tags",
            table_names.join(",")
        )
    };
    let pagination = PaginationConfig::default()
        .with_page_size(500)
        .with_limit(None);

    let records = client
        .get_table_records(
            "sys_dictionary",
            Some(&query),
            Some("element,internal_type,mandatory,read_only,default_value"),
            &pagination,
            Some("element"),
        )
        .await?;

    Ok(records
        .into_iter()
        .map(|record| SchemaField {
            name: record.get_str("element").unwrap_or_default().to_string(),
            internal_type: record
                .fields
                .get("internal_type")
                .and_then(json_value_as_text)
                .unwrap_or_default(),
            mandatory: record
                .fields
                .get("mandatory")
                .map(json_value_as_bool)
                .unwrap_or(false),
            read_only: record
                .fields
                .get("read_only")
                .map(json_value_as_bool)
                .unwrap_or(false),
            default_value: if record
                .fields
                .get("default_value")
                .and_then(json_value_as_text)
                .as_deref()
                .unwrap_or("")
                .is_empty()
            {
                None
            } else {
                record
                    .fields
                    .get("default_value")
                    .and_then(json_value_as_text)
            },
        })
        .filter(|field| !field.name.is_empty())
        .collect())
}

async fn fetch_table_hierarchy(
    client: &mut crate::client::SnowClient,
    table: &str,
) -> anyhow::Result<Vec<String>> {
    let mut table_names = Vec::new();
    let mut current = match fetch_table_definition_by_name(client, table).await {
        Ok(current) => current,
        Err(error) if is_not_found_error(&error) => None,
        Err(error) => return Err(error),
    };

    while let Some(definition) = current {
        let next_super_class = definition.super_class_sys_id.clone();
        table_names.push(definition.name);
        current = match next_super_class {
            Some(sys_id) if !sys_id.is_empty() => {
                fetch_table_definition_by_sys_id(client, &sys_id).await?
            }
            _ => None,
        };
    }

    if table_names.is_empty() {
        table_names.push(table.to_string());
    }

    Ok(table_names)
}

async fn fetch_table_definition_by_name(
    client: &mut crate::client::SnowClient,
    table: &str,
) -> anyhow::Result<Option<TableDefinition>> {
    let query = format!("name={table}");
    fetch_table_definition(client, &query).await
}

async fn fetch_table_definition_by_sys_id(
    client: &mut crate::client::SnowClient,
    sys_id: &str,
) -> anyhow::Result<Option<TableDefinition>> {
    let query = format!("sys_id={sys_id}");
    fetch_table_definition(client, &query).await
}

async fn fetch_table_definition(
    client: &mut crate::client::SnowClient,
    query: &str,
) -> anyhow::Result<Option<TableDefinition>> {
    let pagination = PaginationConfig::default().with_limit(Some(1));
    let records = client
        .get_table_records(
            "sys_db_object",
            Some(query),
            Some("name,super_class"),
            &pagination,
            None,
        )
        .await?;

    Ok(records.into_iter().next().map(|record| TableDefinition {
        name: record.get_str("name").unwrap_or_default().to_string(),
        super_class_sys_id: record
            .fields
            .get("super_class")
            .and_then(json_value_as_text),
    }))
}

fn read_dataset_file(file: &str) -> anyhow::Result<TableExportArtifact> {
    let body = std::fs::read_to_string(file)?;
    let artifact: TableExportArtifact = serde_json::from_str(&body)
        .map_err(|error| anyhow::anyhow!("Invalid dataset file '{}': {}", file, error))?;
    Ok(artifact)
}

fn ensure_json_output(format: &OutputFormat, command: &str) -> anyhow::Result<()> {
    if matches!(format, OutputFormat::Csv) {
        anyhow::bail!("`{}` currently supports only JSON output", command);
    }
    Ok(())
}

fn artifact_field_names(artifact: &TableExportArtifact) -> Vec<String> {
    let mut field_names = std::collections::BTreeSet::new();
    if let Some(fields) = &artifact.fields {
        field_names.extend(fields.iter().cloned());
    }
    for record in &artifact.records {
        field_names.extend(record.fields.keys().cloned());
    }
    field_names.into_iter().collect()
}

fn is_system_managed_field(field_name: &str) -> bool {
    matches!(
        field_name,
        "sys_id"
            | "sys_created_on"
            | "sys_created_by"
            | "sys_updated_on"
            | "sys_updated_by"
            | "sys_mod_count"
            | "sys_tags"
            | "sys_domain_path"
    )
}

fn is_unsupported_field_type(internal_type: &str) -> bool {
    matches!(
        internal_type,
        "journal" | "journal_input" | "script" | "translated_html" | "password"
    )
}

fn default_export_command() -> String {
    "data export".to_string()
}

fn json_value_as_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Object(map) => map
            .get("value")
            .and_then(json_value_as_text)
            .or_else(|| map.get("display_value").and_then(json_value_as_text)),
        serde_json::Value::Array(_) => None,
    }
}

fn json_value_as_bool(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Bool(flag) => *flag,
        serde_json::Value::String(text) => text == "true" || text == "1",
        serde_json::Value::Number(number) => number.as_i64().unwrap_or_default() != 0,
        serde_json::Value::Object(_) => json_value_as_text(value)
            .map(|text| text == "true" || text == "1")
            .unwrap_or(false),
        serde_json::Value::Null | serde_json::Value::Array(_) => false,
    }
}

fn is_not_found_error(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<crate::client::error::ApiError>()
        .map(|api_error| api_error.status == 404)
        .unwrap_or(false)
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

    #[test]
    fn test_json_value_as_text_from_reference_object() {
        let value = serde_json::json!({
            "link": "https://example.com/api/now/table/sys_glide_object?name=integer",
            "value": "integer"
        });

        assert_eq!(json_value_as_text(&value), Some("integer".to_string()));
    }

    #[test]
    fn test_json_value_as_bool_from_string() {
        assert!(json_value_as_bool(&serde_json::json!("true")));
        assert!(!json_value_as_bool(&serde_json::json!("false")));
    }
}
