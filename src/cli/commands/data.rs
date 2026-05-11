use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::cli::args::{DataArgs, DataCommands, OutputFormat};
use crate::cli::output;
use crate::cli::validation::validate_table_name;
use crate::client::pagination::PaginationConfig;
use crate::models::record::{Record, SingleRecordResponse};

const LONG_RUNNING_TIMEOUT_SECS: u64 = 180;

pub async fn handle(
    args: DataArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
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
            handle_export(profile, format, instance, timeout_secs, export).await
        }
        DataCommands::ExportPackage { file, out_dir } => {
            ensure_json_output(format, "data export-package")?;
            handle_export_package(profile, format, instance, timeout_secs, &file, &out_dir).await
        }
        DataCommands::Validate { file } => {
            ensure_json_output(format, "data validate")?;
            handle_validate(profile, format, instance, timeout_secs, &file).await
        }
        DataCommands::Import {
            file,
            dry_run,
            import_set_table,
            fail_on_error,
        } => {
            ensure_json_output(format, "data import")?;
            handle_import(
                profile,
                format,
                instance,
                timeout_secs,
                &file,
                ImportExecutionOptions {
                    dry_run,
                    import_set_table: import_set_table.as_deref(),
                    fail_on_error,
                },
            )
            .await
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

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Deserialize)]
struct ImportSetApiResponse {
    #[serde(default)]
    result: Vec<ImportSetApiResult>,
}

