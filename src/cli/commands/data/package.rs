use super::*;

pub(super) async fn handle_export_package(
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
        let table_spec_name: TableName = table_spec.name.parse()?;
        let records = client
            .get_table_records(
                &table_spec_name,
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

pub(super) async fn build_package_validation_report(
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

pub(super) async fn handle_import_package(
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
        let _: TableName = manifest_table.name.parse()?;
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

pub(super) async fn fetch_table_schema(
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

    let sys_dictionary = TableName::from_static("sys_dictionary");
    let records = client
        .get_table_records(
            &sys_dictionary,
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

pub(super) async fn fetch_table_hierarchy(
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

pub(super) async fn fetch_table_definition_by_name(
    client: &mut crate::client::SnowClient,
    table: &str,
) -> anyhow::Result<Option<TableDefinition>> {
    let query = format!("name={table}");
    fetch_table_definition(client, &query).await
}

pub(super) async fn fetch_table_definition_by_sys_id(
    client: &mut crate::client::SnowClient,
    sys_id: &str,
) -> anyhow::Result<Option<TableDefinition>> {
    let query = format!("sys_id={sys_id}");
    fetch_table_definition(client, &query).await
}

pub(super) async fn fetch_table_definition(
    client: &mut crate::client::SnowClient,
    query: &str,
) -> anyhow::Result<Option<TableDefinition>> {
    let pagination = PaginationConfig::default().with_limit(Some(1));
    let sys_db_object = TableName::from_static("sys_db_object");
    let records = client
        .get_table_records(
            &sys_db_object,
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

pub(super) fn read_dataset_input(file: &str) -> anyhow::Result<DatasetInput> {
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

pub(super) fn read_dataset_export_spec(file: &str) -> anyhow::Result<DatasetExportSpec> {
    let body = std::fs::read_to_string(file)?;
    let spec: DatasetExportSpec = serde_json::from_str(&body)
        .map_err(|error| anyhow::anyhow!("Invalid dataset export spec '{}': {}", file, error))?;
    Ok(spec)
}

pub(super) fn read_dataset_table_artifact(path: &Path) -> anyhow::Result<DatasetTableArtifact> {
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

pub(super) fn validate_dataset_export_spec(spec: &DatasetExportSpec) -> anyhow::Result<()> {
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

pub(super) fn topological_table_order(tables: &[DatasetTableSpec]) -> anyhow::Result<Vec<String>> {
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

pub(super) fn topological_manifest_order(
    tables: &[DatasetManifestTable],
) -> anyhow::Result<Vec<String>> {
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

pub(super) fn topo_sort_dependencies(
    deps: BTreeMap<String, BTreeSet<String>>,
) -> anyhow::Result<Vec<String>> {
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

pub(super) fn required_source_keys(
    tables: &[DatasetTableSpec],
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

pub(super) fn required_source_keys_from_manifest(
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

pub(super) fn package_fetch_fields(
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

pub(super) fn build_source_value_indexes(
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

pub(super) fn build_dataset_table_artifact(
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

pub(super) fn table_output_file_name(table: &DatasetTableSpec) -> anyhow::Result<String> {
    let file = table
        .file
        .clone()
        .unwrap_or_else(|| format!("{}.json", table.name));
    validate_package_file_name(&file)?;
    Ok(file)
}

pub(super) fn package_file_path(base_dir: &Path, file: &str) -> anyhow::Result<PathBuf> {
    validate_package_file_name(file)?;
    Ok(base_dir.join(file))
}

pub(super) fn validate_package_file_name(file: &str) -> anyhow::Result<()> {
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

pub(super) fn artifact_field_list(artifact: &DatasetTableArtifact) -> Option<Vec<String>> {
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

pub(super) fn manifest_table_map(
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

pub(super) fn dataset_base_dir(file: &str) -> PathBuf {
    Path::new(file)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub(super) fn write_json_file<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let mut file = File::create(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}

pub(super) fn remap_reference_fields(
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

pub(super) fn ensure_json_output(format: &OutputFormat, command: &str) -> anyhow::Result<()> {
    if matches!(format, OutputFormat::Csv) {
        anyhow::bail!("`{}` currently supports only JSON output", command);
    }
    Ok(())
}

pub(super) fn record_field_names(fields: Option<&[String]>, records: &[Record]) -> Vec<String> {
    let mut field_names = std::collections::BTreeSet::new();
    if let Some(fields) = fields {
        field_names.extend(fields.iter().cloned());
    }
    for record in records {
        field_names.extend(record.fields.keys().cloned());
    }
    field_names.into_iter().collect()
}

pub(super) fn is_system_managed_field(field_name: &str) -> bool {
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

pub(super) fn is_unsupported_field_type(internal_type: &str) -> bool {
    matches!(
        internal_type,
        "journal" | "journal_input" | "script" | "translated_html" | "password"
    )
}

pub(super) fn default_export_command() -> String {
    "data export".to_string()
}

pub(super) fn json_value_as_text(value: &serde_json::Value) -> Option<String> {
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

pub(super) fn json_value_as_bool(value: &serde_json::Value) -> bool {
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

pub(super) fn extract_reference_marker(
    value: Option<&serde_json::Value>,
) -> Option<ReferenceMarker> {
    let value = value?;
    serde_json::from_value::<ReferencePlaceholder>(value.clone())
        .ok()
        .map(|placeholder| placeholder.reference)
}

pub(super) fn is_not_found_error(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<crate::client::error::ApiError>()
        .map(|api_error| api_error.status == 404)
        .unwrap_or(false)
}

pub(super) fn write_export_file(
    artifact: &TableExportArtifact,
    format: &OutputFormat,
    out_path: &str,
) -> anyhow::Result<()> {
    let mut file = File::create(out_path)?;
    match format {
        // Auto is coerced to Json before export; this arm keeps the match total.
        OutputFormat::Json | OutputFormat::Auto => {
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

pub(super) fn split_csv_fields(fields: Option<&str>) -> Option<Vec<String>> {
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

pub(super) fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

pub(super) fn output_format_name(format: &OutputFormat) -> &'static str {
    match format {
        OutputFormat::Json => "json",
        OutputFormat::Csv => "csv",
        OutputFormat::Jsonl => "jsonl",
        OutputFormat::Toon => "toon",
        // Text and (export-coerced) Auto both serialize as JSON on disk.
        OutputFormat::Text | OutputFormat::Auto => "json",
    }
}

pub(super) fn long_running_timeout_secs(timeout_secs: Option<u64>) -> u64 {
    timeout_secs.unwrap_or(LONG_RUNNING_TIMEOUT_SECS)
}
