//! Unknown-field detection for Table API reads (`--check-fields`).
//!
//! ServiceNow silently drops unknown or unreadable names from
//! `sysparm_fields` and silently ignores encoded-query terms on them, so a
//! typo such as `assigned_two.name=...` returns the unfiltered result. The
//! check is cheap and never walks the dictionary:
//!
//! - In `warn` mode the checked paths are added to the main request's
//!   projection and looked up in the returned keys (then removed again), so a
//!   non-empty result costs no extra request. Only an empty result needs one
//!   `sysparm_limit=1` probe without the query.
//! - In `error` mode the probe runs first, so the main query is not sent when
//!   a field is unknown.
//!
//! A failed probe or a table without readable rows leaves the fields
//! unverified: the check never blocks a read on its own failure.

use crate::cli::args::CheckFields;
use crate::client::SnowClient;
use crate::client::pagination::PaginationConfig;
use crate::models::encoded_query;
use crate::models::field_warning::{FieldSource, FieldWarning};
use crate::models::identifiers::TableName;
use crate::models::order_by::OrderBy;
use crate::models::record::Record;
use crate::models::validation::{ValidationError, ValidationIssue};

/// Field paths to verify, each with the flag it came from, deduplicated by
/// path (first source wins).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct FieldChecks(Vec<(String, FieldSource)>);

/// User-supplied names to check. `fields` is the caller's `--fields` (the
/// compact default projection is never checked; `*` checks nothing).
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct CheckInputs<'a> {
    pub fields: Option<&'a str>,
    pub query: Option<&'a str>,
    pub order_by: Option<&'a OrderBy>,
    pub group_by: Option<&'a str>,
    pub avg: Option<&'a str>,
    pub min: Option<&'a str>,
    pub max: Option<&'a str>,
    pub sum: Option<&'a str>,
    pub having: &'a [String],
}