#[derive(Debug, Deserialize)]
struct ImportSetApiResult {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    error_message: Option<String>,
    #[serde(default)]
    status_message: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct ImportExecutionOptions<'a> {
    dry_run: bool,
    import_set_table: Option<&'a str>,
    fail_on_error: bool,
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

#[derive(Debug, Serialize, Deserialize)]
struct DatasetExportSpec {
    version: u8,
    kind: String,
    tables: Vec<DatasetTableSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DatasetTableSpec {
    name: String,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    fields: Option<Vec<String>>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    order_by: Option<String>,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    references: Vec<DatasetReferenceSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DatasetReferenceSpec {
    field: String,
    target_table: String,
    source_key: String,
    target_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct DatasetManifest {
    version: u8,
    kind: String,
    command: String,
    instance: String,
    exported_at_unix_s: u64,
    tables: Vec<DatasetManifestTable>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DatasetManifestTable {
    name: String,
    file: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    fields: Option<Vec<String>>,
    record_count: usize,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    references: Vec<DatasetReferenceSpec>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DatasetTableArtifact {
    version: u8,
    kind: String,
    table: String,
    #[serde(default)]
    source_key_fields: Vec<String>,
    records: Vec<DatasetTableRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DatasetTableRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    source_sys_id: Option<String>,
    data: Record,
}

#[derive(Debug, Serialize)]
struct DatasetExportSummary {
    kind: &'static str,
    command: &'static str,
    instance: String,
    table_count: usize,
    tables: Vec<DatasetTableSummary>,
    out_dir: String,
    manifest_path: String,
}

#[derive(Debug, Serialize)]
struct DatasetTableSummary {
    table: String,
    record_count: usize,
    file: String,
}

#[derive(Debug, Serialize)]
struct DatasetValidationReport {
    kind: &'static str,
    command: &'static str,
    dataset_kind: String,
    ready: bool,
    table_count: usize,
    import_order: Vec<String>,
    tables: Vec<ValidationReport>,
    errors: Vec<ValidationIssue>,
    warnings: Vec<ValidationIssue>,
}

#[derive(Debug, Serialize)]
struct DatasetImportReport {
    kind: &'static str,
    command: &'static str,
    strategy: &'static str,
    strategy_reason: &'static str,
    table_count: usize,
    import_order: Vec<String>,
    created: usize,
    failed: usize,
    skipped: usize,
    tables: Vec<TableImportResult>,
}

#[derive(Debug, Serialize)]
struct TableImportResult {
    table: String,
    created: usize,
    failed: usize,
    skipped: usize,
    failures: Vec<ImportFailure>,
}

#[derive(Debug, Deserialize)]
struct ReferencePlaceholder {
    #[serde(rename = "__reference")]
    reference: ReferenceMarker,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ReferenceMarker {
    target_table: String,
    source_key: String,
    target_key: String,
    source_value: String,
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
    timeout_secs: Option<u64>,
    export: ExportRequest,
) -> anyhow::Result<()> {
    tracing::info!("Exporting records from table: {}", export.table);

    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
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
        OutputFormat::Jsonl | OutputFormat::Toon => output::print_output(&artifact, format),
        OutputFormat::Text => output::print_output(&artifact, format),
    }
}

async fn handle_export_package(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    file: &str,
    out_dir: &str,
) -> anyhow::Result<()> {
    let spec = read_dataset_export_spec(file)?;
    validate_dataset_export_spec(&spec)?;

    let out_dir_path = PathBuf::from(out_dir);
    std::fs::create_dir_all(&out_dir_path)?;

    let import_order = topological_table_order(&spec.tables)?;
    let spec_by_name = spec
        .tables
        .iter()
        .cloned()
        .map(|table| (table.name.clone(), table))
        .collect::<BTreeMap<_, _>>();

    let required_source_keys = required_source_keys(&spec.tables);
    let mut client = crate::client::build_client_with_timeout(
        profile,
        instance,
        Some(long_running_timeout_secs(timeout_secs)),
    )?;
    let instance_url = client.base_url().to_string();
    let mut raw_records_by_table = BTreeMap::new();

    for table_name in &import_order {
        let table_spec = spec_by_name
            .get(table_name)
            .ok_or_else(|| anyhow::anyhow!("Missing dataset spec for table '{}'", table_name))?;
        let fetch_fields = package_fetch_fields(table_spec, &required_source_keys);
        let pagination = PaginationConfig::default().with_limit(table_spec.limit);
        let records = client
            .get_table_records(
                &table_spec.name,
                table_spec.query.as_deref(),
                fetch_fields.as_deref(),
                &pagination,
                table_spec.order_by.as_deref(),
            )
            .await?;
        raw_records_by_table.insert(table_name.clone(), records);
    }

    let mut source_value_indexes = build_source_value_indexes(&spec.tables, &raw_records_by_table);
    let mut manifest_tables = Vec::new();
    let mut summary_tables = Vec::new();

    for table_name in &import_order {
        let table_spec = spec_by_name
            .get(table_name)
            .ok_or_else(|| anyhow::anyhow!("Missing dataset spec for table '{}'", table_name))?;
        let raw_records = raw_records_by_table.get(table_name).ok_or_else(|| {
            anyhow::anyhow!("Missing exported records for table '{}'", table_name)
        })?;
        let artifact = build_dataset_table_artifact(
            table_spec,
            raw_records,
            &required_source_keys,
            &source_value_indexes,
        )?;
        let file_name = table_output_file_name(table_spec)?;
        let file_path = package_file_path(&out_dir_path, &file_name)?;
        write_json_file(&file_path, &artifact)?;

        source_value_indexes = build_source_value_indexes(&spec.tables, &raw_records_by_table);

        manifest_tables.push(DatasetManifestTable {
            name: table_spec.name.clone(),
            file: file_name.clone(),
            query: table_spec.query.clone(),
            fields: artifact_field_list(&artifact),
            record_count: artifact.records.len(),
            depends_on: table_spec.depends_on.clone(),
            references: table_spec.references.clone(),
        });
        summary_tables.push(DatasetTableSummary {
            table: table_spec.name.clone(),
            record_count: artifact.records.len(),
            file: file_name,
        });
    }

    let manifest = DatasetManifest {
        version: 1,
        kind: "dataset".to_string(),
        command: "data export-package".to_string(),
        instance: instance_url,
        exported_at_unix_s: current_unix_timestamp(),
        tables: manifest_tables,
    };

    let manifest_path = out_dir_path.join("manifest.json");
    write_json_file(&manifest_path, &manifest)?;

    let summary = DatasetExportSummary {
        kind: "dataset-export-result",
        command: "data export-package",
        instance: manifest.instance,
        table_count: summary_tables.len(),
        tables: summary_tables,
        out_dir: out_dir_path.display().to_string(),
        manifest_path: manifest_path.display().to_string(),
    };

    output::print_output(&summary, format)
}

async fn handle_validate(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    file: &str,
) -> anyhow::Result<()> {
    match read_dataset_input(file)? {
        DatasetInput::Flat(artifact) => {
            let report =
                build_flat_validation_report(profile, instance, timeout_secs, &artifact).await?;
            output::print_output(&report, format)
        }
        DatasetInput::Package(manifest) => {
            let report = build_package_validation_report(
                profile,
                instance,
                Some(long_running_timeout_secs(timeout_secs)),
                file,
                &manifest,
            )
            .await?;
            output::print_output(&report, format)
        }
    }
}

async fn handle_import(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    file: &str,
    options: ImportExecutionOptions<'_>,
) -> anyhow::Result<()> {
    match read_dataset_input(file)? {
        DatasetInput::Flat(artifact) => {
            handle_import_flat(profile, format, instance, timeout_secs, &artifact, options).await
        }
        DatasetInput::Package(manifest) => {
            if options.import_set_table.is_some() {
                anyhow::bail!(
                    "--import-set-table is currently only supported for flat table-export artifacts, not dataset package imports"
                );
            }

            handle_import_package(
                profile,
                format,
                instance,
                Some(long_running_timeout_secs(timeout_secs)),
                file,
                &manifest,
                options.dry_run,
            )
            .await
        }
    }
}

async fn build_flat_validation_report(
    profile: &str,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    artifact: &TableExportArtifact,
) -> anyhow::Result<ValidationReport> {
    build_table_validation_report(
        profile,
        instance,
        timeout_secs,
        &artifact.kind,
        &artifact.table,
        artifact.fields.as_deref(),
        &artifact.records,
    )
    .await
}

async fn build_table_validation_report(
    profile: &str,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    dataset_kind: &str,
    table: &str,
    fields: Option<&[String]>,
    records: &[Record],
) -> anyhow::Result<ValidationReport> {
    let schema_fields = fetch_table_schema(profile, instance, timeout_secs, table).await?;
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if dataset_kind != "table-export" && dataset_kind != "dataset-table" {
        errors.push(ValidationIssue {
            kind: "dataset_kind",
            field: None,
            record_index: None,
            message: format!(
                "Unsupported dataset kind '{}'; only 'table-export' and 'dataset-table' are supported in v1",
                dataset_kind
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
                table
            ),
        });
    }

    let field_names = record_field_names(fields, records);
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
                message: format!("Field '{}' does not exist on table '{}'", field_name, table),
            }),
        }
    }

    for required_field in schema_fields.iter().filter(|field| {
        field.mandatory
            && !field.read_only
            && field.default_value.as_deref().unwrap_or("").is_empty()
    }) {
        for (record_index, record) in records.iter().enumerate() {
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
        dataset_kind: dataset_kind.to_string(),
        table: table.to_string(),
        ready: errors.is_empty(),
        record_count: records.len(),
        field_count: field_names.len(),
        errors,
        warnings,
    })
}

async fn handle_import_flat(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    artifact: &TableExportArtifact,
    options: ImportExecutionOptions<'_>,
) -> anyhow::Result<()> {
    let report = build_flat_validation_report(profile, instance, timeout_secs, artifact).await?;

    if !report.ready {
        anyhow::bail!(
            "Dataset validation failed for table '{}' with {} error(s)",
            report.table,
            report.errors.len()
        );
    }

    if options.dry_run {
        let (strategy, strategy_reason) = if options.import_set_table.is_some() {
            (
                "import_set",
                "Dry run preview for create-only Import Set API loading into the selected staging table",
            )
        } else {
            (
                "table_api",
                "Dry run preview for create-only Table API import",
            )
        };

        let import_report = ImportReport {
            kind: "import-dry-run",
            command: "data import",
            strategy,
            strategy_reason,
            table: artifact.table.clone(),
            record_count: artifact.record_count,
            created: 0,
            failed: 0,
            skipped: artifact.record_count,
            validation: ImportValidationSummary {
                ready: true,
                error_count: report.errors.len(),
                warning_count: report.warnings.len(),
            },
            failures: Vec::new(),
        };

        return output::print_output(&import_report, format);
    }

    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
    validate_table_name(&artifact.table)?;
    let use_import_set = options.import_set_table.is_some();
    let path = if let Some(staging_table) = options.import_set_table {
        validate_table_name(staging_table)?;
        format!("/api/now/import/{staging_table}")
    } else {
        format!("/api/now/table/{}", artifact.table)
    };
    let mut created = 0usize;
    let mut failures = Vec::new();

    for (record_index, record) in artifact.records.iter().enumerate() {
        let body = serde_json::to_string(record)?;
        if use_import_set {
            match client.post_json::<ImportSetApiResponse>(&path, &body).await {
                Ok(response) => {
                    let error_message = response.result.iter().find_map(|result| {
                        match result
                            .status
                            .as_deref()
                            .map(str::to_ascii_lowercase)
                            .as_deref()
                        {
                            Some("error") => result
                                .error_message
                                .clone()
                                .or_else(|| result.status_message.clone())
                                .or_else(|| {
                                    Some("Import Set API returned an error row".to_string())
                                }),
                            _ => None,
                        }
                    });

                    if let Some(message) = error_message {
                        failures.push(ImportFailure {
                            record_index,
                            message,
                        });
                    } else {
                        created += 1;
                    }
                }
                Err(error) => failures.push(ImportFailure {
                    record_index,
                    message: error.to_string(),
                }),
            }
        } else {
            match client.post_json::<SingleRecordResponse>(&path, &body).await {
                Ok(_) => created += 1,
                Err(error) => failures.push(ImportFailure {
                    record_index,
                    message: error.to_string(),
                }),
            }
        }
    }

    let (strategy, strategy_reason) = if options.import_set_table.is_some() {
        (
            "import_set",
            "Used the Import Set API with the selected staging table for flat create-only import",
        )
    } else {
        (
            "table_api",
            "Import Set API bulk loading was not selected, so the CLI used direct Table API create requests",
        )
    };

    let import_report = ImportReport {
        kind: "import-result",
        command: "data import",
        strategy,
        strategy_reason,
        table: artifact.table.clone(),
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

    if use_import_set && options.fail_on_error && import_report.failed > 0 {
        anyhow::bail!(
            "Import Set-backed data import completed with {} failed record(s). Re-run without --fail-on-error to inspect the structured response without failing the command.",
            import_report.failed
        );
    }

    if import_report.failed > 0 {
        anyhow::bail!(
            "Import completed with {} failed record(s) for table '{}'",
            import_report.failed,
            import_report.table
        );
    }

    Ok(())
}

async fn build_package_validation_report(
    profile: &str,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    file: &str,
    manifest: &DatasetManifest,
) -> anyhow::Result<DatasetValidationReport> {
    let tables_by_name = manifest_table_map(&manifest.tables)?;
    let import_order = topological_manifest_order(&manifest.tables)?;
    let base_dir = dataset_base_dir(file);
    let mut table_reports = Vec::new();
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut table_artifacts = BTreeMap::new();

    for table in &manifest.tables {
        let artifact = read_dataset_table_artifact(&package_file_path(&base_dir, &table.file)?)?;
        let report = build_table_validation_report(
            profile,
            instance,
            timeout_secs,
            &artifact.kind,
            &table.name,
            table.fields.as_deref(),
            &artifact
                .records
                .iter()
                .map(|record| record.data.clone())
                .collect::<Vec<_>>(),
        )
        .await?;
        errors.extend(report.errors.clone());
        warnings.extend(report.warnings.clone());
        table_reports.push(report);
        table_artifacts.insert(table.name.clone(), artifact);
    }

    for table in &manifest.tables {
        for reference in &table.references {
            if !tables_by_name.contains_key(&reference.target_table) {
                errors.push(ValidationIssue {
                    kind: "missing_dependency_table",
                    field: Some(reference.field.clone()),
                    record_index: None,
                    message: format!(
                        "Reference field '{}' on table '{}' points to missing target table '{}'",
                        reference.field, table.name, reference.target_table
                    ),
                });
                continue;
            }

            let target_artifact =
                table_artifacts
                    .get(&reference.target_table)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Missing dataset table artifact for '{}'",
                            reference.target_table
                        )
                    })?;

            let source_values = target_artifact
                .records
                .iter()
                .filter_map(|record| {
                    record
                        .data
                        .fields
                        .get(&reference.source_key)
                        .and_then(json_value_as_text)
                })
                .collect::<BTreeSet<_>>();

            for (record_index, record) in table_artifacts
                .get(&table.name)
                .ok_or_else(|| {
                    anyhow::anyhow!("Missing dataset table artifact for '{}'", table.name)
                })?
                .records
                .iter()
                .enumerate()
            {
                match extract_reference_marker(record.data.fields.get(&reference.field)) {
                    Some(marker) => {
                        if marker.target_table != reference.target_table
                            || marker.source_key != reference.source_key
                            || marker.target_key != reference.target_key
                        {
                            errors.push(ValidationIssue {
                                kind: "reference_mismatch",
                                field: Some(reference.field.clone()),
                                record_index: Some(record_index),
                                message: format!(
                                    "Reference placeholder for '{}.{}' does not match the manifest definition",
                                    table.name, reference.field
                                ),
                            });
                        } else if !source_values.contains(&marker.source_value) {
                            errors.push(ValidationIssue {
                                kind: "unresolved_reference",
                                field: Some(reference.field.clone()),
                                record_index: Some(record_index),
                                message: format!(
                                    "Reference '{}.{}' points to source value '{}' which does not exist in table '{}'",
                                    table.name, reference.field, marker.source_value, reference.target_table
                                ),
                            });
                        }
                    }
                    None => errors.push(ValidationIssue {
                        kind: "missing_reference_placeholder",
                        field: Some(reference.field.clone()),
                        record_index: Some(record_index),
                        message: format!(
                            "Reference field '{}.{}' is missing a dataset reference placeholder",
                            table.name, reference.field
                        ),
                    }),
                }
            }
        }
    }

