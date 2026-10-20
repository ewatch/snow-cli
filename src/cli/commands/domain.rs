//! Domain discovery does not calculate effective access: the platform owns
//! parent/Contains/visibility relationships, ACLs, and process overrides.
use crate::cli::args::{DomainArgs, DomainCommands, OutputFormat};
use crate::cli::{commands::dictionary, output};
use crate::client::{SnowClient, pagination::PaginationConfig};
use crate::models::{
    domain::DomainId, identifiers::TableName, record::Record, validation::ValidationError,
};

const DOMAIN_FIELDS: [&str; 3] = ["sys_domain", "sys_domain_path", "sys_overrides"];
/// A dictionary element every table inherits. When its row comes back, the
/// dictionary was readable, so a missing `sys_domain` means "not separated"
/// rather than "unknown".
const DICTIONARY_PROBE_FIELD: &str = "sys_id";
/// Roles that let the Table API honour `sysparm_query_no_domain`. `admin`
/// implies every role, so an admin never needs the explicit grant.
const CROSS_DOMAIN_ROLES: &str = "admin,query_no_domain_table_api";
// Observed on domain-separated instances; missing/hidden properties stay unknown.
const DOMAIN_PROPERTIES: [&str; 6] = [
    "glide.sys.domain.enabled",
    "glide.sys.domain.provider",
    "glide.sys.domain.paths.installed",
    "glide.sys.domain.use_record_domain_for_data",
    "glide.sys.domain.use_record_domain_for_processes",
    "glide.sys.domain.delegated_administration",
];

pub async fn handle(
    args: DomainArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
    match args.command {
        DomainCommands::List {
            limit,
            query_no_domain,
        } => {
            anyhow::ensure!(
                limit > 0,
                ValidationError::new("--limit must be greater than zero")
            );
            if query_no_domain {
                warn_if_query_no_domain_ignored(&mut client).await;
            }
            let result = client
                .get_table_records_with_domain(
                    &TableName::from_static("domain"),
                    None,
                    Some("sys_id,name,parent,active,sys_domain,sys_domain_path"),
                    &PaginationConfig::default().with_limit(Some(limit)),
                    Some(&"name".parse()?),
                    query_no_domain,
                )
                .await?;
            output::print_table_list(&result, format)
        }
        DomainCommands::Inspect {
            domain,
            limit,
            query_no_domain,
        } => {
            if query_no_domain {
                warn_if_query_no_domain_ignored(&mut client).await;
            }
            inspect(&mut client, &domain, limit, query_no_domain, format).await
        }
        DomainCommands::Table {
            table,
            counts,
            via_script,
        } => {
            let metadata = table_domain_fields(&mut client, &table).await?;
            let has_sys_domain = metadata.has_sys_domain();
            let mut fields = std::collections::BTreeMap::from([
                ("table".into(), serde_json::json!(table.as_str())),
                (
                    "hierarchy".into(),
                    serde_json::json!(
                        metadata
                            .hierarchy
                            .iter()
                            .map(TableName::as_str)
                            .collect::<Vec<_>>()
                    ),
                ),
                (
                    "domain_fields".into(),
                    serde_json::to_value(&metadata.fields)?,
                ),
                ("has_sys_domain".into(), serde_json::json!(has_sys_domain)),
                (
                    "note".into(),
                    serde_json::json!(
                        "has_sys_domain is null when the dictionary could not be read. Field presence does not prove activation or effective visibility. sys_overrides indicates process override support."
                    ),
                ),
            ]);
            if counts {
                let counts = if has_sys_domain == Some(false) {
                    serde_json::json!({"note": "The table has no sys_domain field; records are not domain-separated."})
                } else if via_script {
                    script_domain_counts(&mut client, profile, instance, timeout_secs, &table)
                        .await?
                } else {
                    api_domain_counts(&mut client, &table).await?
                };
                fields.insert("counts".into(), counts);
            }
            output::print_record(&Record { fields }, format)
        }
        DomainCommands::Status => status(&mut client, format).await,
    }
}

