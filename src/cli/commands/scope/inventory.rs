use super::*;

const ARTIFACT_DEFINITIONS: &[ArtifactDefinition] = &[
    ArtifactDefinition {
        artifact_type: "tables",
        category: "data_model_logic",
        table: "sys_db_object",
        fields: "sys_id,name,label,super_class",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "script_includes",
        category: "server_logic",
        table: "sys_script_include",
        fields: "sys_id,name,api_name,active,client_callable",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "business_rules",
        category: "server_logic",
        table: "sys_script",
        fields: "sys_id,name,collection,when,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "scheduled_scripts",
        category: "server_logic",
        table: "sysauto_script",
        fields: "sys_id,name,run_type,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "processors",
        category: "server_logic",
        table: "sys_processor",
        fields: "sys_id,name,type,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "transform_maps",
        category: "server_logic",
        table: "sys_transform_map",
        fields: "sys_id,name,target_table,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "transform_entries",
        category: "server_logic",
        table: "sys_transform_entry",
        fields: "sys_id,target_field,source_field,map",
        name_field: "target_field",
    },
    ArtifactDefinition {
        artifact_type: "transform_scripts",
        category: "server_logic",
        table: "sys_transform_script",
        fields: "sys_id,name,map,when,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "client_scripts",
        category: "client_logic",
        table: "sys_script_client",
        fields: "sys_id,name,table,type,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "ui_actions",
        category: "client_logic",
        table: "sys_ui_action",
        fields: "sys_id,name,table,action_name,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "ui_pages",
        category: "client_logic",
        table: "sys_ui_page",
        fields: "sys_id,name,category,sys_name",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "ui_policies",
        category: "client_logic",
        table: "sys_ui_policy",
        fields: "sys_id,short_description,table,active",
        name_field: "short_description",
    },
    ArtifactDefinition {
        artifact_type: "ui_policy_actions",
        category: "client_logic",
        table: "sys_ui_policy_action",
        fields: "sys_id,ui_policy,field,mandatory,visible,read_only",
        name_field: "field",
    },
    ArtifactDefinition {
        artifact_type: "flows",
        category: "flow_logic",
        table: "sys_hub_flow",
        fields: "sys_id,name,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "flow_actions",
        category: "flow_logic",
        table: "sys_hub_action_type_definition",
        fields: "sys_id,name,scope,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "flow_action_instances",
        category: "flow_logic",
        table: "sys_hub_action_instance",
        fields: "sys_id,name,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "flow_versions",
        category: "flow_logic",
        table: "sys_hub_flow_version",
        fields: "sys_id,name,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "flow_trigger_definitions",
        category: "flow_logic",
        table: "sys_hub_trigger_definition",
        fields: "sys_id,name,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "scripted_rest_apis",
        category: "integration_logic",
        table: "sys_ws_definition",
        fields: "sys_id,name,base_api_path,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "scripted_rest_operations",
        category: "integration_logic",
        table: "sys_ws_operation",
        fields: "sys_id,name,http_method,relative_path,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "rest_messages",
        category: "integration_logic",
        table: "sys_rest_message",
        fields: "sys_id,name,rest_endpoint,authentication_type",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "rest_message_functions",
        category: "integration_logic",
        table: "sys_rest_message_fn",
        fields: "sys_id,function_name,http_method,rest_endpoint",
        name_field: "function_name",
    },
    ArtifactDefinition {
        artifact_type: "acls",
        category: "security_logic",
        table: "sys_security_acl",
        fields: "sys_id,name,operation,type,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "acl_roles",
        category: "security_logic",
        table: "sys_security_acl_role",
        fields: "sys_id,sys_security_acl,sys_user_role",
        name_field: "sys_user_role",
    },
    ArtifactDefinition {
        artifact_type: "scope_privileges",
        category: "security_logic",
        table: "sys_scope_privilege",
        fields: "sys_id,target_scope,target_name,operation,status",
        name_field: "target_name",
    },
    ArtifactDefinition {
        artifact_type: "event_registrations",
        category: "event_notification_logic",
        table: "sysevent_register",
        fields: "sys_id,event_name,description,fired_by",
        name_field: "event_name",
    },
    ArtifactDefinition {
        artifact_type: "email_notifications",
        category: "event_notification_logic",
        table: "sysevent_email_action",
        fields: "sys_id,name,event_name,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "properties",
        category: "data_model_logic",
        table: "sys_properties",
        fields: "sys_id,name,type,description",
        name_field: "name",
    },
];