    Ok(DatasetValidationReport {
        kind: "dataset-validation-report",
        command: "data validate",
        dataset_kind: manifest.kind.clone(),
        ready: errors.is_empty(),
        table_count: manifest.tables.len(),
        import_order,
        tables: table_reports,
        errors,
        warnings,
    })
}

async fn handle_import_package(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    file: &str,
    manifest: &DatasetManifest,
    dry_run: bool,
) -> anyhow::Result<()> {
    let validation =
        build_package_validation_report(profile, instance, timeout_secs, file, manifest).await?;
    if !validation.ready {
        anyhow::bail!(
            "Dataset package validation failed with {} error(s)",
            validation.errors.len()
        );
    }

    if dry_run {
        let table_results = manifest
            .tables
            .iter()
            .map(|table| TableImportResult {
                table: table.name.clone(),
                created: 0,
                failed: 0,
                skipped: table.record_count,
                failures: Vec::new(),
            })
            .collect::<Vec<_>>();

        let report = DatasetImportReport {
            kind: "dataset-import-dry-run",
            command: "data import",
            strategy: "table_api",
            strategy_reason: "Dry run preview for ordered Table API create requests with reference remapping",
            table_count: manifest.tables.len(),
            import_order: validation.import_order,
            created: 0,
            failed: 0,
            skipped: manifest.tables.iter().map(|table| table.record_count).sum(),
            tables: table_results,
        };

        return output::print_output(&report, format);
    }

    let base_dir = dataset_base_dir(file);
    let manifest_tables = manifest_table_map(&manifest.tables)?;
    let required_source_keys = required_source_keys_from_manifest(&manifest.tables);
    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
    let mut source_to_target_ids: HashMap<String, HashMap<String, HashMap<String, String>>> =
        HashMap::new();
    let mut table_results = Vec::new();
    let mut created_total = 0usize;
    let mut failed_total = 0usize;

    for table_name in &validation.import_order {
        let manifest_table = manifest_tables
            .get(table_name)
            .ok_or_else(|| anyhow::anyhow!("Missing manifest table '{}'", table_name))?;
        validate_table_name(&manifest_table.name)?;
        let artifact =
            read_dataset_table_artifact(&package_file_path(&base_dir, &manifest_table.file)?)?;
        let path = format!("/api/now/table/{}", manifest_table.name);
        let mut created = 0usize;
        let mut failures = Vec::new();

        for (record_index, record) in artifact.records.iter().enumerate() {
            let body_record = remap_reference_fields(
                &record.data,
                &manifest_table.references,
                &source_to_target_ids,
            )?;
            let body = serde_json::to_string(&body_record)?;
            match client.post_json::<SingleRecordResponse>(&path, &body).await {
                Ok(response) => {
                    created += 1;
                    created_total += 1;
                    if let Some(source_keys) = required_source_keys.get(&manifest_table.name) {
                        let target_sys_id =
                            response.result.sys_id().unwrap_or_default().to_string();
                        for source_key in source_keys {
                            if let Some(source_value) = record
                                .data
                                .fields
                                .get(source_key)
                                .and_then(json_value_as_text)
                            {
                                source_to_target_ids
                                    .entry(manifest_table.name.clone())
                                    .or_default()
                                    .entry(source_key.clone())
                                    .or_default()
                                    .insert(source_value, target_sys_id.clone());
                            }
                        }
                    }
                }
                Err(error) => {
                    failed_total += 1;
                    failures.push(ImportFailure {
                        record_index,
                        message: error.to_string(),
                    });
                }
            }
        }

        table_results.push(TableImportResult {
            table: manifest_table.name.clone(),
            created,
            failed: failures.len(),
            skipped: 0,
            failures,
        });
    }

    let report = DatasetImportReport {
        kind: "dataset-import-result",
        command: "data import",
        strategy: "table_api",
        strategy_reason: "Import Set API bulk loading is not implemented yet, so the CLI used ordered Table API create requests with reference remapping",
        table_count: manifest.tables.len(),
        import_order: validation.import_order,
        created: created_total,
        failed: failed_total,
        skipped: 0,
        tables: table_results,
    };

    output::print_output(&report, format)?;

    if report.failed > 0 {
        anyhow::bail!(
            "Dataset package import completed with {} failed record(s)",
            report.failed
        );
    }

    Ok(())
}

