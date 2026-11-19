//! Inherited domain-field metadata from `sys_dictionary`.
use crate::cli::commands::dictionary;
use crate::client::{SnowClient, pagination::PaginationConfig};
use crate::models::{domain::DomainId, identifiers::TableName, record::Record};

const DOMAIN_FIELDS: [&str; 3] = ["sys_domain", "sys_domain_path", "sys_overrides"];
/// A dictionary element every table inherits. When its row comes back, the
/// dictionary was readable, so a missing `sys_domain` means "not separated"
/// rather than "unknown".
const DICTIONARY_PROBE_FIELD: &str = "sys_id";

pub(super) struct TableDomainMetadata {
    pub(super) hierarchy: Vec<TableName>,
    /// Dictionary rows for the domain fields only (the probe row is removed).
    pub(super) fields: Vec<Record>,
    dictionary_readable: bool,
}

impl TableDomainMetadata {
    /// `Some(true)` when the table or an ancestor defines `sys_domain`,
    /// `Some(false)` when the readable dictionary shows it does not, and
    /// `None` when the dictionary could not be read.
    pub(super) fn has_sys_domain(&self) -> Option<bool> {
        if self
            .fields
            .iter()
            .any(|row| row.get_str("element") == Some("sys_domain"))
        {
            Some(true)
        } else if self.dictionary_readable {
            Some(false)
        } else {
            None
        }
    }
}

pub(super) async fn table_domain_fields(
    client: &mut SnowClient,
    table: &TableName,
) -> anyhow::Result<TableDomainMetadata> {
    let hierarchy = dictionary::fetch_table_hierarchy(client, table).await?;
    let names = hierarchy
        .iter()
        .map(TableName::as_str)
        .collect::<Vec<_>>()
        .join(",");
    let query = format!(
        "nameIN{names}^elementIN{},{DICTIONARY_PROBE_FIELD}",
        DOMAIN_FIELDS.join(",")
    );
    let rows = client
        .get_table_records(
            &TableName::from_static("sys_dictionary"),
            Some(&query),
            Some("name,element,internal_type,reference"),
            &PaginationConfig::default(),
            None,
        )
        .await?;
    let (probe, fields): (Vec<_>, Vec<_>) = rows
        .into_iter()
        .partition(|row| row.get_str("element") == Some(DICTIONARY_PROBE_FIELD));
    Ok(TableDomainMetadata {
        hierarchy,
        fields,
        dictionary_readable: !probe.is_empty(),
    })
}

/// Verify inherited sys_domain metadata before narrowing a read. ServiceNow
/// may ignore invalid query fields, so a failed check must never issue the read.
pub(crate) async fn constrain_query(
    client: &mut SnowClient,
    table: &TableName,
    domain: Option<&DomainId>,
    query: Option<String>,
) -> anyhow::Result<Option<String>> {
    let Some(domain) = domain else {
        return Ok(query);
    };
    let query = domain.apply_to_query(query.as_deref())?;
    let metadata = table_domain_fields(client, table).await?;
    anyhow::ensure!(
        metadata.has_sys_domain() == Some(true),
        "Cannot verify sys_domain on '{table}' or its ancestors. The table may not be domain-separated or metadata may be inaccessible; refusing --domain read."
    );
    Ok(Some(query))
}