/// Default per-table cap applied when enumerating scope artifacts without an
/// explicit `--limit`. Scope inventory targets bounded custom application
/// scopes; this keeps a mis-targeted run from paginating through an entire
/// instance before failing.
const DEFAULT_ENUMERATION_LIMIT: usize = 5_000;

/// Maximum number of table names packed into a single `sys_dictionary` or
/// `sys_choice` `nameIN` query. Chunking keeps the request URL bounded on
/// scopes that define many tables.
const DICTIONARY_QUERY_CHUNK: usize = 50;

/// Whether a scope is the platform (`global`) scope, whose artifacts span the
/// entire base instance and cannot be meaningfully enumerated in full.
fn is_platform_scope(scope: &str) -> bool {
    scope == "global"
}

/// Source tables already accounted for by a dedicated artifact type. Used to
/// classify everything else in `sys_metadata` as `other`.
fn known_metadata_source_tables() -> Vec<&'static str> {
    let mut tables: Vec<&'static str> = ARTIFACT_DEFINITIONS.iter().map(|def| def.table).collect();
    tables.push("sys_dictionary");
    tables.push("sys_choice");
    tables
}

pub(super) async fn handle_inspect(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    scope_input: &EncodedQueryValue,
    details: ScopeDetailLevel,
    limit: Option<usize>,
) -> anyhow::Result<()> {
    let payload = match details {
        // `basic` needs only counts, so tally them via `X-Total-Count`
        // instead of downloading every matching record. This keeps inspect
        // cheap and reliable even for platform-scale scopes like `global`.
        ScopeDetailLevel::Basic => {
            let counted =
                collect_scope_summary(profile, instance, timeout_secs, scope_input).await?;
            ScopeInspectOutput {
                scope: counted.scope,
                details: "basic".to_string(),
                summary: counted.summary,
                artifacts: None,
                warnings: counted.warnings,
            }
        }
        // `full` lists the artifacts themselves, so it must enumerate them
        // under the same bounds and guards as `inventory`.
        ScopeDetailLevel::Full => {
            let collected =
                collect_scope_data(profile, instance, timeout_secs, scope_input, limit).await?;
            let rows = collected.to_inventory_rows();
            ScopeInspectOutput {
                scope: collected.scope,
                details: "full".to_string(),
                summary: collected.summary,
                artifacts: Some(rows),
                warnings: collected.warnings,
            }
        }
    };

    match format {
        OutputFormat::Json | OutputFormat::Jsonl | OutputFormat::Toon | OutputFormat::Auto => {
            output::print_output(&payload, format)
        }
        OutputFormat::Csv => {
            let csv_rows = payload
                .summary
                .to_csv_rows(&payload.scope.scope, &payload.scope.sys_id);
            output::print_list(&csv_rows, format)
        }
        OutputFormat::Text => output::print_output(&payload, format),
    }
}

pub(super) async fn handle_inventory(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    scope_input: &EncodedQueryValue,
    limit: Option<usize>,
) -> anyhow::Result<()> {
    let collected = collect_scope_data(profile, instance, timeout_secs, scope_input, limit).await?;
    let rows = collected.to_inventory_rows();

    match format {
        OutputFormat::Json | OutputFormat::Jsonl | OutputFormat::Toon | OutputFormat::Auto => {
            let payload = ScopeInventoryOutput {
                scope: collected.scope,
                summary: collected.summary,
                rows,
                warnings: collected.warnings,
            };
            output::print_output(&payload, format)
        }
        OutputFormat::Csv => output::print_list(&rows, format),
        OutputFormat::Text => {
            let payload = ScopeInventoryOutput {
                scope: collected.scope,
                summary: collected.summary,
                rows,
                warnings: collected.warnings,
            };
            output::print_output(&payload, format)
        }
    }
}
/// Scope plus artifact counts, produced without enumerating records.
pub(super) struct CollectedScopeSummary {
    pub(super) scope: ScopeInfo,
    pub(super) summary: ScopeSummary,
    pub(super) warnings: Vec<String>,
}

