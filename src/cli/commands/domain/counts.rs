//! `domain table --counts`: per-domain record counts from the Aggregate API
//! (visible domains) or one read-only background script (all domains).
use serde::{Deserialize, Serialize};

use super::metadata::TableDomainMetadata;
use super::roles::{CROSS_DOMAIN_ROLES, session_has_any_role};
use crate::cli::args::DomainCountScope;
use crate::client::{AggregateResult, SnowClient, error::TransportError};
use crate::models::{identifiers::TableName, validation::ValidationError};

/// The field the counts are grouped by, in both the Aggregate API and the script.
const GROUP_FIELD: &str = "sys_domain";

/// Connection settings the background-script path re-uses, since script
/// execution builds its own form session.
pub(super) struct ScriptConnection<'a> {
    pub(super) profile: &'a str,
    pub(super) instance: Option<&'a str>,
    pub(super) timeout_secs: Option<u64>,
}

/// The `counts` section of `domain table --counts`.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(super) enum DomainCounts {
    /// The table has no `sys_domain`, so nothing was counted.
    NotSeparated { note: &'static str },
    /// `--counts`: the domains visible to this session.
    Visible(VisibleDomainCounts),
    /// `--counts all`: every domain, via a background script.
    All(AllDomainCounts),
}

#[derive(Debug, Serialize)]
pub(super) struct VisibleDomainCounts {
    source: &'static str,
    scope: &'static str,
    total: usize,
    by_domain: Vec<DomainCount>,
    cross_domain: CrossDomain,
}

/// Field order is part of the output contract: it matches what the command
/// printed before these counts were typed.
#[derive(Debug, Serialize)]
pub(super) struct AllDomainCounts {
    by_domain: Vec<DomainCount>,
    total: usize,
    source: &'static str,
    scope: &'static str,
}

/// One domain's record count.
#[derive(Debug, Serialize, PartialEq, Eq)]
struct DomainCount {
    /// The domain record's own name: the last segment of `display`.
    domain: String,
    /// The reference display value, a domain path such as `TOP/ACME`.
    display: String,
    sys_domain: String,
    count: usize,
}

impl DomainCount {
    /// An empty display value falls back to the sys_id.
    fn new(sys_domain: String, display: Option<String>, count: usize) -> Self {
        let display = display
            .filter(|display| !display.is_empty())
            .unwrap_or_else(|| sys_domain.clone());
        Self {
            domain: domain_name_from_display(&display).to_string(),
            display,
            sys_domain,
            count,
        }
    }
}

/// Records outside the visible domains, measured with two Table API totals.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(untagged)]
enum CrossDomain {
    Measured {
        available: bool,
        visible_total: Option<usize>,
        all_total: Option<usize>,
        hidden: Option<usize>,
    },
    /// `available` is `false` without the role and `null` when the roles
    /// could not be read; `hidden` is always `null`.
    Unavailable {
        available: Option<bool>,
        hidden: Option<usize>,
        reason: &'static str,
    },
}

impl CrossDomain {
    fn measured(visible_total: Option<usize>, all_total: Option<usize>) -> Self {
        Self::Measured {
            available: true,
            visible_total,
            all_total,
            hidden: all_total
                .zip(visible_total)
                .map(|(all, visible)| all.saturating_sub(visible)),
        }
    }

    fn unavailable(available: Option<bool>, reason: &'static str) -> Self {
        Self::Unavailable {
            available,
            hidden: None,
            reason,
        }
    }
}

/// Count `table`'s records per domain for `scope`.
pub(super) async fn domain_counts(
    client: &mut SnowClient,
    connection: &ScriptConnection<'_>,
    table: &TableName,
    metadata: &TableDomainMetadata,
    scope: DomainCountScope,
) -> anyhow::Result<DomainCounts> {
    if metadata.has_sys_domain() == Some(false) {
        return Ok(DomainCounts::NotSeparated {
            note: "The table has no sys_domain field; records are not domain-separated.",
        });
    }
    match scope {
        DomainCountScope::Visible => api_domain_counts(client, table)
            .await
            .map(DomainCounts::Visible),
        DomainCountScope::All => script_domain_counts(client, connection, table)
            .await
            .map(DomainCounts::All),
    }
}

/// A domain reference displays as its path (`TOP/ACME`); the domain record's
/// own name, which `--with-domain` reports, is the last segment.
fn domain_name_from_display(display: &str) -> &str {
    display.rsplit('/').next().unwrap_or(display)
}

/// Parse a group count, which ServiceNow sends as a decimal string.
fn parse_count(count: &serde_json::Value) -> Option<usize> {
    match count {
        serde_json::Value::String(count) => count.trim().parse().ok(),
        serde_json::Value::Number(count) => count.as_u64()?.try_into().ok(),
        _ => None,
    }
}

/// One `sysparm_group_by=sys_domain` Aggregate API group.
#[derive(Deserialize)]
struct AggregateGroup {
    stats: AggregateStats,
    groupby_fields: Vec<GroupByField>,
}

#[derive(Deserialize)]
struct AggregateStats {
    #[serde(default)]
    count: serde_json::Value,
}

