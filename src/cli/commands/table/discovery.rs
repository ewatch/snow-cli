//! Table discovery: `table find` (names from `sys_db_object`) and
//! `table choices` (choice labels from `sys_choice`).

use crate::cli::args::OutputFormat;
use crate::cli::commands::dictionary::fetch_table_hierarchy;
use crate::cli::output;
use crate::client::SnowClient;
use crate::client::pagination::PaginationConfig;
use crate::models::identifiers::{EncodedQueryValue, TableName};
use crate::models::record::Record;

/// Bounded default row count for `table find`, matching `table list`.
const DEFAULT_FIND_LIMIT: usize = 20;

/// `sys_db_object` columns `table find` reads, with dot-walks for the parent
/// table and scope so agents see the hierarchy without a second lookup.
const FIND_FIELDS: &str = "name,label,super_class.name,sys_scope.scope";

/// `sys_choice` columns `table choices` reads.
const CHOICE_FIELDS: &str = "name,element,value,label,sequence,dependent_value";

/// Page window for `table find`.
#[derive(Debug, Clone, Copy)]
pub(super) struct FindWindow {
    pub(super) limit: Option<usize>,
    pub(super) offset: usize,
    pub(super) all: bool,
}

/// `table find <text>`: tables whose name or label contains `text`, sorted by
/// name.
pub(super) async fn handle_find(
    client: &mut SnowClient,
    text: &EncodedQueryValue,
    window: FindWindow,
    format: &OutputFormat,
) -> anyhow::Result<()> {
    let table: TableName = "sys_db_object".parse()?;
    let query = format!("nameLIKE{text}^ORlabelLIKE{text}^ORDERBYname");
    let pagination = PaginationConfig::default()
        .with_limit((!window.all).then(|| window.limit.unwrap_or(DEFAULT_FIND_LIMIT)))
        .with_offset(window.offset);
    let mut result = client
        .get_table_records_with_meta(&table, Some(&query), Some(FIND_FIELDS), &pagination, None)
        .await?;
    for record in &mut result.records {
        rename_find_fields(record);
    }
    output::print_table_list(&result, format)
}

/// Replace dot-walked column names with stable output names: `extends` (the
/// parent table, `null` for a base table) and `scope`.
fn rename_find_fields(record: &mut Record) {
    for (from, to) in [
        ("super_class.name", "extends"),
        ("sys_scope.scope", "scope"),
    ] {
        let value = match record.fields.remove(from) {
            Some(serde_json::Value::String(text)) if text.is_empty() => serde_json::Value::Null,
            Some(value) => value,
            None => serde_json::Value::Null,
        };
        record.fields.insert(to.to_owned(), value);
    }
}

/// `table choices <table> [field]`: active choice values and labels, taken
/// from the most specific table in the hierarchy that defines choices for
/// each field (`incident.priority` choices live on `task`).
pub(super) async fn handle_choices(
    client: &mut SnowClient,
    table: &TableName,
    field: Option<&EncodedQueryValue>,
    language: &EncodedQueryValue,
    format: &OutputFormat,
) -> anyhow::Result<()> {
    let chain = fetch_table_hierarchy(client, table).await?;
    let names = chain
        .iter()
        .map(TableName::as_str)
        .collect::<Vec<_>>()
        .join(",");
    let mut query = format!("nameIN{names}^inactive=false^language={language}");
    if let Some(field) = field {
        query.push_str(&format!("^element={field}"));
    }
    query.push_str("^ORDERBYelement^ORDERBYsequence");

    let choice_table: TableName = "sys_choice".parse()?;
    let records = client
        .get_table_records(
            &choice_table,
            Some(&query),
            Some(CHOICE_FIELDS),
            &PaginationConfig::default(),
            None,
        )
        .await?;
    let records = keep_most_specific_choices(records, &chain);
    let records: Vec<Record> = records.into_iter().map(into_choice_row).collect();
    output::print_records(&records, format)
}

/// Keep, per element, only the rows from the most derived table that has any
/// choices for it, preserving the server's element/sequence order.
fn keep_most_specific_choices(records: Vec<Record>, chain: &[TableName]) -> Vec<Record> {
    let depth = |record: &Record| {
        record
            .get_str("name")
            .and_then(|name| chain.iter().position(|table| table.as_str() == name))
            .unwrap_or(usize::MAX)
    };
    let mut best: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for record in &records {
        let element = record.get_str("element").unwrap_or_default().to_owned();
        let level = depth(record);
        best.entry(element)
            .and_modify(|current| *current = (*current).min(level))
            .or_insert(level);
    }
    records
        .into_iter()
        .filter(|record| {
            best.get(record.get_str("element").unwrap_or_default()) == Some(&depth(record))
        })
        .collect()
}

/// Output row: `element`, `value`, `label`, `sequence`, the defining `table`,
/// and `dependent_value` only when set.
fn into_choice_row(mut record: Record) -> Record {
    if let Some(table) = record.fields.remove("name") {
        record.fields.insert("table".to_owned(), table);
    }
    if record.get_str("dependent_value").is_some_and(str::is_empty) {
        record.fields.remove("dependent_value");
    }
    record
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(fields: serde_json::Value) -> Record {
        serde_json::from_value(fields).unwrap()
    }

    #[test]
    fn find_renames_dot_walked_fields() {
        let mut row = record(serde_json::json!({
            "name": "sys_hub_flow", "label": "Flow",
            "super_class.name": "sys_hub_flow_base", "sys_scope.scope": "global"
        }));
        rename_find_fields(&mut row);
        assert_eq!(row.get_str("extends"), Some("sys_hub_flow_base"));
        assert_eq!(row.get_str("scope"), Some("global"));
        assert!(!row.fields.contains_key("super_class.name"));

        let mut base = record(serde_json::json!({"name": "task", "super_class.name": ""}));
        rename_find_fields(&mut base);
        assert_eq!(base.fields["extends"], serde_json::Value::Null);
    }

    #[test]
    fn choices_prefer_the_most_specific_table_per_element() {
        let chain: Vec<TableName> = vec!["incident".parse().unwrap(), "task".parse().unwrap()];
        let rows = vec![
            record(serde_json::json!({"name": "task", "element": "priority", "value": "1"})),
            record(serde_json::json!({"name": "incident", "element": "state", "value": "1"})),
            record(serde_json::json!({"name": "task", "element": "state", "value": "-5"})),
            record(serde_json::json!({"name": "task", "element": "priority", "value": "2"})),
        ];
        let kept: Vec<(String, String)> = keep_most_specific_choices(rows, &chain)
            .iter()
            .map(|r| {
                (
                    r.get_str("name").unwrap().to_owned(),
                    r.get_str("value").unwrap().to_owned(),
                )
            })
            .collect();
        assert_eq!(
            kept,
            [
                ("task".to_owned(), "1".to_owned()),
                ("incident".to_owned(), "1".to_owned()),
                ("task".to_owned(), "2".to_owned()),
            ]
        );
    }

    #[test]
    fn choice_rows_name_the_defining_table_and_drop_empty_dependents() {
        let row = into_choice_row(record(serde_json::json!({
            "name": "task", "element": "priority", "value": "1", "dependent_value": ""
        })));
        assert_eq!(row.get_str("table"), Some("task"));
        assert!(!row.fields.contains_key("name"));
        assert!(!row.fields.contains_key("dependent_value"));
    }
}
