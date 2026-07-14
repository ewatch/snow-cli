use super::*;

pub(super) async fn handle_export(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    export: ExportRequest,
) -> anyhow::Result<()> {
    tracing::info!("Exporting records from table: {}", export.table);

    // Export artifacts are designed to be re-imported, and `data import` reads
    // JSON only. The token-efficient `auto` format therefore degrades to JSON
    // here so an export never produces an un-importable file. An explicit
    // `--output toon` still forces TOON for a user who really wants it.
    let coerced_export_format;
    let format = if matches!(format, OutputFormat::Auto) {
        coerced_export_format = OutputFormat::Json;
        &coerced_export_format
    } else {
        format
    };

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
        table: export.table.to_string(),
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
        // `format` is coerced away from Auto above; the arm keeps the match total.
        OutputFormat::Json | OutputFormat::Auto => output::print_output(&artifact, format),
        OutputFormat::Csv => output::print_records(&artifact.records, format),
        OutputFormat::Jsonl | OutputFormat::Toon => output::print_output(&artifact, format),
        OutputFormat::Text => output::print_output(&artifact, format),
    }
}

pub(super) async fn handle_validate(
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

pub(super) async fn handle_import(
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

pub(super) async fn build_flat_validation_report(
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

pub(super) async fn build_table_validation_report(
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

pub(super) async fn handle_import_flat(
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
    let _: TableName = artifact.table.parse()?;
    let use_import_set = options.import_set_table.is_some();
    let path = if let Some(staging_table) = options.import_set_table {
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
