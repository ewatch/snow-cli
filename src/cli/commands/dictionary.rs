//! Shared `sys_db_object` / `sys_dictionary` lookups for schema-aware commands.
//!
//! `sys_dictionary.name` is a plain table-name field, so `nameINSTANCEOF<table>`
//! silently degrades to an exact match and never reaches parent tables. The
//! effective schema of a table therefore has to be built by walking the
//! `sys_db_object.super_class` chain and querying `nameIN<chain>` explicitly.

use std::collections::HashMap;

use crate::client::SnowClient;
use crate::client::pagination::PaginationConfig;
use crate::models::identifiers::TableName;
use crate::models::order_by::OrderBy;
use crate::models::record::Record;

/// Page size for `sys_dictionary` reads; wide tables such as `cmdb_ci_server`
/// have several hundred effective columns.
const DICTIONARY_PAGE_SIZE: usize = 500;

/// Upper bound on `super_class` hops, guarding against a corrupted cyclic
/// hierarchy. Real ServiceNow hierarchies are well under ten levels deep.
const MAX_HIERARCHY_DEPTH: usize = 32;

/// Return `table` followed by its ancestors, most-derived first
/// (e.g. `[incident, task]`).
///
/// When `table` is not found in `sys_db_object`, returns just `[table]` so
/// the caller's dictionary lookup decides how to report the missing table.
pub(crate) async fn fetch_table_hierarchy(
    client: &mut SnowClient,
    table: &TableName,
) -> anyhow::Result<Vec<TableName>> {
    let mut chain = Vec::new();
    let mut next = match fetch_table_definition(client, &format!("name={table}")).await {
        Ok(definition) => definition,
        Err(error) if is_not_found(&error) => None,
        Err(error) => return Err(error),
    };

    while let Some((name, super_class)) = next {
        if chain.contains(&name) || chain.len() >= MAX_HIERARCHY_DEPTH {
            tracing::warn!(table = %table, at = %name, "Stopping table hierarchy walk at a cycle or depth limit");
            break;
        }
        chain.push(name);
        next = match super_class {
            Some(sys_id) => fetch_table_definition(client, &format!("sys_id={sys_id}")).await?,
            None => None,
        };
    }

    if chain.is_empty() {
        chain.push(table.clone());
    }
    Ok(chain)
}

/// Look up one `sys_db_object` row, returning its name and `super_class` sys_id.
async fn fetch_table_definition(
    client: &mut SnowClient,
    query: &str,
) -> anyhow::Result<Option<(TableName, Option<String>)>> {
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

    let Some(record) = records.into_iter().next() else {
        return Ok(None);
    };
    let name: TableName = record.get_str("name").unwrap_or_default().parse()?;
    let super_class = reference_value(&record, "super_class").filter(|id| !id.is_empty());
    Ok(Some((name, super_class)))
}

/// Fetch the effective `sys_dictionary` columns of `tables`, a hierarchy as
/// returned by [`fetch_table_hierarchy`].
///
/// When a column is defined on several tables in the chain (a child override
/// of a parent column), only the most-derived row is kept. Rows are sorted by
/// column name. `fields` must include `name` and `element`.
pub(crate) async fn fetch_dictionary_columns(
    client: &mut SnowClient,
    tables: &[TableName],
    fields: &str,
) -> anyhow::Result<Vec<Record>> {
    let names = tables
        .iter()
        .map(TableName::as_str)
        .collect::<Vec<_>>()
        .join(",");
    let query = match tables {
        [single] => format!("name={single}^elementISNOTEMPTY^element!=sys_tags"),
        _ => format!("nameIN{names}^elementISNOTEMPTY^element!=sys_tags"),
    };
    let pagination = PaginationConfig::default()
        .with_page_size(DICTIONARY_PAGE_SIZE)
        .with_limit(None);
    let order_by: OrderBy = "element".parse()?;
    let sys_dictionary = TableName::from_static("sys_dictionary");
    let records = client
        .get_table_records(
            &sys_dictionary,
            Some(&query),
            Some(fields),
            &pagination,
            Some(&order_by),
        )
        .await?;

    Ok(keep_most_derived_columns(records, tables))
}

/// Deduplicate columns by `element`, keeping the row from the table closest to
/// the start of `tables`, and sort the result by column name.
fn keep_most_derived_columns(records: Vec<Record>, tables: &[TableName]) -> Vec<Record> {
    let depth = |record: &Record| {
        let name = record.get_str("name").unwrap_or_default();
        tables
            .iter()
            .position(|table| table.as_str() == name)
            .unwrap_or(usize::MAX)
    };

    let mut by_element: HashMap<String, Record> = HashMap::new();
    for record in records {
        let element = record.get_str("element").unwrap_or_default().to_string();
        match by_element.get(&element) {
            Some(existing) if depth(existing) <= depth(&record) => {}
            _ => {
                by_element.insert(element, record);
            }
        }
    }

    let mut columns: Vec<(String, Record)> = by_element.into_iter().collect();
    columns.sort_by(|(a, _), (b, _)| a.cmp(b));
    columns.into_iter().map(|(_, record)| record).collect()
}

fn is_not_found(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<crate::client::error::ApiError>()
        .is_some_and(|api_error| api_error.status == 404)
}

/// Read a reference field that may be a plain sys_id or a `{"value": ...}` link object.
fn reference_value(record: &Record, field: &str) -> Option<String> {
    match record.fields.get(field) {
        Some(serde_json::Value::String(text)) => Some(text.clone()),
        Some(serde_json::Value::Object(map)) => map
            .get("value")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(table: &str, element: &str, label: &str) -> Record {
        serde_json::from_value(serde_json::json!({
            "name": table,
            "element": element,
            "column_label": label,
        }))
        .unwrap()
    }

    fn chain() -> Vec<TableName> {
        vec!["incident".parse().unwrap(), "task".parse().unwrap()]
    }

    #[test]
    fn child_override_wins_over_parent_column() {
        let records = vec![
            column("task", "state", "Task state"),
            column("incident", "state", "Incident state"),
            column("task", "short_description", "Short description"),
            column("incident", "category", "Category"),
        ];

        let columns = keep_most_derived_columns(records, &chain());

        let summary: Vec<(&str, &str)> = columns
            .iter()
            .map(|r| (r.get_str("element").unwrap(), r.get_str("name").unwrap()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("category", "incident"),
                ("short_description", "task"),
                ("state", "incident"),
            ]
        );
        assert_eq!(columns[2].get_str("column_label"), Some("Incident state"));
    }

    #[test]
    fn rows_from_unknown_tables_lose_to_chain_members() {
        let records = vec![
            column("incident", "number", "Number"),
            column("other", "number", "Other number"),
        ];

        let columns = keep_most_derived_columns(records, &chain());

        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].get_str("name"), Some("incident"));
    }

    #[test]
    fn reference_value_reads_plain_and_link_objects() {
        let plain: Record =
            serde_json::from_value(serde_json::json!({"super_class": "abc"})).unwrap();
        let link: Record = serde_json::from_value(
            serde_json::json!({"super_class": {"value": "def", "link": "https://x"}}),
        )
        .unwrap();
        assert_eq!(
            reference_value(&plain, "super_class").as_deref(),
            Some("abc")
        );
        assert_eq!(
            reference_value(&link, "super_class").as_deref(),
            Some("def")
        );
    }
}