async fn status(client: &mut SnowClient, format: &OutputFormat) -> anyhow::Result<()> {
    let pagination = PaginationConfig::default().with_limit(Some(20));
    // Authentication/transport errors remain errors; never label them disabled.
    let properties = client
        .get_table_records(
            &TableName::from_static("sys_properties"),
            Some(&format!("nameIN{}", DOMAIN_PROPERTIES.join(","))),
            Some("name,value"),
            &pagination,
            None,
        )
        .await?;
    let enabled = properties
        .iter()
        .find(|row| row.get_str("name") == Some("glide.sys.domain.enabled"))
        .and_then(|row| row.get_str("value"))
        .and_then(|value| match value {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        });
    let user = client
        .get_table_records(
            &TableName::from_static("sys_user"),
            Some("sys_id=javascript:gs.getUserID()"),
            Some("sys_id,user_name,sys_domain"),
            &PaginationConfig::default().with_limit(Some(1)),
            None,
        )
        .await?;
    // A home domain other than global proves separation is in use even when
    // the session cannot read sys_properties (e.g. itil users).
    let separation_in_use = user
        .first()
        .and_then(|user| user.fields.get("sys_domain"))
        .and_then(reference_value)
        .filter(|domain| *domain != "global")
        .map(|_| true);
    output::print_record(
        &Record {
            fields: std::collections::BTreeMap::from([
                ("enabled".into(), serde_json::json!(enabled)),
                (
                    "separation_in_use".into(),
                    serde_json::json!(separation_in_use),
                ),
                ("properties".into(), serde_json::to_value(properties)?),
                (
                    "authenticated_user".into(),
                    serde_json::to_value(user.first())?,
                ),
                ("session_domain".into(), serde_json::Value::Null),
                (
                    "note".into(),
                    serde_json::json!(
                        "sys_user.sys_domain is the home domain, not proof of the current session domain. separation_in_use is true when that home domain is not global, and null otherwise. Missing properties or fields are unknown. Reads retain server domain visibility and ACLs; empty results do not prove absence. Domain separation is independent of application scope."
                    ),
                ),
            ]),
        },
        format,
    )
}

struct TableDomainMetadata {
    hierarchy: Vec<TableName>,
    /// Dictionary rows for the domain fields only (the probe row is removed).
    fields: Vec<Record>,
    dictionary_readable: bool,
}

