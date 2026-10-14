//! Domain discovery does not calculate effective access: the platform owns
//! parent/Contains/visibility relationships, ACLs, and process overrides.
use crate::cli::args::{DomainArgs, DomainCommands, OutputFormat};
use crate::cli::{commands::dictionary, output};
use crate::client::{SnowClient, pagination::PaginationConfig};
use crate::models::{
    domain::DomainId, identifiers::TableName, record::Record, validation::ValidationError,
};

const DOMAIN_FIELDS: [&str; 3] = ["sys_domain", "sys_domain_path", "sys_overrides"];
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
        } => inspect(&mut client, &domain, limit, query_no_domain, format).await,
        DomainCommands::Table { table } => {
            let (hierarchy, fields) = table_domain_fields(&mut client, &table).await?;
            let data_separated = fields
                .iter()
                .any(|row| row.get_str("element") == Some("sys_domain"));
            output::print_record(
                &Record {
                    fields: std::collections::HashMap::from([
                        ("table".into(), serde_json::json!(table.as_str())),
                        (
                            "hierarchy".into(),
                            serde_json::json!(
                                hierarchy.iter().map(TableName::as_str).collect::<Vec<_>>()
                            ),
                        ),
                        ("domain_fields".into(), serde_json::to_value(&fields)?),
                        (
                            "has_sys_domain".into(),
                            if data_separated {
                                serde_json::json!(true)
                            } else {
                                serde_json::Value::Null
                            },
                        ),
                        (
                            "note".into(),
                            serde_json::json!(
                                "Missing metadata may mean absent fields or restricted access. Field presence does not prove activation or effective visibility. sys_overrides indicates process override support."
                            ),
                        ),
                    ]),
                },
                format,
            )
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
    output::print_record(
        &Record {
            fields: std::collections::HashMap::from([
                ("enabled".into(), serde_json::json!(enabled)),
                ("properties".into(), serde_json::to_value(properties)?),
                (
                    "authenticated_user".into(),
                    serde_json::to_value(user.first())?,
                ),
                ("session_domain".into(), serde_json::Value::Null),
                (
                    "note".into(),
                    serde_json::json!(
                        "sys_user.sys_domain is the home domain, not proof of the current session domain. Missing properties or fields are unknown. Reads retain server domain visibility and ACLs; empty results do not prove absence. Domain separation is independent of application scope."
                    ),
                ),
            ]),
        },
        format,
    )
}

async fn table_domain_fields(
    client: &mut SnowClient,
    table: &TableName,
) -> anyhow::Result<(Vec<TableName>, Vec<Record>)> {
    let hierarchy = dictionary::fetch_table_hierarchy(client, table).await?;
    let names = hierarchy
        .iter()
        .map(TableName::as_str)
        .collect::<Vec<_>>()
        .join(",");
    let query = format!("nameIN{names}^elementIN{}", DOMAIN_FIELDS.join(","));
    let fields = client
        .get_table_records(
            &TableName::from_static("sys_dictionary"),
            Some(&query),
            Some("name,element,internal_type,reference"),
            &PaginationConfig::default(),
            None,
        )
        .await?;
    Ok((hierarchy, fields))
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
    let (_, fields) = table_domain_fields(client, table).await?;
    anyhow::ensure!(
        fields
            .iter()
            .any(|row| row.get_str("element") == Some("sys_domain")),
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
            fields: std::collections::HashMap::from([
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