async fn fetch_table_schema(
    profile: &str,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    table: &str,
) -> anyhow::Result<Vec<SchemaField>> {
    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
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

enum DatasetInput {
    Flat(TableExportArtifact),
    Package(DatasetManifest),
}

fn read_dataset_input(file: &str) -> anyhow::Result<DatasetInput> {
    let body = std::fs::read_to_string(file)?;
    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|error| anyhow::anyhow!("Invalid dataset file '{}': {}", file, error))?;
    let kind = value
        .get("kind")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            anyhow::anyhow!("Dataset file '{}' is missing a string 'kind' field", file)
        })?;

    match kind {
        "table-export" => Ok(DatasetInput::Flat(serde_json::from_value(value)?)),
        "dataset" => Ok(DatasetInput::Package(serde_json::from_value(value)?)),
        _ => anyhow::bail!(
            "Unsupported dataset kind '{}' in file '{}'; expected 'table-export' or 'dataset'",
            kind,
            file
        ),
    }
}

fn read_dataset_export_spec(file: &str) -> anyhow::Result<DatasetExportSpec> {
    let body = std::fs::read_to_string(file)?;
    let spec: DatasetExportSpec = serde_json::from_str(&body)
        .map_err(|error| anyhow::anyhow!("Invalid dataset export spec '{}': {}", file, error))?;
    Ok(spec)
}