impl FieldChecks {
    pub(super) fn collect(mode: CheckFields, inputs: CheckInputs<'_>) -> Self {
        let mut checks = Self::default();
        if mode == CheckFields::Off {
            return checks;
        }
        let list = |value: Option<&str>| -> Vec<String> {
            value
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|field| !field.is_empty() && *field != "*")
                .map(str::to_string)
                .collect()
        };
        for field in list(inputs.fields) {
            checks.push(field, FieldSource::Fields);
        }
        for field in encoded_query::field_paths(inputs.query.unwrap_or_default()) {
            checks.push(field, FieldSource::Query);
        }
        for field in inputs.order_by.into_iter().flat_map(OrderBy::fields) {
            checks.push(field.to_string(), FieldSource::OrderBy);
        }
        for field in list(inputs.group_by) {
            checks.push(field, FieldSource::GroupBy);
        }
        for field in list(inputs.avg) {
            checks.push(field, FieldSource::Avg);
        }
        for field in list(inputs.min) {
            checks.push(field, FieldSource::Min);
        }
        for field in list(inputs.max) {
            checks.push(field, FieldSource::Max);
        }
        for field in list(inputs.sum) {
            checks.push(field, FieldSource::Sum);
        }
        for field in inputs.having {
            checks.push(field.clone(), FieldSource::Having);
        }
        checks
    }

    fn push(&mut self, field: String, source: FieldSource) {
        if !self.0.iter().any(|(existing, _)| *existing == field) {
            self.0.push((field, source));
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Subset of checked paths that dot-walk across a reference (`a.b`).
    pub(super) fn dot_walked(&self) -> Self {
        Self(
            self.0
                .iter()
                .filter(|(field, _)| field.contains('.'))
                .cloned()
                .collect(),
        )
    }

    /// Whether an empty result warrants a probe: only names that shape the
    /// result (query, sort, grouping, aggregates) matter then; a
    /// projection-only check on an empty page has nothing to report.
    pub(super) fn needs_probe_on_empty(&self) -> bool {
        self.0
            .iter()
            .any(|(_, source)| *source != FieldSource::Fields)
    }

    fn paths(&self) -> String {
        self.0
            .iter()
            .map(|(field, _)| field.as_str())
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Add checked paths missing from `projection` (`None` = every field,
    /// which is left alone). Returns the projection to send and the paths
    /// that were added and must be removed from the output again.
    pub(super) fn augment_projection(
        &self,
        projection: Option<String>,
    ) -> (Option<String>, Vec<String>) {
        let Some(mut projection) = projection else {
            return (None, Vec::new());
        };
        let mut injected = Vec::new();
        for (field, _) in &self.0 {
            if !projection
                .split(',')
                .any(|existing| existing.trim() == field)
            {
                if !projection.is_empty() {
                    projection.push(',');
                }
                projection.push_str(field);
                injected.push(field.to_string());
            }
        }
        (Some(projection), injected)
    }

    /// Verify checked paths against fetched records in `warn` mode.
    ///
    /// - When `records` is empty and any non-projection field was checked,
    ///   probes one unfiltered row.
    /// - When `all_fields` is true (`--fields '*'`), top-level columns are
    ///   checked directly against `records`, while dot-walked paths (which
    ///   ServiceNow omits when `sysparm_fields` is not sent) are verified via
    ///   a targeted one-row probe so `--fields '*'` matches `--check-fields
    ///   error` behavior. If that probe fails, falls back to root-segment
    ///   verification against `records`.
    pub(super) async fn check_records(
        &self,
        client: &mut SnowClient,
        table: &TableName,
        records: &[Record],
        all_fields: bool,
        query_no_domain: bool,
    ) -> Vec<FieldWarning> {
        if self.is_empty() {
            return Vec::new();
        }
        if records.is_empty() {
            if self.needs_probe_on_empty() {
                return self
                    .probe(client, table, query_no_domain)
                    .await
                    .unwrap_or_default();
            }
            return Vec::new();
        }
        if !all_fields {
            return self.missing(table, records, false);
        }
        let dot_checks = self.dot_walked();
        if dot_checks.is_empty() {
            return self.missing(table, records, false);
        }
        let Some(probed_missing) = dot_checks.probe(client, table, query_no_domain).await else {
            return self.missing(table, records, true);
        };
        self.merge_all_fields_warnings(table, records, &probed_missing)
    }

    /// Combine top-level record checks with probed dot-walk warnings in
    /// original first-seen order.
    fn merge_all_fields_warnings(
        &self,
        table: &TableName,
        records: &[Record],
        probed_dot_missing: &[FieldWarning],
    ) -> Vec<FieldWarning> {
        self.0
            .iter()
            .filter_map(|(field, source)| {
                if field.contains('.') {
                    probed_dot_missing
                        .iter()
                        .find(|warning| warning.field == *field)
                        .cloned()
                } else if !records
                    .iter()
                    .any(|record| record.fields.contains_key(field))
                {
                    Some(FieldWarning::unknown_field(table.as_str(), field, *source))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Warnings for checked paths absent from every record. `all_fields`
    /// means the records were read without a projection, where dot-walked
    /// paths are not returned, so only their first segment can be verified.
    /// No records means nothing can be verified.
    pub(super) fn missing(
        &self,
        table: &TableName,
        records: &[Record],
        all_fields: bool,
    ) -> Vec<FieldWarning> {
        if records.is_empty() {
            return Vec::new();
        }
        self.0
            .iter()
            .filter(|(field, _)| {
                let key = if all_fields {
                    field.split('.').next().unwrap_or(field)
                } else {
                    field.as_str()
                };
                !records.iter().any(|record| record.fields.contains_key(key))
            })
            .map(|(field, source)| FieldWarning::unknown_field(table.as_str(), field, *source))
            .collect()
    }

    /// Read one row (no query) projected to the checked paths and report the
    /// missing ones. `None` when the probe fails or the table has no readable
    /// rows: the fields stay unverified rather than blocking the command.
    pub(super) async fn probe(
        &self,
        client: &mut SnowClient,
        table: &TableName,
        query_no_domain: bool,
    ) -> Option<Vec<FieldWarning>> {
        if self.is_empty() {
            return Some(Vec::new());
        }
        let pagination = PaginationConfig::default()
            .with_page_size(1)
            .with_limit(Some(1));
        let paths = self.paths();
        match client
            .get_table_records_with_domain(
                table,
                None,
                Some(&paths),
                &pagination,
                None,
                query_no_domain,
            )
            .await
        {
            Ok(result) if !result.records.is_empty() => {
                Some(self.missing(table, &result.records, false))
            }
            Ok(_) => {
                tracing::debug!(table = %table, "field check skipped: no readable row to inspect");
                None
            }
            Err(error) => {
                // Only stable codes are logged, never transport or response text.
                let code = match error.downcast_ref::<crate::client::error::ApiError>() {
                    Some(api) => api.code.clone(),
                    None => crate::client::error::classify_transport_error(&error)
                        .map_or("UNKNOWN_ERROR", |kind| kind.code())
                        .to_string(),
                };
                tracing::warn!(
                    table = %table,
                    code = %code,
                    "field check skipped: probe read failed; field names are unverified"
                );
                None
            }
        }
    }
}

/// Remove projection paths added only for checking.
pub(super) fn strip_injected(records: &mut [Record], injected: &[String]) {
    if injected.is_empty() {
        return;
    }
    for record in records {
        for field in injected {
            record.fields.remove(field);
        }
    }
}

/// Emit each warning as one JSON line on stderr (`{"warning":{...}}`), in
/// addition to any structured envelope, so CSV/plain consumers see it too.
pub(super) fn emit_stderr(warnings: &[FieldWarning]) {
    for warning in warnings {
        if let Ok(json) = serde_json::to_string(&serde_json::json!({ "warning": warning })) {
            eprintln!("{json}");
        }
    }
}

/// `--check-fields error`: a validation failure (exit 2) listing every field.
pub(super) fn into_error(table: &TableName, warnings: &[FieldWarning]) -> anyhow::Error {
    let names = warnings
        .iter()
        .map(|warning| format!("'{}' ({})", warning.field, warning.source.flag()))
        .collect::<Vec<_>>()
        .join(", ");
    let issues = warnings
        .iter()
        .map(|warning| ValidationIssue {
            kind: "unknown_field",
            field: Some(warning.field.clone()),
            record_index: None,
            message: warning.message.clone(),
        })
        .collect();
    ValidationError::new(format!(
        "{table} returned no field {names} (unknown or not readable); fix the field names or pass --check-fields warn|off"
    ))
    .with_issues(issues)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> TableName {
        "incident".parse().unwrap()
    }

    fn record(value: serde_json::Value) -> Record {
        serde_json::from_value(value).unwrap()
    }

    fn checks() -> FieldChecks {
        let order: OrderBy = "-sys_created_on".parse().unwrap();
        FieldChecks::collect(
            CheckFields::Warn,
            CheckInputs {
                fields: Some("number,extends"),
                query: Some("active=true^assigned_two.name=Abel Tuter^number=1"),
                order_by: Some(&order),
                ..CheckInputs::default()
            },
        )
    }

    #[test]
    fn collects_aggregate_and_having_sources() {
        let having = vec!["reassignment_count".to_string(), "priority".to_string()];
        let collected = FieldChecks::collect(
            CheckFields::Warn,
            CheckInputs {
                group_by: Some("state"),
                avg: Some("priority"),
                min: Some("impact"),
                max: Some("urgency"),
                sum: Some("calendar_duration"),
                having: &having,
                ..CheckInputs::default()
            },
        );
        assert_eq!(
            collected.0,
            vec![
                ("state".to_string(), FieldSource::GroupBy),
                ("priority".to_string(), FieldSource::Avg),
                ("impact".to_string(), FieldSource::Min),
                ("urgency".to_string(), FieldSource::Max),
                ("calendar_duration".to_string(), FieldSource::Sum),
                ("reassignment_count".to_string(), FieldSource::Having),
            ]
        );
    }

    #[test]
    fn augment_projection_avoids_leading_comma_on_empty_projection() {
        let (projection, injected) = checks().augment_projection(Some(String::new()));
        assert_eq!(
            projection.as_deref(),
            Some("number,extends,active,assigned_two.name,sys_created_on")
        );
        assert_eq!(injected.len(), 5);
    }

    #[test]
    fn dot_walked_and_merge_preserve_order() {
        let dot = checks().dot_walked();
        assert_eq!(
            dot.0,
            vec![("assigned_two.name".to_string(), FieldSource::Query)]
        );
        let records = [record(serde_json::json!({
            "number": "INC1", "active": "true", "assigned_two": "x", "sys_created_on": "t"
        }))];
        let probed = vec![FieldWarning::unknown_field(
            "incident",
            "assigned_two.name",
            FieldSource::Query,
        )];
        let merged = checks().merge_all_fields_warnings(&table(), &records, &probed);
        let fields: Vec<_> = merged.iter().map(|w| w.field.as_str()).collect();
        assert_eq!(fields, ["extends", "assigned_two.name"]);
    }

    #[test]
    fn collects_deduplicated_paths_with_sources() {
        assert_eq!(
            checks().0,
            vec![
                ("number".to_string(), FieldSource::Fields),
                ("extends".to_string(), FieldSource::Fields),
                ("active".to_string(), FieldSource::Query),
                ("assigned_two.name".to_string(), FieldSource::Query),
                ("sys_created_on".to_string(), FieldSource::OrderBy),
            ]
        );
        assert!(FieldChecks::collect(CheckFields::Off, CheckInputs::default()).is_empty());
        let star = FieldChecks::collect(
            CheckFields::Warn,
            CheckInputs {
                fields: Some("*"),
                ..CheckInputs::default()
            },
        );
        assert!(star.is_empty());
    }

    #[test]
    fn augments_projection_and_reports_injected_paths() {
        let (projection, injected) = checks().augment_projection(Some("sys_id,number".into()));
        assert_eq!(
            projection.as_deref(),
            Some("sys_id,number,extends,active,assigned_two.name,sys_created_on")
        );
        assert_eq!(
            injected,
            ["extends", "active", "assigned_two.name", "sys_created_on"]
        );
        assert_eq!(checks().augment_projection(None), (None, Vec::new()));
    }

    #[test]
    fn missing_reports_absent_keys_and_skips_empty_results() {
        let records = [record(serde_json::json!({
            "number": "INC1", "active": "true", "sys_created_on": "x"
        }))];
        let warnings = checks().missing(&table(), &records, false);
        let fields: Vec<_> = warnings.iter().map(|w| w.field.as_str()).collect();
        assert_eq!(fields, ["extends", "assigned_two.name"]);
        assert!(checks().missing(&table(), &[], false).is_empty());
    }

    #[test]
    fn missing_without_projection_checks_first_segment_only() {
        let records = [record(serde_json::json!({
            "number": "INC1", "active": "true", "assigned_two": "", "extends": "", "sys_created_on": "x"
        }))];
        assert!(checks().missing(&table(), &records, true).is_empty());
    }

    #[test]
    fn strip_injected_removes_only_added_fields() {
        let mut records = [record(serde_json::json!({"number": "1", "active": "true"}))];
        strip_injected(&mut records, &["active".to_string()]);
        assert_eq!(
            serde_json::to_value(&records[0]).unwrap(),
            serde_json::json!({"number": "1"})
        );
    }

    #[test]
    fn error_mode_is_a_validation_error_with_issues() {
        let warnings = [FieldWarning::unknown_field(
            "incident",
            "assigned_two",
            FieldSource::Query,
        )];
        let error = into_error(&table(), &warnings);
        let validation = error.downcast_ref::<ValidationError>().unwrap();
        assert_eq!(validation.issues().len(), 1);
        assert_eq!(validation.issues()[0].kind, "unknown_field");
        assert!(validation.to_string().contains("'assigned_two' (--query)"));
    }
}
