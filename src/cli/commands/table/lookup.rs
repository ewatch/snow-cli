//! `table get <table> <name>`: resolve a record by a human key in one read.

use crate::client::pagination::PaginationConfig;
use crate::client::{SnowClient, TableReadOptions};
use crate::error::CliError;
use crate::models::identifiers::{EncodedQueryValue, TableName};
use crate::models::record::Record;
use crate::models::validation::{ValidationError, ValidationIssue};

/// Candidates listed when a name matches several records.
const MAX_CANDIDATES: usize = 5;

/// Fields every lookup reads so an ambiguous match can name its candidates.
const CANDIDATE_FIELDS: [&str; 2] = ["sys_id", "sys_scope"];

/// The field a non-sys_id key is matched against when `--key-field` is
/// omitted: `number` for task-style numbers (`INC0010001`, `CHG0030001`),
/// otherwise `name` (script includes, tables, groups, ...).
pub(super) fn default_key_field(key: &str) -> &'static str {
    let prefix_len = key
        .find(|ch: char| !ch.is_ascii_uppercase())
        .unwrap_or(key.len());
    let digits = &key[prefix_len..];
    if prefix_len >= 2 && digits.len() >= 4 && digits.bytes().all(|b| b.is_ascii_digit()) {
        "number"
    } else {
        "name"
    }
}

/// Read the single record of `table` whose `key_field` equals `key`.
///
/// One Table API read (limit `MAX_CANDIDATES + 1`) with the caller's
/// projection plus `sys_id`/`sys_scope`; fields added only for disambiguation
/// are removed from the returned record. No match is a `RECORD_NOT_FOUND`
/// API error (exit 4, like a missing sys_id); several matches are a
/// validation error listing up to five candidates.
pub(super) async fn get_record_by_key(
    client: &mut SnowClient,
    table: &TableName,
    key: &EncodedQueryValue,
    key_field: Option<&EncodedQueryValue>,
    projection: Option<&str>,
    options: &TableReadOptions,
) -> anyhow::Result<Record> {
    let field = key_field.map_or_else(|| default_key_field(key.as_str()), |f| f.as_str());
    let query = format!("{field}={key}");

    let mut added = Vec::new();
    let projection = projection.map(|projection| {
        let mut projection = projection.to_string();
        for extra in CANDIDATE_FIELDS {
            if !projection
                .split(',')
                .any(|existing| existing.trim() == extra)
            {
                projection.push(',');
                projection.push_str(extra);
                added.push(extra);
            }
        }
        projection
    });

    let pagination = PaginationConfig::default()
        .with_page_size(MAX_CANDIDATES + 1)
        .with_limit(Some(MAX_CANDIDATES + 1));
    let mut result = client
        .get_table_records_with_options(
            table,
            Some(&query),
            projection.as_deref(),
            &pagination,
            None,
            options,
        )
        .await?;

    match result.records.len() {
        0 => Err(CliError::Api {
            code: "RECORD_NOT_FOUND",
            message: format!(
                "No {table} record has {field} '{key}'. Pass a sys_id, or --key-field to match another field."
            ),
            status: 404,
            detail: None,
            instance: client.base_url().to_string(),
        }
        .into()),
        1 => {
            let mut record = result.records.remove(0);
            for extra in added {
                record.fields.remove(extra);
            }
            Ok(record)
        }
        count => {
            let more = if count > MAX_CANDIDATES { "more than " } else { "" };
            let issues = result
                .records
                .iter()
                .take(MAX_CANDIDATES)
                .map(|record| ValidationIssue {
                    kind: "ambiguous_record",
                    field: record.sys_id().map(str::to_string),
                    record_index: None,
                    message: format!(
                        "candidate sys_id {} in scope {}",
                        record.sys_id().unwrap_or("?"),
                        record.reference_value("sys_scope").unwrap_or("?")
                    ),
                })
                .collect();
            Err(ValidationError::new(format!(
                "{more}{} {table} records have {field} '{key}'; pass one candidate's sys_id instead",
                count.min(MAX_CANDIDATES)
            ))
            .with_issues(issues)
            .into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_numbers_use_number_and_everything_else_name() {
        assert_eq!(default_key_field("INC0010001"), "number");
        assert_eq!(default_key_field("CHG0030001"), "number");
        assert_eq!(default_key_field("ChangeRequestStateHandler"), "name");
        assert_eq!(default_key_field("AbstractAjaxProcessor"), "name");
        assert_eq!(default_key_field("INC12"), "name");
        assert_eq!(default_key_field("x_acme_util"), "name");
    }
}