/// Tallies artifact counts into a [`ScopeSummary`]. Only artifact types with
/// at least one record are recorded, matching [`ScopeSummary::from_rows`].
#[derive(Default)]
struct CountAccumulator {
    total: usize,
    artifact_counts: BTreeMap<String, usize>,
    category_counts: BTreeMap<String, usize>,
}

impl CountAccumulator {
    fn add(&mut self, category: &str, artifact_type: &str, count: usize) {
        if count == 0 {
            return;
        }
        self.total += count;
        *self
            .artifact_counts
            .entry(artifact_type.to_string())
            .or_insert(0) += count;
        *self
            .category_counts
            .entry(category.to_string())
            .or_insert(0) += count;
    }

    fn into_summary(self) -> ScopeSummary {
        ScopeSummary {
            total_artifacts: self.total,
            artifact_counts: self.artifact_counts,
            category_counts: self.category_counts,
        }
    }
}

/// Resolve a scope name or sys_id to its `sys_scope` record.
pub(super) async fn resolve_scope(
    client: &mut crate::client::SnowClient,
    scope_input: &EncodedQueryValue,
) -> anyhow::Result<ScopeInfo> {
    let scope_query = format!("scope={scope_input}^ORsys_id={scope_input}");
    let sys_scope = TableName::from_static("sys_scope");
    let scopes = client
        .get_table_records(
            &sys_scope,
            Some(&scope_query),
            Some("sys_id,scope,name,version"),
            &PaginationConfig::default(),
            None,
        )
        .await?;

    let scope_record = scopes
        .first()
        .ok_or_else(|| anyhow::anyhow!("Scope '{scope_input}' was not found in sys_scope"))?;

    Ok(ScopeInfo {
        sys_id: field_text(scope_record, "sys_id"),
        scope: field_text(scope_record, "scope"),
        name: field_text(scope_record, "name"),
        version: field_text(scope_record, "version"),
    })
}

/// Count scope artifacts using the Table API's `X-Total-Count` header, without
/// downloading the records. This is what makes `inspect` viable for the
/// `global` scope, whose artifacts span the entire base instance.
///
/// Counts trust the server-side `sys_scope` filter; unlike full enumeration
/// they cannot re-validate each record's `sys_scope` field, so a table that
/// silently ignores the filter would be over-counted. That trade-off is what
/// keeps the operation bounded.
pub(super) async fn collect_scope_summary(
    profile: &str,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    scope_input: &EncodedQueryValue,
) -> anyhow::Result<CollectedScopeSummary> {
    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
    let scope = resolve_scope(&mut client, scope_input).await?;
    let platform = is_platform_scope(&scope.scope);

    let mut warnings = Vec::new();
    let mut acc = CountAccumulator::default();

    for definition in ARTIFACT_DEFINITIONS {
        let count =
            count_scope_records(&mut client, &scope.sys_id, definition.table, &mut warnings).await;
        acc.add(definition.category, definition.artifact_type, count);
    }

    // "other" = sys_metadata rows in the scope not already backed by a
    // dedicated artifact type, counted exactly with a single `NOT IN` query.
    let known = known_metadata_source_tables().join(",");
    let other_query = format!("sys_scope={}^sys_class_nameNOT IN{}", scope.sys_id, known);
    let sys_metadata = TableName::from_static("sys_metadata");
    match client
        .count_table_records(&sys_metadata, Some(&other_query))
        .await
    {
        Ok(Some(count)) => acc.add("other", "other", count),
        Ok(None) => warnings.push(
            "Could not count other sys_metadata artifacts: instance did not report X-Total-Count"
                .to_string(),
        ),
        Err(err) => warnings.push(format!(
            "Failed to count other sys_metadata artifacts: {err}"
        )),
    }

    // Dictionary and choice counts require the scope's table names, which
    // makes them unbounded for the platform scope. Skip them there.
    if platform {
        warnings.push(format!(
            "Skipped dictionary and choice counts for platform scope '{}' to avoid unbounded queries; primary artifact counts remain exact",
            scope.scope
        ));
    } else {
        let table_names = fetch_scope_table_names(&mut client, &scope.sys_id, &mut warnings).await;
        if !table_names.is_empty() {
            let refs = table_names.iter().map(String::as_str).collect::<Vec<_>>();
            let dictionary = count_dictionary_fields(&mut client, &refs, &mut warnings).await;
            acc.add("data_model_logic", "dictionary_fields", dictionary);
            let choices = count_choice_records(&mut client, &refs, &mut warnings).await;
            acc.add("data_model_logic", "choices", choices);
        }
    }

    Ok(CollectedScopeSummary {
        scope,
        summary: acc.into_summary(),
        warnings,
    })
}