#[derive(Deserialize)]
struct GroupByField {
    field: String,
    value: String,
    #[serde(default)]
    display_value: Option<String>,
}

/// Validate one group; a malformed group is an error, never a zero count.
fn domain_count_from_group(group: serde_json::Value) -> Result<DomainCount, TransportError> {
    let group: AggregateGroup = serde_json::from_value(group).map_err(|_| {
        TransportError::InvalidEnvelope(
            "Unexpected Aggregate API response: group is missing 'stats' or 'groupby_fields'",
        )
    })?;
    let count = parse_count(&group.stats.count).ok_or(TransportError::InvalidEnvelope(
        "Unexpected Aggregate API response: group has a missing or invalid count",
    ))?;
    let field = group
        .groupby_fields
        .into_iter()
        .find(|field| field.field == GROUP_FIELD)
        .ok_or(TransportError::InvalidEnvelope(
            "Unexpected Aggregate API response: group lacks the sys_domain groupby field",
        ))?;
    Ok(DomainCount::new(field.value, field.display_value, count))
}

/// Per-domain counts for the domains visible to this session, plus a hidden
/// count when the session may read across domains. The Aggregate API ignores
/// `sysparm_query_no_domain` even with the role, so the hidden count compares
/// two Table API totals instead.
async fn api_domain_counts(
    client: &mut SnowClient,
    table: &TableName,
) -> anyhow::Result<VisibleDomainCounts> {
    let groups = match client
        .aggregate(
            table,
            &[
                ("sysparm_count", "true"),
                ("sysparm_group_by", GROUP_FIELD),
                ("sysparm_display_value", "all"),
            ],
        )
        .await?
    {
        AggregateResult::Grouped(groups) => groups,
        // A grouped query answered with a single object has no groups.
        AggregateResult::Single(_) => Vec::new(),
    };
    let by_domain = groups
        .into_iter()
        .map(domain_count_from_group)
        .collect::<Result<Vec<_>, _>>()?;
    let total = by_domain.iter().map(|row| row.count).sum();

    let cross_domain = match session_has_any_role(client, CROSS_DOMAIN_ROLES).await {
        Some(true) => {
            let visible = client
                .count_table_records_with_domain(table, None, false)
                .await?;
            let all = client
                .count_table_records_with_domain(table, None, true)
                .await?;
            CrossDomain::measured(visible, all)
        }
        Some(false) => CrossDomain::unavailable(
            Some(false),
            "Requires the admin or query_no_domain_table_api role; counts cover only visible domains.",
        ),
        None => CrossDomain::unavailable(
            None,
            "Could not read this session's roles; counts cover only visible domains.",
        ),
    };
    Ok(VisibleDomainCounts {
        source: "aggregate_api",
        scope: "visible_domains",
        total,
        by_domain,
        cross_domain,
    })
}

/// Read-only GlideAggregate across every domain. `queryNoDomain()` lifts the
/// session's domain visibility; the table name is a validated identifier and
/// is JSON-encoded into the script. It prints raw rows; names and totals are
/// derived in Rust so both count paths share one rule.
fn domain_count_script(table: &TableName) -> anyhow::Result<String> {
    let table = serde_json::to_string(table.as_str())?;
    Ok(format!(
        r#"(function () {{
  var ga = new GlideAggregate({table});
  if (!ga.isValid()) {{ gs.print(JSON.stringify({{ ok: false, error: 'Unknown table' }})); return; }}
  ga.queryNoDomain();
  ga.addAggregate('COUNT');
  ga.groupBy('{GROUP_FIELD}');
  ga.query();
  var rows = [];
  while (ga.next()) {{
    rows.push({{
      sys_domain: String(ga.getValue('{GROUP_FIELD}') || ''),
      display: String(ga.getDisplayValue('{GROUP_FIELD}') || ''),
      count: String(ga.getAggregate('COUNT'))
    }});
  }}
  gs.print(JSON.stringify({{ ok: true, rows: rows }}));
}})();"#
    ))
}

/// What [`domain_count_script`] prints.
#[derive(Deserialize)]
struct ScriptOutput {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    rows: Vec<ScriptRow>,
}

#[derive(Deserialize)]
struct ScriptRow {
    sys_domain: String,
    display: String,
    count: serde_json::Value,
}

