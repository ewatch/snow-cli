//! `flow show`: a Flow Designer flow as a compact, ordered step tree.
//!
//! Reads `sys_hub_flow` and its component tables through the ordinary Table
//! API client with dot-walked names (no display-value round trips), and
//! builds the tree with [`crate::models::flow`]. Read-only.

use serde::Serialize;

use crate::cli::args::{FlowArgs, FlowCommands, OutputFormat};
use crate::cli::output;
use crate::client::SnowClient;
use crate::client::pagination::PaginationConfig;
use crate::error::CliError;
use crate::models::flow::{self, Component, Step, StepKind, Structure};
use crate::models::identifiers::{RecordKey, TableName};
use crate::models::record::Record;
use crate::models::validation::{ValidationError, ValidationIssue};

/// Candidates listed when a name matches several flows.
const MAX_CANDIDATES: usize = 5;

/// Component rows read per table. Large OOB flows have a few dozen steps;
/// the bound keeps one pathological flow from paging indefinitely.
const MAX_COMPONENT_ROWS: usize = 1000;

/// Description characters echoed in the output.
const MAX_DESCRIPTION_CHARS: usize = 300;

const FLOW_FIELDS: &str =
    "sys_id,name,internal_name,type,status,active,sys_scope.scope,description,latest_snapshot";
const TRIGGER_FIELDS: &str = "sys_id,flow,name,trigger_type";
const COMPONENT_FIELDS: &str = "sys_id,flow,ui_id,parent_ui_id,order,comment";

/// Variable values shown as labels: the If/Else condition label and the
/// record trigger's table and condition.
const LABEL_ELEMENTS: [&str; 3] = ["condition_name", "table", "condition"];

/// One component table family (v1 or `_v2`).
struct ComponentTables {
    trigger: &'static str,
    logic: &'static str,
    action: &'static str,
    subflow: &'static str,
}

const V1_TABLES: ComponentTables = ComponentTables {
    trigger: "sys_hub_trigger_instance",
    logic: "sys_hub_flow_logic",
    action: "sys_hub_action_instance",
    subflow: "sys_hub_sub_flow_instance",
};

const V2_TABLES: ComponentTables = ComponentTables {
    trigger: "sys_hub_trigger_instance_v2",
    logic: "sys_hub_flow_logic_instance_v2",
    action: "sys_hub_action_instance_v2",
    subflow: "sys_hub_sub_flow_instance_v2",
};

#[derive(Debug, Serialize)]
struct FlowView {
    flow: FlowSummary,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    triggers: Vec<TriggerView>,
    structure: Structure,
    steps: Vec<Step>,
    action_types: Vec<String>,
    /// Where the steps were read: `flow`, `snapshot` (published snapshot
    /// rows), or `v2` (the `_v2` tables).
    source: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct FlowSummary {
    sys_id: String,
    name: String,
    internal_name: String,
    #[serde(rename = "type")]
    flow_type: String,
    status: String,
    active: bool,
    scope: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    description: String,
}

#[derive(Debug, Serialize)]
struct TriggerView {
    name: String,
    #[serde(rename = "type")]
    trigger_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    table: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    condition: String,
}

/// Rows of one table family for the flow's own id or its snapshot.
#[derive(Default)]
struct Rows {
    triggers: Vec<Record>,
    logic: Vec<Record>,
    actions: Vec<Record>,
    subflows: Vec<Record>,
    truncated: bool,
}

impl Rows {
    fn has_steps(&self) -> bool {
        !(self.logic.is_empty() && self.actions.is_empty() && self.subflows.is_empty())
    }