fn read_dataset_table_artifact(path: &Path) -> anyhow::Result<DatasetTableArtifact> {
    let body = std::fs::read_to_string(path)?;
    let artifact: DatasetTableArtifact = serde_json::from_str(&body).map_err(|error| {
        anyhow::anyhow!(
            "Invalid dataset table artifact '{}': {}",
            path.display(),
            error
        )
    })?;
    Ok(artifact)
}

fn validate_dataset_export_spec(spec: &DatasetExportSpec) -> anyhow::Result<()> {
    if spec.kind != "dataset-export-spec" {
        anyhow::bail!(
            "Unsupported dataset export spec kind '{}'; expected 'dataset-export-spec'",
            spec.kind
        );
    }
    if spec.tables.is_empty() {
        anyhow::bail!("Dataset export spec must include at least one table");
    }
    let mut seen = BTreeSet::new();
    for table in &spec.tables {
        if !seen.insert(&table.name) {
            anyhow::bail!(
                "Dataset export spec contains duplicate table '{}'",
                table.name
            );
        }
    }
    topological_table_order(&spec.tables)?;
    Ok(())
}

fn topological_table_order(tables: &[DatasetTableSpec]) -> anyhow::Result<Vec<String>> {
    let mut deps = BTreeMap::<String, BTreeSet<String>>::new();
    let table_names = tables
        .iter()
        .map(|table| table.name.clone())
        .collect::<BTreeSet<_>>();

    for table in tables {
        let mut current_deps = table.depends_on.iter().cloned().collect::<BTreeSet<_>>();
        current_deps.extend(
            table
                .references
                .iter()
                .map(|reference| reference.target_table.clone()),
        );
        for dep in &current_deps {
            if !table_names.contains(dep) {
                anyhow::bail!("Table '{}' depends on missing table '{}'", table.name, dep);
            }
        }
        deps.insert(table.name.clone(), current_deps);
    }

    topo_sort_dependencies(deps)
}

