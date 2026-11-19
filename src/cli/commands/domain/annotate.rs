//! `--with-domain` record annotation.
use crate::cli::output::DomainSummary;
use crate::client::{SnowClient, pagination::PaginationConfig};
use crate::models::{identifiers::TableName, record::Record};

/// Adds `sys_domain_name` to each record and returns record counts per domain
/// name. Names come from one `domain` lookup; `global` has no domain record,
/// and names the session cannot read stay null.
pub(crate) async fn annotate_record_domains(
    client: &mut SnowClient,
    records: &mut [Record],
    query_no_domain: bool,
) -> DomainSummary {
    let ids = records
        .iter()
        .filter_map(|record| record.reference_value("sys_domain"))
        .filter(|id| *id != "global")
        .map(str::to_string)
        .collect::<std::collections::BTreeSet<_>>();
    let mut names = std::collections::HashMap::new();
    if !ids.is_empty() {
        let query = format!(
            "sys_idIN{}",
            ids.iter().cloned().collect::<Vec<_>>().join(",")
        );
        match client
            .get_table_records_with_domain(
                &TableName::from_static("domain"),
                Some(&query),
                Some("sys_id,name"),
                &PaginationConfig::default().with_limit(Some(ids.len())),
                None,
                query_no_domain,
            )
            .await
        {
            Ok(result) => {
                for row in result.records {
                    if let (Some(id), Some(name)) = (row.get_str("sys_id"), row.get_str("name")) {
                        names.insert(id.to_string(), name.to_string());
                    }
                }
            }
            Err(error) => tracing::debug!(error = %error, "Could not resolve domain names"),
        }
    }
    let mut summary = DomainSummary::new();
    for record in records.iter_mut() {
        let id = record.reference_value("sys_domain").map(str::to_string);
        let name = match id.as_deref() {
            Some("global") => Some("global".to_string()),
            Some(id) => names.get(id).cloned(),
            None => None,
        };
        let key = name.clone().or(id).unwrap_or_else(|| "(none)".to_string());
        *summary.entry(key).or_insert(0) += 1;
        record
            .fields
            .insert("sys_domain_name".into(), serde_json::json!(name));
    }
    summary
}
