use std::collections::{BTreeMap, HashSet};

use crate::cli::args::{OutputFormat, ScopeArgs, ScopeCommands, ScopeDetailLevel};
use crate::cli::output;
use crate::client::pagination::PaginationConfig;
use crate::models::record::Record;

struct ArtifactDefinition {
    artifact_type: &'static str,
    category: &'static str,
    table: &'static str,
    fields: &'static str,
    name_field: &'static str,
}

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

pub async fn handle(
    args: ScopeArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
) -> anyhow::Result<()> {
    match args.command {
        ScopeCommands::Inspect { scope, details } => {
            handle_inspect(profile, format, instance, &scope, details).await
        }
        ScopeCommands::Inventory { scope } => {
            handle_inventory(profile, format, instance, &scope).await
        }
    }
}

async fn handle_inspect(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    scope_input: &str,
    details: ScopeDetailLevel,
) -> anyhow::Result<()> {
    let collected = collect_scope_data(profile, instance, scope_input).await?;
    let rows = collected.to_inventory_rows();

    let payload = ScopeInspectOutput {
        scope: collected.scope,
        details: match details {
            ScopeDetailLevel::Basic => "basic".to_string(),
            ScopeDetailLevel::Full => "full".to_string(),
        },
        summary: collected.summary,
        artifacts: if matches!(details, ScopeDetailLevel::Full) {
            Some(rows)
        } else {
            None
        },
        warnings: collected.warnings,
    };

    match format {
        OutputFormat::Json => output::print_output(&payload, format),
        OutputFormat::Csv => {
            let csv_rows = payload
                .summary
                .to_csv_rows(&payload.scope.scope, &payload.scope.sys_id);
            output::print_list(&csv_rows, format)
        }
    }
}

async fn handle_inventory(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    scope_input: &str,
) -> anyhow::Result<()> {
    let collected = collect_scope_data(profile, instance, scope_input).await?;
    let rows = collected.to_inventory_rows();

    match format {
        OutputFormat::Json => {
            let payload = ScopeInventoryOutput {
                scope: collected.scope,
                summary: collected.summary,
                rows,
                warnings: collected.warnings,
            };
            output::print_output(&payload, format)
        }
        OutputFormat::Csv => output::print_list(&rows, format),
    }
}