fn topological_manifest_order(tables: &[DatasetManifestTable]) -> anyhow::Result<Vec<String>> {
    let mut deps = BTreeMap::<String, BTreeSet<String>>::new();
    let table_names = tables
        .iter()
        .map(|table| table.name.clone())
        .collect::<BTreeSet<_>>();

    for table in tables {
        let mut current_deps = table.depends_on.iter().cloned().collect::<BTreeSet<_>>();
        current_deps.extend(
            table
                .references
                .iter()
                .map(|reference| reference.target_table.clone()),
        );
        for dep in &current_deps {
            if !table_names.contains(dep) {
                anyhow::bail!(
                    "Manifest table '{}' depends on missing table '{}'",
                    table.name,
                    dep
                );
            }
        }
        deps.insert(table.name.clone(), current_deps);
    }

    topo_sort_dependencies(deps)
}

fn topo_sort_dependencies(deps: BTreeMap<String, BTreeSet<String>>) -> anyhow::Result<Vec<String>> {
    let mut incoming = deps.clone();
    let mut outgoing = BTreeMap::<String, BTreeSet<String>>::new();
    for (table, table_deps) in &deps {
        for dep in table_deps {
            outgoing
                .entry(dep.clone())
                .or_default()
                .insert(table.clone());
        }
    }

    let mut ready = incoming
        .iter()
        .filter(|(_, deps)| deps.is_empty())
        .map(|(table, _)| table.clone())
        .collect::<VecDeque<_>>();
    let mut order = Vec::new();

    while let Some(table) = ready.pop_front() {
        order.push(table.clone());
        if let Some(children) = outgoing.get(&table).cloned() {
            for child in children {
                if let Some(child_deps) = incoming.get_mut(&child) {
                    child_deps.remove(&table);
                    if child_deps.is_empty() {
                        ready.push_back(child.clone());
                    }
                }
            }
        }
        incoming.remove(&table);
    }

    if !incoming.is_empty() {
        anyhow::bail!(
            "Dataset contains a dependency cycle involving: {}",
            incoming.keys().cloned().collect::<Vec<_>>().join(", ")
        );
    }

    Ok(order)
}