    /// Only rows belonging to `flow_id`.
    fn for_flow(&self, flow_id: &str) -> Self {
        let keep = |rows: &[Record]| {
            rows.iter()
                .filter(|row| text(row, "flow") == flow_id)
                .cloned()
                .collect::<Vec<_>>()
        };
        Self {
            triggers: keep(&self.triggers),
            logic: keep(&self.logic),
            actions: keep(&self.actions),
            subflows: keep(&self.subflows),
            truncated: self.truncated,
        }
    }
}

pub async fn handle(
    args: FlowArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    match args.command {
        FlowCommands::Show { flow } => {
            tracing::info!(flow = %flow, "Showing flow");
            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
            let view = show(&mut client, &flow).await?;
            output::print_output(&view, format)
        }
    }
}

async fn show(client: &mut SnowClient, key: &RecordKey) -> anyhow::Result<FlowView> {
    let record = find_flow(client, key).await?;
    let flow_id = record.sys_id().unwrap_or_default().to_string();
    let snapshot = text(&record, "latest_snapshot").to_string();
    let mut ids = vec![flow_id.clone()];
    if !snapshot.is_empty() && snapshot != flow_id {
        ids.push(snapshot.clone());
    }

    let all = read_rows(client, &V1_TABLES, &ids).await?;
    let own = all.for_flow(&flow_id);
    let (rows, source) = if own.has_steps() {
        (own, "flow")
    } else if all.has_steps() {
        (all.for_flow(&snapshot), "snapshot")
    } else {
        let v2 = read_rows(client, &V2_TABLES, &ids).await?;
        let own_v2 = v2.for_flow(&flow_id);
        if own_v2.has_steps() || !v2.has_steps() {
            (own_v2, "v2")
        } else {
            (v2.for_flow(&snapshot), "v2")
        }
    };

    let mut warnings = Vec::new();
    if rows.truncated {
        warnings.push(format!(
            "More than {MAX_COMPONENT_ROWS} rows in a component table; steps are incomplete"
        ));
    }
    let labels = if source == "v2" {
        Labels::default()
    } else {
        match read_labels(client, &rows).await {
            Ok(labels) => labels,
            Err(error) => {
                // Labels are enrichment: keep the structure when they fail.
                tracing::warn!(error = %error, "Could not read flow step labels");
                warnings.push(
                    "Trigger table/condition and If labels unavailable (sys_variable_value read failed)"
                        .to_string(),
                );
                Labels::default()
            }
        }
    };

    let components = components(&rows, &labels);
    let steps = flow::build_steps(&components);
    let mut description = text(&record, "description").to_string();
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        description = description.chars().take(MAX_DESCRIPTION_CHARS).collect();
        description.push('…');
    }
    Ok(FlowView {
        flow: FlowSummary {
            sys_id: flow_id,
            name: text(&record, "name").to_string(),
            internal_name: text(&record, "internal_name").to_string(),
            flow_type: text(&record, "type").to_string(),
            status: text(&record, "status").to_string(),
            active: text(&record, "active") == "true",
            scope: text(&record, "sys_scope.scope").to_string(),
            description,
        },
        triggers: rows
            .triggers
            .iter()
            .map(|row| {
                let sys_id = row.sys_id().unwrap_or_default();
                TriggerView {
                    name: text(row, "name").to_string(),
                    trigger_type: text(row, "trigger_type").to_string(),
                    table: labels.get(sys_id, "table"),
                    condition: labels.get(sys_id, "condition"),
                }
            })
            .collect(),
        structure: flow::structure(&steps),
        action_types: flow::action_types(&steps),
        steps,
        source,
        warnings,
    })
}