async fn collect_scope_data(
    profile: &str,
    instance: Option<&str>,
    scope_input: &str,
) -> anyhow::Result<CollectedScopeData> {
    let mut client = crate::client::build_client(profile, instance)?;
    let pagination = PaginationConfig::default();

    let scope_query = format!("scope={scope_input}^ORsys_id={scope_input}");
    let scopes = client
        .get_table_records(
            "sys_scope",
            Some(&scope_query),
            Some("sys_id,scope,name,version"),
            &pagination,
            None,
        )
        .await?;

    let scope_record = scopes
        .first()
        .ok_or_else(|| anyhow::anyhow!("Scope '{scope_input}' was not found in sys_scope"))?;

    let scope = ScopeInfo {
        sys_id: field_text(scope_record, "sys_id"),
        scope: field_text(scope_record, "scope"),
        name: field_text(scope_record, "name"),
        version: field_text(scope_record, "version"),
    };

    let mut warnings = Vec::new();
    let mut artifact_sets = Vec::new();

    for definition in ARTIFACT_DEFINITIONS {
        let records = fetch_records_for_scope(
            &mut client,
            &scope.sys_id,
            definition.table,
            definition.fields,
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

    let dictionary = fetch_dictionary_records(&mut client, &table_name_refs, &mut warnings).await;
    artifact_sets.push(CollectedArtifactSet {
        category: "data_model_logic".to_string(),
        artifact_type: "dictionary_fields".to_string(),
        source_table: "sys_dictionary".to_string(),
        name_field: "element".to_string(),
        records: dictionary,
    });

    let choices = fetch_choice_records(&mut client, &table_name_refs, &mut warnings).await;
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

async fn fetch_records_for_scope(
    client: &mut crate::client::SnowClient,
    scope_sys_id: &str,
    table: &str,
    fields: &str,
    warnings: &mut Vec<String>,
) -> Vec<Record> {
    let query = format!("sys_scope={scope_sys_id}");
    let fields = format!("{fields},sys_scope");
    let pagination = PaginationConfig::default();
    match client
        .get_table_records(table, Some(&query), Some(&fields), &pagination, None)
        .await
    {
        Ok(records) => {
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

async fn fetch_dictionary_records(
    client: &mut crate::client::SnowClient,
    table_names: &[&str],
    warnings: &mut Vec<String>,
) -> Vec<Record> {
    if table_names.is_empty() {
        return Vec::new();
    }

    let query = build_dictionary_query(table_names);
    let pagination = PaginationConfig::default().with_page_size(200);

    match client
        .get_table_records(
            "sys_dictionary",
            Some(&query),
            Some("sys_id,name,element,column_label,internal_type,reference"),
            &pagination,
            None,
        )
        .await
    {
        Ok(records) => records,
        Err(err) => {
            warnings.push(format!("Failed to query sys_dictionary: {err}"));
            Vec::new()
        }
    }
}

async fn fetch_choice_records(
    client: &mut crate::client::SnowClient,
    table_names: &[&str],
    warnings: &mut Vec<String>,
) -> Vec<Record> {
    if table_names.is_empty() {
        return Vec::new();
    }

    let query = format!("nameIN{}", table_names.join(","));
    let pagination = PaginationConfig::default().with_page_size(200);

    match client
        .get_table_records(
            "sys_choice",
            Some(&query),
            Some("sys_id,name,element,value,label,inactive"),
            &pagination,
            None,
        )
        .await
    {
        Ok(records) => records,
        Err(err) => {
            warnings.push(format!("Failed to query sys_choice: {err}"));
            Vec::new()
        }
    }
}

async fn fetch_other_metadata_rows(
    client: &mut crate::client::SnowClient,
    scope: &str,
    scope_sys_id: &str,
    known_source_tables: &HashSet<String>,
    warnings: &mut Vec<String>,
) -> Vec<ScopeInventoryRow> {
    let query = format!("sys_scope={scope_sys_id}");
    let pagination = PaginationConfig::default();
    let metadata_records = match client
        .get_table_records(
            "sys_metadata",
            Some(&query),
            Some("sys_id,name,sys_class_name"),
            &pagination,
            None,
        )
        .await
    {
        Ok(records) => records,
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

fn build_dictionary_query(table_names: &[&str]) -> String {
    format!(
        "nameIN{}^elementISNOTEMPTY^element!=sys_tags",
        table_names.join(",")
    )
}

fn field_text(record: &Record, field: &str) -> String {
    record
        .fields
        .get(field)
        .and_then(value_as_text)
        .unwrap_or_default()
}

fn value_as_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Object(map) => map
            .get("value")
            .and_then(|inner| inner.as_str())
            .map(ToString::to_string),
        _ => None,
    }
}

#[derive(Debug, serde::Serialize)]
struct ScopeInspectOutput {
    scope: ScopeInfo,
    details: String,
    summary: ScopeSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    artifacts: Option<Vec<ScopeInventoryRow>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
struct ScopeInfo {
    sys_id: String,
    scope: String,
    name: String,
    version: String,
}

#[derive(Debug, serde::Serialize)]
struct ScopeSummary {
    total_artifacts: usize,
    artifact_counts: BTreeMap<String, usize>,
    category_counts: BTreeMap<String, usize>,
}

impl ScopeSummary {
    fn from_rows(rows: &[ScopeInventoryRow]) -> Self {
        let mut artifact_counts = BTreeMap::new();
        let mut category_counts = BTreeMap::new();

        for row in rows {
            *artifact_counts
                .entry(row.artifact_type.clone())
                .or_insert(0) += 1;
            *category_counts.entry(row.category.clone()).or_insert(0) += 1;
        }

        Self {
            total_artifacts: rows.len(),
            artifact_counts,
            category_counts,
        }
    }

    fn to_csv_rows(&self, scope: &str, scope_sys_id: &str) -> Vec<ScopeInspectCsvRow> {
        self.artifact_counts
            .iter()
            .map(|(artifact, count)| ScopeInspectCsvRow {
                scope: scope.to_string(),
                scope_sys_id: scope_sys_id.to_string(),
                artifact: artifact.clone(),
                count: *count,
            })
            .collect()
    }
}

#[derive(Debug, serde::Serialize)]
struct ScopeInspectCsvRow {
    scope: String,
    scope_sys_id: String,
    artifact: String,
    count: usize,
}

#[derive(Debug, serde::Serialize)]
struct ScopeInventoryOutput {
    scope: ScopeInfo,
    summary: ScopeSummary,
    rows: Vec<ScopeInventoryRow>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct ScopeInventoryRow {
    scope: String,
    scope_sys_id: String,
    category: String,
    artifact_type: String,
    source_table: String,
    sys_id: String,
    name: String,
}

struct CollectedArtifactSet {
    category: String,
    artifact_type: String,
    source_table: String,
    name_field: String,
    records: Vec<Record>,
}

struct CollectedScopeData {
    scope: ScopeInfo,
    summary: ScopeSummary,
    artifact_sets: Vec<CollectedArtifactSet>,
    other_rows: Vec<ScopeInventoryRow>,
    warnings: Vec<String>,
}

impl CollectedScopeData {
    fn to_inventory_rows(&self) -> Vec<ScopeInventoryRow> {
        let mut rows = Vec::new();
        for set in &self.artifact_sets {
            rows.extend(map_inventory_rows(
                &self.scope,
                &set.category,
                &set.artifact_type,
                &set.source_table,
                &set.records,
                &set.name_field,
            ));
        }
        rows.extend(self.other_rows.clone());
        rows
    }
}

fn map_inventory_rows(
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
mod tests {
    use super::*;

    #[test]
    fn test_build_dictionary_query() {
        let query = build_dictionary_query(&["x_app_table", "x_app_second"]);
        assert_eq!(
            query,
            "nameINx_app_table,x_app_second^elementISNOTEMPTY^element!=sys_tags"
        );
    }

    #[test]
    fn test_value_as_text_from_reference_object() {
        let value = serde_json::json!({"link": "https://example", "value": "abc123"});
        assert_eq!(value_as_text(&value), Some("abc123".to_string()));
    }

    #[test]
    fn test_scope_summary_counts_by_category_and_artifact_type() {
        let rows = vec![
            ScopeInventoryRow {
                scope: "x_app".to_string(),
                scope_sys_id: "id".to_string(),
                category: "server_logic".to_string(),
                artifact_type: "script_includes".to_string(),
                source_table: "sys_script_include".to_string(),
                sys_id: "1".to_string(),
                name: "SI1".to_string(),
            },
            ScopeInventoryRow {
                scope: "x_app".to_string(),
                scope_sys_id: "id".to_string(),
                category: "other".to_string(),
                artifact_type: "other".to_string(),
                source_table: "x_custom_meta".to_string(),
                sys_id: "2".to_string(),
                name: "Meta".to_string(),
            },
        ];

        let summary = ScopeSummary::from_rows(&rows);
        assert_eq!(summary.total_artifacts, 2);
        assert_eq!(summary.category_counts.get("server_logic"), Some(&1));
        assert_eq!(summary.category_counts.get("other"), Some(&1));
        assert_eq!(summary.artifact_counts.get("script_includes"), Some(&1));
        assert_eq!(summary.artifact_counts.get("other"), Some(&1));
    }
}