fn required_source_keys(tables: &[DatasetTableSpec]) -> HashMap<String, BTreeSet<String>> {
    let mut keys = HashMap::<String, BTreeSet<String>>::new();
    for table in tables {
        for reference in &table.references {
            keys.entry(reference.target_table.clone())
                .or_default()
                .insert(reference.source_key.clone());
        }
    }
    keys
}

fn required_source_keys_from_manifest(
    tables: &[DatasetManifestTable],
) -> HashMap<String, BTreeSet<String>> {
    let mut keys = HashMap::<String, BTreeSet<String>>::new();
    for table in tables {
        for reference in &table.references {
            keys.entry(reference.target_table.clone())
                .or_default()
                .insert(reference.source_key.clone());
        }
    }
    keys
}

fn package_fetch_fields(
    table: &DatasetTableSpec,
    required_source_keys: &HashMap<String, BTreeSet<String>>,
) -> Option<String> {
    let mut fields = table
        .fields
        .clone()
        .unwrap_or_default()
        .into_iter()
        .collect::<BTreeSet<_>>();
    fields.insert("sys_id".to_string());
    for reference in &table.references {
        fields.insert(reference.field.clone());
    }
    if let Some(keys) = required_source_keys.get(&table.name) {
        fields.extend(keys.iter().cloned());
    }
    Some(fields.into_iter().collect::<Vec<_>>().join(","))
}

fn build_source_value_indexes(
    tables: &[DatasetTableSpec],
    raw_records_by_table: &BTreeMap<String, Vec<Record>>,
) -> HashMap<String, HashMap<String, HashMap<String, String>>> {
    let required_keys = required_source_keys(tables);
    let mut indexes = HashMap::new();
    for (table, keys) in required_keys {
        if let Some(records) = raw_records_by_table.get(&table) {
            let mut key_index = HashMap::new();
            for key in keys {
                let mut source_values = HashMap::new();
                for record in records {
                    if let (Some(source_sys_id), Some(source_value)) = (
                        record.fields.get("sys_id").and_then(json_value_as_text),
                        record.fields.get(&key).and_then(json_value_as_text),
                    ) {
                        source_values.insert(source_sys_id, source_value);
                    }
                }
                key_index.insert(key, source_values);
            }
            indexes.insert(table, key_index);
        }
    }
    indexes
}

fn build_dataset_table_artifact(
    table: &DatasetTableSpec,
    raw_records: &[Record],
    required_source_keys: &HashMap<String, BTreeSet<String>>,
    source_value_indexes: &HashMap<String, HashMap<String, HashMap<String, String>>>,
) -> anyhow::Result<DatasetTableArtifact> {
    let mut exported_fields = table
        .fields
        .clone()
        .unwrap_or_default()
        .into_iter()
        .collect::<BTreeSet<_>>();
    if let Some(keys) = required_source_keys.get(&table.name) {
        exported_fields.extend(keys.iter().cloned());
    }
    for reference in &table.references {
        exported_fields.insert(reference.field.clone());
    }

    let mut records = Vec::new();
    for raw_record in raw_records {
        let mut fields = if table.fields.is_none() {
            raw_record.fields.clone()
        } else {
            raw_record
                .fields
                .iter()
                .filter(|(key, _)| exported_fields.contains(*key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<HashMap<_, _>>()
        };
        fields.remove("sys_id");

        for reference in &table.references {
            let source_sys_id = raw_record
                .fields
                .get(&reference.field)
                .and_then(json_value_as_text)
                .unwrap_or_default();
            if source_sys_id.is_empty() {
                fields.insert(reference.field.clone(), serde_json::Value::Null);
                continue;
            }
            let source_value = source_value_indexes
                .get(&reference.target_table)
                .and_then(|by_key| by_key.get(&reference.source_key))
                .and_then(|by_sys_id| by_sys_id.get(&source_sys_id))
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Unable to export reference '{}.{}': source sys_id '{}' was not found in target table '{}' using source key '{}'",
                        table.name,
                        reference.field,
                        source_sys_id,
                        reference.target_table,
                        reference.source_key
                    )
                })?;

            fields.insert(
                reference.field.clone(),
                serde_json::json!({
                    "__reference": {
                        "target_table": reference.target_table,
                        "source_key": reference.source_key,
                        "target_key": reference.target_key,
                        "source_value": source_value,
                    }
                }),
            );
        }

        records.push(DatasetTableRecord {
            source_sys_id: raw_record.fields.get("sys_id").and_then(json_value_as_text),
            data: Record { fields },
        });
    }

    Ok(DatasetTableArtifact {
        version: 1,
        kind: "dataset-table".to_string(),
        table: table.name.clone(),
        source_key_fields: required_source_keys
            .get(&table.name)
            .map(|keys| keys.iter().cloned().collect())
            .unwrap_or_default(),
        records,
    })
}