/// Look up one flow by sys_id, or by internal_name or name (exact).
async fn find_flow(client: &mut SnowClient, key: &RecordKey) -> anyhow::Result<Record> {
    let query = match key {
        RecordKey::SysId(sys_id) => format!("sys_id={sys_id}"),
        RecordKey::Name(name) => format!("internal_name={name}^ORname={name}"),
    };
    let pagination = PaginationConfig::default()
        .with_page_size(MAX_CANDIDATES + 1)
        .with_limit(Some(MAX_CANDIDATES + 1));
    let mut records = client
        .get_table_records(
            &TableName::from_static("sys_hub_flow"),
            Some(&query),
            Some(FLOW_FIELDS),
            &pagination,
            None,
        )
        .await?;
    // An internal_name match is unique per scope; prefer it over name matches.
    let wanted = key.to_string();
    if records.len() > 1 {
        let exact: Vec<usize> = records
            .iter()
            .enumerate()
            .filter(|(_, record)| text(record, "internal_name") == wanted)
            .map(|(index, _)| index)
            .collect();
        if let [index] = exact.as_slice() {
            return Ok(records.swap_remove(*index));
        }
    }
    match records.len() {
        0 => Err(CliError::Api {
            code: "RECORD_NOT_FOUND",
            message: format!(
                "No flow named '{key}'. Try `table list sys_hub_flow --query 'nameLIKE<text>' --fields name,internal_name,type,status`."
            ),
            status: 404,
            detail: None,
            instance: client.base_url().to_string(),
        }
        .into()),
        1 => Ok(records.remove(0)),
        count => {
            let issues = records
                .iter()
                .take(MAX_CANDIDATES)
                .map(|record| ValidationIssue {
                    kind: "ambiguous_record",
                    field: record.sys_id().map(str::to_string),
                    record_index: None,
                    message: format!(
                        "candidate {} ({}, scope {}, sys_id {})",
                        text(record, "name"),
                        text(record, "internal_name"),
                        text(record, "sys_scope.scope"),
                        record.sys_id().unwrap_or("?")
                    ),
                })
                .collect();
            Err(ValidationError::new(format!(
                "{} flows match '{key}'; pass the sys_id",
                count.min(MAX_CANDIDATES)
            ))
            .with_issues(issues)
            .into())
        }
    }
}

/// Read the trigger and component tables of one family for every id in
/// `ids` (the flow and its snapshot) with one `flowIN` request per table.
async fn read_rows(
    client: &mut SnowClient,
    tables: &ComponentTables,
    ids: &[String],
) -> anyhow::Result<Rows> {
    let query = format!("flowIN{}", ids.join(","));
    let mut rows = Rows::default();
    let (triggers, truncated) = read_table(client, tables.trigger, &query, TRIGGER_FIELDS).await?;
    rows.triggers = triggers;
    rows.truncated |= truncated;
    let fields = format!("{COMPONENT_FIELDS},logic_definition.name");
    let (logic, truncated) = read_table(client, tables.logic, &query, &fields).await?;
    rows.logic = logic;
    rows.truncated |= truncated;
    let fields = format!("{COMPONENT_FIELDS},action_type.name");
    let (actions, truncated) = read_table(client, tables.action, &query, &fields).await?;
    rows.actions = actions;
    rows.truncated |= truncated;
    let fields = format!("{COMPONENT_FIELDS},subflow.name");
    let (subflows, truncated) = read_table(client, tables.subflow, &query, &fields).await?;
    rows.subflows = subflows;
    rows.truncated |= truncated;
    Ok(rows)
}

async fn read_table(
    client: &mut SnowClient,
    table: &'static str,
    query: &str,
    fields: &str,
) -> anyhow::Result<(Vec<Record>, bool)> {
    let pagination = PaginationConfig::default()
        .with_page_size(MAX_COMPONENT_ROWS)
        .with_limit(Some(MAX_COMPONENT_ROWS));
    let result = client
        .get_table_records_with_meta(
            &TableName::from_static(table),
            Some(query),
            Some(fields),
            &pagination,
            None,
        )
        .await?;
    Ok((result.records, result.truncated))
}

/// `sys_variable_value` labels keyed by (component sys_id, element).
#[derive(Default)]
struct Labels(Vec<(String, String, String)>);

impl Labels {
    fn get(&self, sys_id: &str, element: &str) -> String {
        self.0
            .iter()
            .find(|(key, name, _)| key == sys_id && name == element)
            .map(|(_, _, value)| value.clone())
            .unwrap_or_default()
    }
}

