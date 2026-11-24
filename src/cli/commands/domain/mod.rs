//! Domain discovery does not calculate effective access: the platform owns
//! parent/Contains/visibility relationships, ACLs, and process overrides.
mod annotate;
mod counts;
mod metadata;
mod roles;

pub(crate) use annotate::annotate_record_domains;
pub(crate) use metadata::constrain_query;
pub(crate) use roles::warn_if_query_no_domain_ignored;

use crate::cli::args::{DomainArgs, DomainCommands, OutputFormat};
use crate::cli::output;
use crate::client::{SnowClient, pagination::PaginationConfig};
use crate::models::{
    domain::DomainId, identifiers::TableName, record::Record, validation::ValidationError,
};

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
        DomainCommands::List(crate::cli::args::DomainListArgs {
            limit,
            query_no_domain,
        }) => {
            anyhow::ensure!(
                limit > 0,
                ValidationError::new("--limit must be greater than zero")
            );
            warn_if_query_no_domain_ignored(&mut client, query_no_domain).await;
            let result = client
                .get_table_records_with_domain(
                    &TableName::from_static("domain"),
                    None,
                    Some("sys_id,name,parent,active,sys_domain,sys_domain_path"),
                    &PaginationConfig::default().with_limit(Some(limit)),
                    Some(&"name".parse()?),
                    query_no_domain,
                )
                .await
                .map_err(domain_table_missing)?;
            output::print_table_list(&result, format)
        }
        DomainCommands::Inspect(crate::cli::args::DomainInspectArgs {
            domain,
            limit,
            query_no_domain,
        }) => {
            warn_if_query_no_domain_ignored(&mut client, query_no_domain).await;
            inspect(&mut client, &domain, limit, query_no_domain, format).await
        }
        DomainCommands::Table { table, counts } => {
            let metadata = metadata::table_domain_fields(&mut client, &table).await?;
            let counts = match counts {
                Some(scope) => {
                    let connection = counts::ScriptConnection {
                        profile,
                        instance,
                        timeout_secs,
                    };
                    Some(
                        counts::domain_counts(&mut client, &connection, &table, &metadata, scope)
                            .await?,
                    )
                }
                None => None,
            };
            output::print_record(&table_report(&table, &metadata, counts.as_ref())?, format)
        }
        DomainCommands::Status => status(&mut client, format).await,
    }
}

/// The `domain table` result: inherited domain fields and, with `--counts`,
/// the per-domain record counts.
fn table_report(
    table: &TableName,
    metadata: &metadata::TableDomainMetadata,
    counts: Option<&counts::DomainCounts>,
) -> anyhow::Result<Record> {
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
        (
            "has_sys_domain".into(),
            serde_json::json!(metadata.has_sys_domain()),
        ),
        (
            "note".into(),
            serde_json::json!(
                "has_sys_domain is null when the dictionary could not be read. Field presence does not prove activation or effective visibility. sys_overrides indicates process override support."
            ),
        ),
    ]);
    if let Some(counts) = counts {
        fields.insert("counts".into(), serde_json::to_value(counts)?);
    }
    Ok(Record { fields })
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
        .and_then(|user| user.reference_value("sys_domain"))
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
            .await
            .map_err(domain_table_missing)?;
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

/// Rewrites `INVALID_TABLE` from the domain tables into an actionable error:
/// those tables exist only when domain separation is installed.
fn domain_table_missing(error: anyhow::Error) -> anyhow::Error {
    match error.downcast::<crate::client::error::ApiError>() {
        Ok(api) if api.code == "INVALID_TABLE" => crate::client::error::ApiError {
            code: "DOMAIN_SEPARATION_INACTIVE".to_string(),
            message: "Domain separation is not active on this instance (the domain tables do not exist); run `snow-cli domain status` to confirm".to_string(),
            ..api
        }
        .into(),
        Ok(api) => api.into(),
        Err(other) => other,
    }
}

#[cfg(test)]
mod domain_missing_tests {
    use super::domain_table_missing;
    use crate::client::error::ApiError;

    fn api(code: &str) -> anyhow::Error {
        ApiError {
            code: code.to_string(),
            message: "Invalid table domain".to_string(),
            status: 400,
            detail: None,
            instance: "https://example.service-now.com".to_string(),
        }
        .into()
    }

    #[test]
    fn invalid_domain_table_maps_to_inactive_domain_separation() {
        let mapped = domain_table_missing(api("INVALID_TABLE"));
        let api = mapped.downcast_ref::<ApiError>().expect("api error");
        assert_eq!(api.code, "DOMAIN_SEPARATION_INACTIVE");
        assert_eq!(api.status, 400);
    }

    #[test]
    fn other_api_errors_pass_through() {
        let mapped = domain_table_missing(api("SERVER_ERROR"));
        assert_eq!(
            mapped.downcast_ref::<ApiError>().unwrap().code,
            "SERVER_ERROR"
        );
    }
}