fn table_output_file_name(table: &DatasetTableSpec) -> anyhow::Result<String> {
    let file = table
        .file
        .clone()
        .unwrap_or_else(|| format!("{}.json", table.name));
    validate_package_file_name(&file)?;
    Ok(file)
}

fn package_file_path(base_dir: &Path, file: &str) -> anyhow::Result<PathBuf> {
    validate_package_file_name(file)?;
    Ok(base_dir.join(file))
}

fn validate_package_file_name(file: &str) -> anyhow::Result<()> {
    let path = Path::new(file);
    if file.trim().is_empty() {
        anyhow::bail!("Dataset package file name must not be empty.");
    }
    if path.is_absolute() {
        anyhow::bail!("Dataset package file '{}' must be relative.", file);
    }
    if path.components().count() != 1
        || !path
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
    {
        anyhow::bail!(
            "Dataset package file '{}' must be a plain file name without directories or traversal.",
            file
        );
    }
    Ok(())
}

fn artifact_field_list(artifact: &DatasetTableArtifact) -> Option<Vec<String>> {
    let records = artifact
        .records
        .iter()
        .map(|record| record.data.clone())
        .collect::<Vec<_>>();
    let field_names = record_field_names(None, &records);
    if field_names.is_empty() {
        None
    } else {
        Some(field_names)
    }
}

fn manifest_table_map(
    tables: &[DatasetManifestTable],
) -> anyhow::Result<BTreeMap<String, DatasetManifestTable>> {
    let mut map = BTreeMap::new();
    for table in tables {
        if map.insert(table.name.clone(), table.clone()).is_some() {
            anyhow::bail!("Dataset manifest contains duplicate table '{}'", table.name);
        }
    }
    Ok(map)
}

fn dataset_base_dir(file: &str) -> PathBuf {
    Path::new(file)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let mut file = File::create(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn remap_reference_fields(
    record: &Record,
    references: &[DatasetReferenceSpec],
    source_to_target_ids: &HashMap<String, HashMap<String, HashMap<String, String>>>,
) -> anyhow::Result<Record> {
    let mut fields = record.fields.clone();
    for reference in references {
        let Some(value) = fields.get(&reference.field).cloned() else {
            continue;
        };
        if let Some(marker) = extract_reference_marker(Some(&value)) {
            let target_sys_id = source_to_target_ids
                .get(&marker.target_table)
                .and_then(|by_key| by_key.get(&marker.source_key))
                .and_then(|by_value| by_value.get(&marker.source_value))
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No remapped target sys_id found for reference '{}.{}' with source value '{}'",
                        marker.target_table,
                        reference.field,
                        marker.source_value
                    )
                })?;
            fields.insert(reference.field.clone(), serde_json::json!(target_sys_id));
        }
    }
    Ok(Record { fields })
}

fn ensure_json_output(format: &OutputFormat, command: &str) -> anyhow::Result<()> {
    if matches!(format, OutputFormat::Csv) {
        anyhow::bail!("`{}` currently supports only JSON output", command);
    }
    Ok(())
}

fn record_field_names(fields: Option<&[String]>, records: &[Record]) -> Vec<String> {
    let mut field_names = std::collections::BTreeSet::new();
    if let Some(fields) = fields {
        field_names.extend(fields.iter().cloned());
    }
    for record in records {
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

fn extract_reference_marker(value: Option<&serde_json::Value>) -> Option<ReferenceMarker> {
    let value = value?;
    serde_json::from_value::<ReferencePlaceholder>(value.clone())
        .ok()
        .map(|placeholder| placeholder.reference)
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
        OutputFormat::Jsonl => {
            output::write_jsonl_value(&serde_json::to_value(artifact)?, &mut file)?
        }
        OutputFormat::Toon => output::write_toon(artifact, &mut file)?,
        OutputFormat::Text => {
            serde_json::to_writer_pretty(&mut file, artifact)?;
            file.write_all(b"\n")?;
        }
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
        OutputFormat::Jsonl => "jsonl",
        OutputFormat::Toon => "toon",
        OutputFormat::Text => "json",
    }
}

fn long_running_timeout_secs(timeout_secs: Option<u64>) -> u64 {
    timeout_secs.unwrap_or(LONG_RUNNING_TIMEOUT_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_package_file_name_rejects_traversal_and_absolute_paths() {
        assert!(validate_package_file_name("incident.json").is_ok());
        assert!(validate_package_file_name("../secret.json").is_err());
        assert!(validate_package_file_name("nested/incident.json").is_err());
        assert!(validate_package_file_name("/tmp/incident.json").is_err());
        assert!(validate_package_file_name("").is_err());
    }

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