/// Condition labels of logic steps and the trigger's table and condition.
async fn read_labels(client: &mut SnowClient, rows: &Rows) -> anyhow::Result<Labels> {
    let keys: Vec<&str> = rows
        .triggers
        .iter()
        .chain(rows.logic.iter())
        .filter_map(Record::sys_id)
        .collect();
    if keys.is_empty() {
        return Ok(Labels::default());
    }
    let query = format!(
        "document_keyIN{}^variable.elementIN{}",
        keys.join(","),
        LABEL_ELEMENTS.join(",")
    );
    let (records, _) = read_table(
        client,
        "sys_variable_value",
        &query,
        "document_key,variable.element,value",
    )
    .await?;
    Ok(Labels(
        records
            .iter()
            .filter(|record| LABEL_ELEMENTS.contains(&text(record, "variable.element")))
            .map(|record| {
                (
                    text(record, "document_key").to_string(),
                    text(record, "variable.element").to_string(),
                    text(record, "value").to_string(),
                )
            })
            .collect(),
    ))
}

fn components(rows: &Rows, labels: &Labels) -> Vec<Component> {
    let families: [(&[Record], StepKind, &str); 3] = [
        (&rows.logic, StepKind::Logic, "logic_definition.name"),
        (&rows.actions, StepKind::Action, "action_type.name"),
        (&rows.subflows, StepKind::Subflow, "subflow.name"),
    ];
    families
        .into_iter()
        .flat_map(|(records, kind, type_field)| {
            records.iter().map(move |record| {
                let label = labels.get(record.sys_id().unwrap_or_default(), "condition_name");
                let comment = text(record, "comment");
                let note = match (label.is_empty(), comment.is_empty()) {
                    (false, false) => format!("{label}; {comment}"),
                    (false, true) => label,
                    _ => comment.to_string(),
                };
                Component {
                    kind,
                    ui_id: text(record, "ui_id").to_string(),
                    parent_ui_id: text(record, "parent_ui_id").to_string(),
                    order: text(record, "order").to_string(),
                    type_name: text(record, type_field).to_string(),
                    note,
                }
            })
        })
        .collect()
}

/// A string field of `record`, or "" (reference objects yield their value).
fn text<'a>(record: &'a Record, field: &str) -> &'a str {
    record
        .get_str(field)
        .or_else(|| record.reference_value(field))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(value: serde_json::Value) -> Record {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn components_take_labels_and_comments_as_notes() {
        let rows = Rows {
            logic: vec![record(serde_json::json!({
                "sys_id": "l1", "flow": "f", "ui_id": "u1", "parent_ui_id": "",
                "order": "1", "comment": "", "logic_definition.name": "If"
            }))],
            actions: vec![record(serde_json::json!({
                "sys_id": "a1", "flow": "f", "ui_id": "u2", "parent_ui_id": "u1",
                "order": "2", "comment": "Mark approved", "action_type.name": "Update Record"
            }))],
            ..Rows::default()
        };
        let labels = Labels(vec![(
            "l1".to_string(),
            "condition_name".to_string(),
            "Skipped".to_string(),
        )]);
        let components = components(&rows, &labels);
        assert_eq!(components[0].note, "Skipped");
        assert_eq!(components[0].type_name, "If");
        assert_eq!(components[1].note, "Mark approved");
        assert_eq!(components[1].kind, StepKind::Action);
    }

    #[test]
    fn rows_for_flow_keep_only_that_flow() {
        let rows = Rows {
            actions: vec![
                record(serde_json::json!({"sys_id": "a", "flow": "f"})),
                record(serde_json::json!({"sys_id": "b", "flow": "snap"})),
            ],
            ..Rows::default()
        };
        assert_eq!(rows.for_flow("f").actions.len(), 1);
        assert!(rows.for_flow("f").has_steps());
        assert!(!rows.for_flow("other").has_steps());
    }
}