impl TableDomainMetadata {
    fn has_sys_domain(&self) -> Option<bool> {
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

async fn table_domain_fields(
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

/// Report observed relationships without inventing the server's effective access set.
async fn inspect(
    client: &mut SnowClient,
    domain: &DomainId,
    limit: usize,
    query_no_domain: bool,
    format: &OutputFormat,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        limit > 0,
        ValidationError::new("--limit must be greater than zero")
    );
    let mut relationships = serde_json::Map::new();
    for (table, query, fields) in [
        (
            "domain",
            format!("sys_id={domain}^ORparent={domain}"),
            "sys_id,name,parent,active",
        ),
        (
            "domain_contains",
            format!("domain={domain}^ORcontains={domain}"),
            "sys_id,domain,contains",
        ),
        (
            "sys_user_visibility",
            format!("sys_domain={domain}"),
            "sys_id,user,sys_domain,inherited,granted_by",
        ),
        (
            "sys_user_group_visibility",
            format!("sys_domain={domain}"),
            "sys_id,group,sys_domain",
        ),
    ] {
        let result = client
            .get_table_records_with_domain(
                &table.parse()?,
                Some(&query),
                Some(fields),
                &PaginationConfig::default().with_limit(Some(limit)),
                None,
                query_no_domain,
            )
            .await?;
        relationships.insert(table.to_string(), serde_json::json!({
            "records": result.records,
            "meta": {"returned": result.returned(), "total": result.total,
                "truncated": result.truncated, "offset": result.offset, "next_offset": result.next_offset()},
        }));
    }
    output::print_record(
        &Record {
            fields: std::collections::BTreeMap::from([
                ("domain_id".into(), serde_json::json!(domain.to_string())),
                (
                    "relationships".into(),
                    serde_json::Value::Object(relationships),
                ),
                (
                    "note".into(),
                    serde_json::json!(
                        "Observed direct relationships only, subject to server visibility and ACLs. This is not an effective access calculation. Global may have no domain record. Contains affects data, not process inheritance; explicit user/group grants can persist across picker selection."
                    ),
                ),
            ]),
        },
        format,
    )
}

/// A reference field's sys_id, whether returned as a link object or a string.
fn reference_value(value: &serde_json::Value) -> Option<&str> {
    match value {
        serde_json::Value::String(value) => Some(value.as_str()),
        serde_json::Value::Object(object) => object.get("value").and_then(|value| value.as_str()),
        _ => None,
    }
    .filter(|value| !value.is_empty())
}

/// Whether this session holds any of `roles` (comma-separated). `None` when
/// the role table could not be read, so callers report "unknown".
async fn session_has_any_role(client: &mut SnowClient, roles: &str) -> Option<bool> {
    let query = format!("user=javascript:gs.getUserID()^role.nameIN{roles}^state=active");
    match client
        .get_table_records(
            &TableName::from_static("sys_user_has_role"),
            Some(&query),
            Some("sys_id"),
            &PaginationConfig::default().with_limit(Some(1)),
            None,
        )
        .await
    {
        Ok(rows) => Some(!rows.is_empty()),
        Err(error) => {
            tracing::debug!(error = %error, "Could not read session roles");
            None
        }
    }
}

/// ServiceNow silently ignores `sysparm_query_no_domain` without the role, so
/// say so instead of letting a visible-only result pass as complete.
pub(crate) async fn warn_if_query_no_domain_ignored(client: &mut SnowClient) {
    if session_has_any_role(client, CROSS_DOMAIN_ROLES).await == Some(false) {
        tracing::warn!(
            "--query-no-domain has no effect without the admin or query_no_domain_table_api role; results cover only the domains visible to this session"
        );
    }
}

/// A domain reference displays as its path (`TOP/ACME`); the domain record's
/// own name, which `--with-domain` reports, is the last segment.
fn domain_name_from_display(display: &str) -> &str {
    display.rsplit('/').next().unwrap_or(display)
}

/// Total matching rows from one Table API page (`X-Total-Count`).
async fn table_total(
    client: &mut SnowClient,
    table: &TableName,
    query_no_domain: bool,
) -> anyhow::Result<Option<usize>> {
    Ok(client
        .get_table_records_with_domain(
            table,
            None,
            Some("sys_id"),
            &PaginationConfig::default().with_limit(Some(1)),
            None,
            query_no_domain,
        )
        .await?
        .total)
}

/// Per-domain counts for the domains visible to this session, plus a hidden
/// count when the session may read across domains. The Aggregate API ignores
/// `sysparm_query_no_domain` even with the role, so the hidden count compares
/// two Table API totals instead.
async fn api_domain_counts(
    client: &mut SnowClient,
    table: &TableName,
) -> anyhow::Result<serde_json::Value> {
    let response: serde_json::Value = client
        .get_json_with_params(
            &format!("/api/now/stats/{table}"),
            &[
                ("sysparm_count", "true"),
                ("sysparm_group_by", "sys_domain"),
                ("sysparm_display_value", "all"),
            ],
        )
        .await?;
    let groups = match response.get("result") {
        Some(serde_json::Value::Array(groups)) => groups.clone(),
        Some(serde_json::Value::Object(_)) => Vec::new(),
        _ => {
            return Err(crate::client::error::TransportError::InvalidEnvelope(
                "Unexpected Aggregate API response: missing 'result'",
            )
            .into());
        }
    };
    let mut total = 0usize;
    let by_domain = groups
        .iter()
        .map(|group| {
            let field = &group["groupby_fields"][0];
            let count = group["stats"]["count"]
                .as_str()
                .and_then(|count| count.parse::<usize>().ok())
                .unwrap_or(0);
            total += count;
            let sys_domain = field["value"].as_str().unwrap_or_default();
            let display = field["display_value"]
                .as_str()
                .filter(|display| !display.is_empty())
                .unwrap_or(sys_domain);
            serde_json::json!({
                "domain": domain_name_from_display(display),
                "display": display,
                "sys_domain": sys_domain,
                "count": count,
            })
        })
        .collect::<Vec<_>>();

    let cross_domain = match session_has_any_role(client, CROSS_DOMAIN_ROLES).await {
        Some(true) => {
            let visible = table_total(client, table, false).await?;
            let all = table_total(client, table, true).await?;
            let hidden = all
                .zip(visible)
                .map(|(all, visible)| all.saturating_sub(visible));
            serde_json::json!({"available": true, "visible_total": visible, "all_total": all, "hidden": hidden})
        }
        Some(false) => serde_json::json!({
            "available": false,
            "hidden": null,
            "reason": "Requires the admin or query_no_domain_table_api role; counts cover only visible domains."
        }),
        None => serde_json::json!({
            "available": null,
            "hidden": null,
            "reason": "Could not read this session's roles; counts cover only visible domains."
        }),
    };
    Ok(serde_json::json!({
        "source": "aggregate_api",
        "scope": "visible_domains",
        "total": total,
        "by_domain": by_domain,
        "cross_domain": cross_domain,
    }))
}

/// Read-only GlideAggregate across every domain. `queryNoDomain()` lifts the
/// session's domain visibility; the table name is a validated identifier and
/// is JSON-encoded into the script.
fn domain_count_script(table: &TableName) -> anyhow::Result<String> {
    let table = serde_json::to_string(table.as_str())?;
    Ok(format!(
        r#"(function () {{
  var ga = new GlideAggregate({table});
  if (!ga.isValid()) {{ gs.print(JSON.stringify({{ ok: false, error: 'Unknown table' }})); return; }}
  ga.queryNoDomain();
  ga.addAggregate('COUNT');
  ga.groupBy('sys_domain');
  ga.query();
  var rows = [], total = 0;
  while (ga.next()) {{
    var count = parseInt(ga.getAggregate('COUNT'), 10);
    var id = ga.getValue('sys_domain') || '';
    var display = ga.getDisplayValue('sys_domain') || id;
    rows.push({{ domain: display.split('/').pop(), display: display, sys_domain: id, count: count }});
    total += count;
  }}
  gs.print(JSON.stringify({{ ok: true, total: total, by_domain: rows }}));
}})();"#
    ))
}