pub(super) async fn collect_scope_data(
    profile: &str,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    scope_input: &EncodedQueryValue,
    limit: Option<usize>,
) -> anyhow::Result<CollectedScopeData> {
    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
    let scope = resolve_scope(&mut client, scope_input).await?;

    // Enumerating every artifact in the platform scope means downloading much
    // of the instance. Require an explicit cap rather than trying.
    if is_platform_scope(&scope.scope) && limit.is_none() {
        anyhow::bail!(
            "Refusing to enumerate platform scope '{}' unbounded; pass --limit N to cap records per table",
            scope.scope
        );
    }
    let per_table_limit = Some(limit.unwrap_or(DEFAULT_ENUMERATION_LIMIT));

    let mut warnings = Vec::new();
    let mut artifact_sets = Vec::new();

    for definition in ARTIFACT_DEFINITIONS {
        let records = fetch_records_for_scope(
            &mut client,
            &scope.sys_id,
            definition.table,
            definition.fields,
            per_table_limit,
            &mut warnings,
        )
        .await;

        artifact_sets.push(CollectedArtifactSet {
            category: definition.category.to_string(),
            artifact_type: definition.artifact_type.to_string(),
            source_table: definition.table.to_string(),
            name_field: definition.name_field.to_string(),
            records,
        });
    }

    let table_names = artifact_sets
        .iter()
        .find(|set| set.artifact_type == "tables")
        .map(|set| {
            set.records
                .iter()
                .filter_map(|record| record.get_str("name").map(ToString::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let table_name_refs = table_names.iter().map(String::as_str).collect::<Vec<_>>();

    let dictionary = fetch_dictionary_records(
        &mut client,
        &table_name_refs,
        per_table_limit,
        &mut warnings,
    )
    .await;
    artifact_sets.push(CollectedArtifactSet {
        category: "data_model_logic".to_string(),
        artifact_type: "dictionary_fields".to_string(),
        source_table: "sys_dictionary".to_string(),
        name_field: "element".to_string(),
        records: dictionary,
    });

    let choices = fetch_choice_records(
        &mut client,
        &table_name_refs,
        per_table_limit,
        &mut warnings,
    )
    .await;
    artifact_sets.push(CollectedArtifactSet {
        category: "data_model_logic".to_string(),
        artifact_type: "choices".to_string(),
        source_table: "sys_choice".to_string(),
        name_field: "label".to_string(),
        records: choices,
    });

    let known_source_tables = artifact_sets
        .iter()
        .map(|set| set.source_table.clone())
        .collect::<HashSet<_>>();

    let other_rows = fetch_other_metadata_rows(
        &mut client,
        &scope.scope,
        &scope.sys_id,
        &known_source_tables,
        per_table_limit,
        &mut warnings,
    )
    .await;

    let mut data = CollectedScopeData {
        scope,
        summary: ScopeSummary {
            total_artifacts: 0,
            artifact_counts: BTreeMap::new(),
            category_counts: BTreeMap::new(),
        },
        artifact_sets,
        other_rows,
        warnings,
    };

    let rows = data.to_inventory_rows();
    data.summary = ScopeSummary::from_rows(&rows);

    Ok(data)
}

/// Remaining records allowed before hitting `limit`, or `None` when unbounded.
fn remaining_budget(limit: Option<usize>, have: usize) -> Option<usize> {
    limit.map(|lim| lim.saturating_sub(have))
}

/// Keep only syntactically valid table names, warning about the rest.
fn valid_table_names<'a>(
    table_names: &[&'a str],
    target: &str,
    warnings: &mut Vec<String>,
) -> Vec<&'a str> {
    table_names
        .iter()
        .filter(|name| match name.parse::<TableName>() {
            Ok(_) => true,
            Err(err) => {
                warnings.push(format!(
                    "Skipped {target} query for invalid table name '{name}': {err}"
                ));
                false
            }
        })
        .copied()
        .collect()
}

async fn count_scope_records(
    client: &mut crate::client::SnowClient,
    scope_sys_id: &str,
    table: &'static str,
    warnings: &mut Vec<String>,
) -> usize {
    let query = format!("sys_scope={scope_sys_id}");
    let table_name = TableName::from_static(table);
    match client.count_table_records(&table_name, Some(&query)).await {
        Ok(Some(count)) => count,
        Ok(None) => {
            warnings.push(format!(
                "Could not count {table}: instance did not report X-Total-Count"
            ));
            0
        }
        Err(err) => {
            warnings.push(format!("Failed to count {table}: {err}"));
            0
        }
    }
}

async fn fetch_scope_table_names(
    client: &mut crate::client::SnowClient,
    scope_sys_id: &str,
    warnings: &mut Vec<String>,
) -> Vec<String> {
    let records = fetch_records_for_scope(
        client,
        scope_sys_id,
        "sys_db_object",
        "sys_id,name,label,super_class",
        Some(DEFAULT_ENUMERATION_LIMIT),
        warnings,
    )
    .await;
    records
        .iter()
        .filter_map(|record| record.get_str("name").map(ToString::to_string))
        .collect()
}

async fn count_dictionary_fields(
    client: &mut crate::client::SnowClient,
    table_names: &[&str],
    warnings: &mut Vec<String>,
) -> usize {
    let names = valid_table_names(table_names, "sys_dictionary", warnings);
    let sys_dictionary = TableName::from_static("sys_dictionary");
    let mut total = 0;
    for chunk in names.chunks(DICTIONARY_QUERY_CHUNK) {
        let query = build_dictionary_query(chunk);
        match client
            .count_table_records(&sys_dictionary, Some(&query))
            .await
        {
            Ok(Some(count)) => total += count,
            Ok(None) => warnings.push(
                "Could not count sys_dictionary: instance did not report X-Total-Count".to_string(),
            ),
            Err(err) => warnings.push(format!("Failed to count sys_dictionary: {err}")),
        }
    }
    total
}

async fn count_choice_records(
    client: &mut crate::client::SnowClient,
    table_names: &[&str],
    warnings: &mut Vec<String>,
) -> usize {
    let names = valid_table_names(table_names, "sys_choice", warnings);
    let sys_choice = TableName::from_static("sys_choice");
    let mut total = 0;
    for chunk in names.chunks(DICTIONARY_QUERY_CHUNK) {
        let query = format!("nameIN{}", chunk.join(","));
        match client.count_table_records(&sys_choice, Some(&query)).await {
            Ok(Some(count)) => total += count,
            Ok(None) => warnings.push(
                "Could not count sys_choice: instance did not report X-Total-Count".to_string(),
            ),
            Err(err) => warnings.push(format!("Failed to count sys_choice: {err}")),
        }
    }
    total
}

pub(super) async fn fetch_records_for_scope(
    client: &mut crate::client::SnowClient,
    scope_sys_id: &str,
    table: &'static str,
    fields: &str,
    limit: Option<usize>,
    warnings: &mut Vec<String>,
) -> Vec<Record> {
    let query = format!("sys_scope={scope_sys_id}");
    let fields = format!("{fields},sys_scope");
    let pagination = PaginationConfig::default().with_limit(limit);
    let table_name = TableName::from_static(table);
    match client
        .get_table_records_with_meta(&table_name, Some(&query), Some(&fields), &pagination, None)
        .await
    {
        Ok(result) => {
            if result.truncated {
                warnings.push(truncation_warning(
                    table,
                    result.records.len(),
                    result.total,
                ));
            }
            let records = result.records;
            if records.is_empty() {
                return records;
            }

            let has_scope_field = records
                .iter()
                .any(|record| record.fields.contains_key("sys_scope"));
            if !has_scope_field {
                warnings.push(format!(
                    "Skipped {table}: records returned without sys_scope field, cannot verify scope-safe filtering"
                ));
                return Vec::new();
            }

            let original_count = records.len();
            let filtered = records
                .into_iter()
                .filter(|record| field_text(record, "sys_scope") == scope_sys_id)
                .collect::<Vec<_>>();

            if filtered.len() != original_count {
                warnings.push(format!(
                    "Filtered {table} from {original_count} to {} records after sys_scope validation",
                    filtered.len()
                ));
            }

            filtered
        }
        Err(err) => {
            warnings.push(format!("Failed to query {table}: {err}"));
            Vec::new()
        }
    }
}

pub(super) async fn fetch_dictionary_records(
    client: &mut crate::client::SnowClient,
    table_names: &[&str],
    limit: Option<usize>,
    warnings: &mut Vec<String>,
) -> Vec<Record> {
    let names = valid_table_names(table_names, "sys_dictionary", warnings);
    if names.is_empty() {
        return Vec::new();
    }

    let sys_dictionary = TableName::from_static("sys_dictionary");
    let mut records = Vec::new();
    for chunk in names.chunks(DICTIONARY_QUERY_CHUNK) {
        let remaining = remaining_budget(limit, records.len());
        if remaining == Some(0) {
            break;
        }
        let query = build_dictionary_query(chunk);
        let pagination = PaginationConfig::default()
            .with_page_size(200)
            .with_limit(remaining);
        match client
            .get_table_records_with_meta(
                &sys_dictionary,
                Some(&query),
                Some("sys_id,name,element,column_label,internal_type,reference"),
                &pagination,
                None,
            )
            .await
        {
            Ok(result) => {
                if result.truncated {
                    warnings.push(truncation_warning(
                        "sys_dictionary",
                        records.len() + result.records.len(),
                        result.total,
                    ));
                }
                records.extend(result.records);
            }
            Err(err) => warnings.push(format!("Failed to query sys_dictionary: {err}")),
        }
    }
    records
}

pub(super) async fn fetch_choice_records(
    client: &mut crate::client::SnowClient,
    table_names: &[&str],
    limit: Option<usize>,
    warnings: &mut Vec<String>,
) -> Vec<Record> {
    let names = valid_table_names(table_names, "sys_choice", warnings);
    if names.is_empty() {
        return Vec::new();
    }

    let sys_choice = TableName::from_static("sys_choice");
    let mut records = Vec::new();
    for chunk in names.chunks(DICTIONARY_QUERY_CHUNK) {
        let remaining = remaining_budget(limit, records.len());
        if remaining == Some(0) {
            break;
        }
        let query = format!("nameIN{}", chunk.join(","));
        let pagination = PaginationConfig::default()
            .with_page_size(200)
            .with_limit(remaining);
        match client
            .get_table_records_with_meta(
                &sys_choice,
                Some(&query),
                Some("sys_id,name,element,value,label,inactive"),
                &pagination,
                None,
            )
            .await
        {
            Ok(result) => {
                if result.truncated {
                    warnings.push(truncation_warning(
                        "sys_choice",
                        records.len() + result.records.len(),
                        result.total,
                    ));
                }
                records.extend(result.records);
            }
            Err(err) => warnings.push(format!("Failed to query sys_choice: {err}")),
        }
    }
    records
}

pub(super) async fn fetch_other_metadata_rows(
    client: &mut crate::client::SnowClient,
    scope: &str,
    scope_sys_id: &str,
    known_source_tables: &HashSet<String>,
    limit: Option<usize>,
    warnings: &mut Vec<String>,
) -> Vec<ScopeInventoryRow> {
    let query = format!("sys_scope={scope_sys_id}");
    let pagination = PaginationConfig::default().with_limit(limit);
    let sys_metadata = TableName::from_static("sys_metadata");
    let metadata_records = match client
        .get_table_records_with_meta(
            &sys_metadata,
            Some(&query),
            Some("sys_id,name,sys_class_name"),
            &pagination,
            None,
        )
        .await
    {
        Ok(result) => {
            if result.truncated {
                warnings.push(truncation_warning(
                    "sys_metadata",
                    result.records.len(),
                    result.total,
                ));
            }
            result.records
        }
        Err(err) => {
            warnings.push(format!(
                "Failed to query sys_metadata for other artifacts: {err}"
            ));
            return Vec::new();
        }
    };

    metadata_records
        .iter()
        .filter_map(|record| {
            let class_name = field_text(record, "sys_class_name");
            if !class_name.is_empty() && known_source_tables.contains(&class_name) {
                return None;
            }

            let source_table = if class_name.is_empty() {
                "unknown".to_string()
            } else {
                class_name
            };

            Some(ScopeInventoryRow {
                scope: scope.to_string(),
                scope_sys_id: scope_sys_id.to_string(),
                category: "other".to_string(),
                artifact_type: "other".to_string(),
                source_table,
                sys_id: field_text(record, "sys_id"),
                name: field_text(record, "name"),
            })
        })
        .collect()
}

/// Warning shown when a per-table cap prevented fetching every matching row.
pub(super) fn truncation_warning(table: &str, returned: usize, total: Option<usize>) -> String {
    match total {
        Some(total) => format!(
            "Truncated {table} at {returned} of {total} records; pass --limit to raise the cap"
        ),
        None => format!("Truncated {table} at {returned} records; pass --limit to raise the cap"),
    }
}

pub(super) fn build_dictionary_query(table_names: &[&str]) -> String {
    format!(
        "nameIN{}^elementISNOTEMPTY^element!=sys_tags",
        table_names.join(",")
    )
}

pub(super) fn field_text(record: &Record, field: &str) -> String {
    record
        .fields
        .get(field)
        .and_then(value_as_text)
        .unwrap_or_default()
}

pub(super) fn value_as_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Object(map) => map
            .get("value")
            .and_then(|inner| inner.as_str())
            .map(ToString::to_string),
        _ => None,
    }
}
pub(super) fn map_inventory_rows(
    scope: &ScopeInfo,
    category: &str,
    artifact_type: &str,
    source_table: &str,
    records: &[Record],
    name_field: &str,
) -> Vec<ScopeInventoryRow> {
    records
        .iter()
        .map(|record| ScopeInventoryRow {
            scope: scope.scope.clone(),
            scope_sys_id: scope.sys_id.clone(),
            category: category.to_string(),
            artifact_type: artifact_type.to_string(),
            source_table: source_table.to_string(),
            sys_id: field_text(record, "sys_id"),
            name: field_text(record, name_field),
        })
        .collect()
}

#[cfg(test)]
mod count_tests {
    use super::*;

    #[test]
    fn is_platform_scope_only_matches_global() {
        assert!(is_platform_scope("global"));
        assert!(!is_platform_scope("x_my_app"));
        assert!(!is_platform_scope("sn_ot_incident_mgmt"));
    }

    #[test]
    fn known_metadata_source_tables_covers_definitions_and_derived() {
        let tables = known_metadata_source_tables();
        assert!(tables.contains(&"sys_script_include"));
        assert!(tables.contains(&"sys_dictionary"));
        assert!(tables.contains(&"sys_choice"));
        assert_eq!(tables.len(), ARTIFACT_DEFINITIONS.len() + 2);
    }

    #[test]
    fn count_accumulator_sums_and_skips_zero() {
        let mut acc = CountAccumulator::default();
        acc.add("server_logic", "script_includes", 3);
        acc.add("server_logic", "business_rules", 0); // skipped
        acc.add("client_logic", "ui_actions", 2);
        let summary = acc.into_summary();

        assert_eq!(summary.total_artifacts, 5);
        assert_eq!(summary.artifact_counts.get("script_includes"), Some(&3));
        assert_eq!(summary.artifact_counts.get("business_rules"), None);
        assert_eq!(summary.category_counts.get("server_logic"), Some(&3));
        assert_eq!(summary.category_counts.get("client_logic"), Some(&2));
    }

    #[test]
    fn remaining_budget_saturates_and_passes_through_none() {
        assert_eq!(remaining_budget(None, 10), None);
        assert_eq!(remaining_budget(Some(5), 2), Some(3));
        assert_eq!(remaining_budget(Some(5), 8), Some(0));
    }

    #[test]
    fn truncation_warning_includes_total_when_known() {
        assert_eq!(
            truncation_warning("sys_script", 5000, Some(12000)),
            "Truncated sys_script at 5000 of 12000 records; pass --limit to raise the cap"
        );
        assert_eq!(
            truncation_warning("sys_script", 5000, None),
            "Truncated sys_script at 5000 records; pass --limit to raise the cap"
        );
    }
}