/// Turn the script's raw rows into counts; a bad count is an error.
fn counts_from_script(output: ScriptOutput) -> anyhow::Result<AllDomainCounts> {
    if !output.ok {
        anyhow::bail!(
            "{}",
            output
                .error
                .as_deref()
                .unwrap_or("domain count script failed")
        );
    }
    let by_domain = output
        .rows
        .into_iter()
        .map(|row| {
            let count = parse_count(&row.count)
                .ok_or_else(|| anyhow::anyhow!("domain count script returned an invalid count"))?;
            Ok(DomainCount::new(row.sys_domain, Some(row.display), count))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(AllDomainCounts {
        total: by_domain.iter().map(|row| row.count).sum(),
        by_domain,
        source: "background_script",
        scope: "all_domains",
    })
}

async fn script_domain_counts(
    client: &mut SnowClient,
    connection: &ScriptConnection<'_>,
    table: &TableName,
) -> anyhow::Result<AllDomainCounts> {
    // Background scripts are admin-only; fail before touching the script
    // endpoint. An unreadable role table leaves the decision to the server.
    anyhow::ensure!(
        session_has_any_role(client, "admin").await != Some(false),
        ValidationError::new(
            "--counts all requires the admin role; use --counts without a value for visible domains"
        )
    );
    let body = crate::cli::commands::script::run_background_script(
        connection.profile,
        connection.instance,
        connection.timeout_secs,
        &domain_count_script(table)?,
        "global",
        None,
    )
    .await?;
    let output: ScriptOutput = serde_json::from_str(&body).map_err(|_| {
        let detail = crate::client::error::safe_error_detail(&body)
            .unwrap_or_else(|| "<empty response>".to_string());
        anyhow::anyhow!("domain count script returned unexpected output: {detail}")
    })?;
    counts_from_script(output)
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
        // Domain names are derived in Rust only.
        assert!(!script.contains("split("));
    }

    #[test]
    fn malformed_groups_are_errors_not_zero_counts() {
        for group in [
            serde_json::json!({"stats": {}, "groupby_fields": [{"field": "sys_domain", "value": "global"}]}),
            serde_json::json!({"stats": {"count": "many"}, "groupby_fields": [{"field": "sys_domain", "value": "global"}]}),
            serde_json::json!({"stats": {"count": "3"}, "groupby_fields": []}),
            serde_json::json!({"stats": {"count": "3"}, "groupby_fields": [{"field": "state", "value": "1"}]}),
            serde_json::json!({"stats": {"count": "3"}, "groupby_fields": [{"field": "sys_domain"}]}),
            serde_json::json!({"groupby_fields": [{"field": "sys_domain", "value": "global"}]}),
        ] {
            let error = domain_count_from_group(group.clone()).unwrap_err();
            assert!(
                matches!(error, TransportError::InvalidEnvelope(_)),
                "{group}"
            );
        }
    }

    #[test]
    fn group_display_falls_back_to_the_sys_id() {
        let row = domain_count_from_group(serde_json::json!({
            "stats": {"count": "3"},
            "groupby_fields": [{"field": "sys_domain", "value": "abc", "display_value": ""}]
        }))
        .unwrap();
        assert_eq!(
            row,
            DomainCount {
                domain: "abc".into(),
                display: "abc".into(),
                sys_domain: "abc".into(),
                count: 3,
            }
        );
    }

    #[test]
    fn counts_serialize_with_the_documented_shapes() {
        let not_separated = DomainCounts::NotSeparated {
            note: "The table has no sys_domain field; records are not domain-separated.",
        };
        assert_eq!(
            serde_json::to_string(&not_separated).unwrap(),
            r#"{"note":"The table has no sys_domain field; records are not domain-separated."}"#
        );

        let row = || DomainCount::new("acme".into(), Some("TOP/ACME".into()), 3);
        let visible = DomainCounts::Visible(VisibleDomainCounts {
            source: "aggregate_api",
            scope: "visible_domains",
            total: 3,
            by_domain: vec![row()],
            cross_domain: CrossDomain::measured(Some(3), None),
        });
        assert_eq!(
            serde_json::to_string(&visible).unwrap(),
            concat!(
                r#"{"source":"aggregate_api","scope":"visible_domains","total":3,"#,
                r#""by_domain":[{"domain":"ACME","display":"TOP/ACME","sys_domain":"acme","count":3}],"#,
                r#""cross_domain":{"available":true,"visible_total":3,"all_total":null,"hidden":null}}"#
            )
        );
        assert_eq!(
            serde_json::to_string(&CrossDomain::unavailable(None, "why")).unwrap(),
            r#"{"available":null,"hidden":null,"reason":"why"}"#
        );

        let output: ScriptOutput = serde_json::from_str(
            r#"{"ok":true,"rows":[{"sys_domain":"acme","display":"TOP/ACME","count":"3"},{"sys_domain":"global","display":"","count":"2"}]}"#,
        )
        .unwrap();
        let all = DomainCounts::All(counts_from_script(output).unwrap());
        assert_eq!(
            serde_json::to_string(&all).unwrap(),
            concat!(
                r#"{"by_domain":[{"domain":"ACME","display":"TOP/ACME","sys_domain":"acme","count":3},"#,
                r#"{"domain":"global","display":"global","sys_domain":"global","count":2}],"#,
                r#""total":5,"source":"background_script","scope":"all_domains"}"#
            )
        );
    }

    #[test]
    fn script_failures_and_bad_counts_are_errors() {
        let failed: ScriptOutput =
            serde_json::from_str(r#"{"ok":false,"error":"Unknown table"}"#).unwrap();
        assert_eq!(
            counts_from_script(failed).err().unwrap().to_string(),
            "Unknown table"
        );
        let bad: ScriptOutput = serde_json::from_str(
            r#"{"ok":true,"rows":[{"sys_domain":"a","display":"A","count":"NaN"}]}"#,
        )
        .unwrap();
        assert!(counts_from_script(bad).is_err());
    }
}