async fn script_domain_counts(
    client: &mut SnowClient,
    profile: &str,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    table: &TableName,
) -> anyhow::Result<serde_json::Value> {
    // Background scripts are admin-only; fail before touching the script
    // endpoint. An unreadable role table leaves the decision to the server.
    anyhow::ensure!(
        session_has_any_role(client, "admin").await != Some(false),
        ValidationError::new(
            "--via-script requires the admin role; use --counts without it for visible domains"
        )
    );
    let body = crate::cli::commands::script::run_background_script(
        profile,
        instance,
        timeout_secs,
        &domain_count_script(table)?,
        "global",
        None,
    )
    .await?;
    let mut payload: serde_json::Value = serde_json::from_str(&body).map_err(|_| {
        let detail = crate::client::error::safe_error_detail(&body);
        anyhow::anyhow!("domain count script returned non-JSON output: {detail}")
    })?;
    if payload["ok"] != serde_json::Value::Bool(true) {
        let error = payload["error"]
            .as_str()
            .unwrap_or("domain count script failed");
        anyhow::bail!("{error}");
    }
    let object = payload
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("domain count script returned a non-object"))?;
    object.remove("ok");
    object.insert("source".into(), serde_json::json!("background_script"));
    object.insert("scope".into(), serde_json::json!("all_domains"));
    Ok(payload)
}

/// Adds `sys_domain_name` to each record and returns record counts per domain
/// name. Names come from one `domain` lookup; `global` has no domain record,
/// and names the session cannot read stay null.
pub(crate) async fn annotate_record_domains(
    client: &mut SnowClient,
    records: &mut [Record],
    query_no_domain: bool,
) -> std::collections::BTreeMap<String, usize> {
    let ids = records
        .iter()
        .filter_map(|record| record.fields.get("sys_domain").and_then(reference_value))
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
    let mut summary = std::collections::BTreeMap::new();
    for record in records.iter_mut() {
        let id = record
            .fields
            .get("sys_domain")
            .and_then(reference_value)
            .map(str::to_string);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_names_match_the_with_domain_summary() {
        assert_eq!(domain_name_from_display("TOP/ACME"), "ACME");
        assert_eq!(
            domain_name_from_display("TOP/MSP/Technicians"),
            "Technicians"
        );
        assert_eq!(domain_name_from_display("global"), "global");
    }

    #[test]
    fn domain_count_script_is_read_only_and_encodes_the_table() {
        let script = domain_count_script(&TableName::from_static("incident")).unwrap();
        assert!(script.contains(r#"new GlideAggregate("incident")"#));
        assert!(script.contains("queryNoDomain()"));
        for mutation in ["update(", "insert(", "deleteRecord", "setValue"] {
            assert!(!script.contains(mutation), "{mutation}");
        }
    }
}
