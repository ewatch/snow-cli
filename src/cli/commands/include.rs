//! `include show`: script include source by name, with parsed inheritance.
//!
//! Reads `sys_script_include` through the ordinary Table API client (no new
//! HTTP path). Inheritance is parsed from the source, because the table has
//! no `extends` column; `--grep` slices the source client-side so an agent
//! never re-downloads a long script to look at another window.

use serde::Serialize;

use crate::cli::args::{IncludeArgs, IncludeCommands, OutputFormat};
use crate::cli::output;
use crate::client::SnowClient;
use crate::client::pagination::PaginationConfig;
use crate::error::CliError;
use crate::models::identifiers::{RecordKey, TableName};
use crate::models::record::Record;
use crate::models::script_outline;
use crate::models::script_source::{self, GrepSlice};
use crate::models::validation::{ValidationError, ValidationIssue};

/// Maximum `--chain` length, handler included. Real script include
/// hierarchies are two or three levels deep (`X` -> `XSNC` -> base).
const MAX_CHAIN_DEPTH: usize = 5;

/// Candidates listed when a name exists in several scopes.
const MAX_CANDIDATES: usize = 5;

/// Fields read for every script include.
const INCLUDE_FIELDS: &str =
    "sys_id,name,api_name,active,access,client_callable,sys_scope,sys_scope.scope,script";

/// Compiled `--grep` patterns are bounded so a pathological pattern cannot
/// exhaust memory.
const GREP_REGEX_SIZE_LIMIT: usize = 1 << 20;

/// Shared bound on `--source-of` lines across a whole chain, so one large
/// member list cannot reproduce every script.
const MAX_SOURCE_OF_LINES: usize = 400;

/// What `include show` prints for one script include.
#[derive(Debug, Serialize)]
struct IncludeView {
    name: String,
    api_name: String,
    sys_id: String,
    scope: String,
    active: bool,
    access: String,
    client_callable: bool,
    /// Parent class parsed from `Object.extendsObject(...)`, or null.
    extends: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    extends_source: Option<&'static str>,
    /// With `--chain`: false when the parent could not be found by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    extends_found: Option<bool>,
    script_lines: usize,
    /// Prototype members, e.g. `152-187 canMoveTo(toState)`.
    outline: Vec<String>,
    /// Without `--chain`: candidate classes this source names.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    references: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    source_of: Vec<MemberSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    grep: Option<GrepSlice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    script: Option<String>,
}

/// One `--source-of` member body.
#[derive(Debug, Serialize)]
struct MemberSource {
    member: String,
    /// 1-based inclusive line range in the script, e.g. `152-187`.
    lines: String,
    source: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    truncated: bool,
}

/// `include show --chain` (or several names) output.
#[derive(Debug, Serialize)]
struct ChainView {
    /// Each requested include followed by its parents (with `--chain`).
    includes: Vec<IncludeView>,
    /// Classes named by the sources that are not themselves listed.
    references: Vec<String>,
    /// `--source-of` members that no class in the chain declares.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    members_not_found: Vec<String>,
    /// Whether `--grep` or `--source-of` output stopped at its line budget.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    truncated: bool,
}

/// How each source is rendered.
struct Render {
    full_script: bool,
    grep: Option<(regex::Regex, usize)>,
    source_of: Vec<String>,
}

/// Line budgets shared by every class of one command.
struct Budget {
    grep_lines: usize,
    source_lines: usize,
    truncated: bool,
}

pub async fn handle(
    args: IncludeArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    match args.command {
        IncludeCommands::Show {
            names,
            chain,
            source_of,
            grep,
            context,
            script,
            no_script,
        } => {
            tracing::info!(includes = names.len(), chain, "Showing script includes");
            let full_script =
                script || !(chain || no_script || grep.is_some() || !source_of.is_empty());
            let render = Render {
                full_script,
                grep: grep
                    .map(|pattern| compile_grep(&pattern).map(|regex| (regex, context)))
                    .transpose()?,
                source_of: source_of
                    .into_iter()
                    .map(|member| member.trim().to_string())
                    .filter(|member| !member.is_empty())
                    .collect(),
            };
            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

            let several = names.len() > 1;
            let mut records: Vec<Record> = Vec::new();
            for name in &names {
                let record = find_include(&mut client, name, None).await?;
                if records
                    .iter()
                    .any(|existing| existing.sys_id() == record.sys_id())
                {
                    continue;
                }
                let mut links = vec![record];
                if chain {
                    follow_chain(&mut client, &mut links, &records).await?;
                }
                records.append(&mut links);
            }
            let mut budget = Budget {
                grep_lines: script_source::MAX_GREP_LINES,
                source_lines: MAX_SOURCE_OF_LINES,
                truncated: false,
            };
            let mut views: Vec<IncludeView> = records
                .iter()
                .map(|record| render_view(record, &render, chain, &mut budget))
                .collect();
            if chain || several {
                let found: Vec<&str> = views
                    .iter()
                    .flat_map(|view| view.source_of.iter().map(|source| source.member.as_str()))
                    .collect();
                let members_not_found = render
                    .source_of
                    .iter()
                    .filter(|member| !found.contains(&member.as_str()))
                    .cloned()
                    .collect();
                let references = chain_references(&records, &views);
                print_chain(
                    ChainView {
                        includes: views,
                        references,
                        members_not_found,
                        truncated: budget.truncated,
                    },
                    format,
                )
            } else {
                let view = views.remove(0);
                print_single(&view, format)
            }
        }
    }
}

/// Candidate classes named anywhere in the chain, minus the chain itself.
fn chain_references(records: &[Record], views: &[IncludeView]) -> Vec<String> {
    let members: Vec<&str> = views
        .iter()
        .flat_map(|view| [view.name.as_str(), view.api_name.as_str()])
        .collect();
    let mut references: Vec<String> = Vec::new();
    for record in records {
        for reference in script_outline::references(text(record, "script")) {
            if !members.contains(&reference.as_str()) && !references.contains(&reference) {
                references.push(reference);
            }
        }
    }
    references.truncate(script_outline::MAX_REFERENCES);
    references
}

fn compile_grep(pattern: &str) -> anyhow::Result<regex::Regex> {
    regex::RegexBuilder::new(pattern)
        .size_limit(GREP_REGEX_SIZE_LIMIT)
        .build()
        .map_err(|_| {
            ValidationError::new(
                "Invalid --grep pattern; use regex syntax such as 'canMoveTo|_resetModel'",
            )
            .into()
        })
}

/// Append each parent named by `extends` until none, a cycle, the depth
/// limit, or a class already shown (`shown`, from an earlier name). A parent
/// that cannot be found ends the chain (marked on the child).
async fn follow_chain(
    client: &mut SnowClient,
    chain: &mut Vec<Record>,
    shown: &[Record],
) -> anyhow::Result<()> {
    while chain.len() < MAX_CHAIN_DEPTH {
        let Some(child) = chain.last() else {
            break;
        };
        let Some(parent) = script_source::parse_extends(text(child, "script")) else {
            break;
        };
        // Already listed (an earlier name's chain, or a cycle): no request.
        let listed = shown
            .iter()
            .chain(chain.iter())
            .any(|record| text(record, "name") == parent || text(record, "api_name") == parent);
        if listed {
            break;
        }
        let child_scope = text(child, "sys_scope.scope").to_string();
        let Ok(key) = parent.parse::<RecordKey>() else {
            break;
        };
        match find_include(client, &key, Some(&child_scope)).await {
            Ok(record) => {
                if shown
                    .iter()
                    .any(|existing| existing.sys_id() == record.sys_id())
                {
                    break;
                }
                let seen = chain
                    .iter()
                    .any(|existing| existing.sys_id() == record.sys_id());
                if seen {
                    tracing::warn!(include = %parent, "Stopping script include chain at a cycle");
                    break;
                }
                chain.push(record);
            }
            Err(error) if is_not_found(&error) => {
                if let Some(child) = chain.last_mut() {
                    child
                        .fields
                        .insert(EXTENDS_NOT_FOUND.to_string(), serde_json::Value::Bool(true));
                }
                break;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Internal marker on a chain link whose parent was not found.
const EXTENDS_NOT_FOUND: &str = "__extends_not_found";

fn is_not_found(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<CliError>(),
        Some(CliError::Api { status: 404, .. })
    )
}

/// Look up one script include by sys_id, api_name (`scope.Name`), or name.
///
/// When a name exists in several scopes, `prefer_scope` (the child's scope
/// while following `--chain`) and then `global` break the tie; without a
/// preference the ambiguity is a validation error listing the candidates.
async fn find_include(
    client: &mut SnowClient,
    key: &RecordKey,
    prefer_scope: Option<&str>,
) -> anyhow::Result<Record> {
    let query = match key {
        RecordKey::SysId(sys_id) => format!("sys_id={sys_id}"),
        RecordKey::Name(name) if name.as_str().contains('.') => format!("api_name={name}"),
        RecordKey::Name(name) => format!("name={name}"),
    };
    let table = TableName::from_static("sys_script_include");
    let pagination = PaginationConfig::default()
        .with_page_size(MAX_CANDIDATES + 1)
        .with_limit(Some(MAX_CANDIDATES + 1));
    let mut records = client
        .get_table_records(
            &table,
            Some(&query),
            Some(INCLUDE_FIELDS),
            &pagination,
            None,
        )
        .await?;

    if records.len() > 1
        && prefer_scope.is_some()
        && let Some(index) = [prefer_scope, Some("global")]
            .into_iter()
            .flatten()
            .find_map(|scope| {
                records
                    .iter()
                    .position(|record| text(record, "sys_scope.scope") == scope)
            })
    {
        return Ok(records.swap_remove(index));
    }

    match records.len() {
        0 => Err(CliError::Api {
            code: "RECORD_NOT_FOUND",
            message: format!(
                "No script include named '{key}'. Try `table list sys_script_include --query 'nameLIKE<text>' --fields name,api_name`."
            ),
            status: 404,
            detail: None,
            instance: client.base_url().to_string(),
        }
        .into()),
        1 => Ok(records.remove(0)),
        count => {
            let issues = records
                .iter()
                .take(MAX_CANDIDATES)
                .map(|record| ValidationIssue {
                    kind: "ambiguous_record",
                    field: record.sys_id().map(str::to_string),
                    record_index: None,
                    message: format!(
                        "candidate {} (sys_id {})",
                        text(record, "api_name"),
                        record.sys_id().unwrap_or("?")
                    ),
                })
                .collect();
            Err(ValidationError::new(format!(
                "{} script includes are named '{key}'; pass the api_name (scope.Name) or sys_id",
                count.min(MAX_CANDIDATES)
            ))
            .with_issues(issues)
            .into())
        }
    }
}

/// A string field of `record`, or "" (reference objects yield their value).
fn text<'a>(record: &'a Record, field: &str) -> &'a str {
    record
        .get_str(field)
        .or_else(|| record.reference_value(field))
        .unwrap_or_default()
}

fn render_view(record: &Record, render: &Render, chain: bool, budget: &mut Budget) -> IncludeView {
    let script = text(record, "script");
    let extends = script_source::parse_extends(script);
    let extends_found =
        (chain && extends.is_some()).then(|| !record.fields.contains_key(EXTENDS_NOT_FOUND));
    let members = script_outline::outline(script);
    let source_of = member_sources(script, &members, &render.source_of, budget);
    let grep = render.grep.as_ref().map(|(pattern, context)| {
        let slice = script_source::grep_lines(
            script,
            |line| pattern.is_match(line),
            *context,
            budget.grep_lines,
        );
        budget.grep_lines = budget.grep_lines.saturating_sub(slice.lines.len());
        budget.truncated |= slice.truncated;
        slice
    });
    let name = text(record, "name").to_string();
    let references = if chain {
        Vec::new()
    } else {
        script_outline::references(script)
            .into_iter()
            .filter(|reference| *reference != name)
            .collect()
    };
    IncludeView {
        api_name: text(record, "api_name").to_string(),
        sys_id: record.sys_id().unwrap_or_default().to_string(),
        scope: text(record, "sys_scope.scope").to_string(),
        active: text(record, "active") == "true",
        access: text(record, "access").to_string(),
        client_callable: text(record, "client_callable") == "true",
        extends_source: extends.as_ref().map(|_| "parsed"),
        extends,
        extends_found: extends_found.filter(|found| !found),
        script_lines: script.lines().count(),
        outline: members
            .iter()
            .map(script_outline::Member::summary)
            .collect(),
        references,
        source_of,
        grep,
        script: render.full_script.then(|| script.to_string()),
        name,
    }
}

/// Source of each requested member this script declares, within the budget.
fn member_sources(
    script: &str,
    members: &[script_outline::Member],
    wanted: &[String],
    budget: &mut Budget,
) -> Vec<MemberSource> {
    if wanted.is_empty() {
        return Vec::new();
    }
    let lines: Vec<&str> = script.lines().collect();
    members
        .iter()
        .filter(|member| wanted.contains(&member.name))
        .map(|member| {
            let from = member.start_line.saturating_sub(1);
            let to = member.end_line.min(lines.len());
            let available = to.saturating_sub(from);
            let shown = available.min(budget.source_lines);
            budget.source_lines -= shown;
            let truncated = shown < available;
            budget.truncated |= truncated;
            MemberSource {
                member: member.name.clone(),
                lines: format!("{}-{}", member.start_line, member.end_line),
                source: lines.get(from..from + shown).unwrap_or_default().join("\n"),
                truncated,
            }
        })
        .collect()
}

fn print_single(view: &IncludeView, format: &OutputFormat) -> anyhow::Result<()> {
    if matches!(format, OutputFormat::Csv) {
        return print_csv_rows(std::slice::from_ref(view), format);
    }
    output::print_output(view, format)
}

fn print_chain(chain: ChainView, format: &OutputFormat) -> anyhow::Result<()> {
    if matches!(format, OutputFormat::Csv) {
        // CSV has no envelope: one row per class, lists as JSON.
        return print_csv_rows(&chain.includes, format);
    }
    output::print_output(&chain, format)
}

fn print_csv_rows(views: &[IncludeView], format: &OutputFormat) -> anyhow::Result<()> {
    let records = views
        .iter()
        .map(|view| serde_json::from_value::<Record>(serde_json::to_value(view)?))
        .collect::<Result<Vec<_>, _>>()?;
    output::print_records(&records, format)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(value: serde_json::Value) -> Record {
        serde_json::from_value(value).unwrap()
    }

    fn handler() -> Record {
        record(serde_json::json!({
            "sys_id": "7d12a0e0c0a8016401c5a33be04be441",
            "name": "ChangeRequestStateHandler",
            "api_name": "global.ChangeRequestStateHandler",
            "active": "true",
            "access": "public",
            "client_callable": "false",
            "sys_scope": {"link": "x", "value": "global"},
            "sys_scope.scope": "global",
            "script": "var ChangeRequestStateHandler = Class.create();\nChangeRequestStateHandler.prototype = Object.extendsObject(ChangeRequestStateHandlerSNC, {\n    initialize: function(gr) {},\n    canMoveTo: function(state) {\n        return true;\n    },\n    type: 'ChangeRequestStateHandler'\n});"
        }))
    }

    fn mode(full_script: bool, grep: Option<(&str, usize)>, source_of: &[&str]) -> Render {
        Render {
            full_script,
            grep: grep.map(|(pattern, context)| (compile_grep(pattern).unwrap(), context)),
            source_of: source_of.iter().map(|member| member.to_string()).collect(),
        }
    }

    fn budget() -> Budget {
        Budget {
            grep_lines: script_source::MAX_GREP_LINES,
            source_lines: MAX_SOURCE_OF_LINES,
            truncated: false,
        }
    }

    #[test]
    fn render_reports_parsed_extends_outline_and_script() {
        let view = render_view(&handler(), &mode(true, None, &[]), false, &mut budget());
        assert_eq!(
            view.extends.as_deref(),
            Some("ChangeRequestStateHandlerSNC")
        );
        assert_eq!(view.extends_source, Some("parsed"));
        assert_eq!(view.outline, ["3 initialize(gr)", "4-6 canMoveTo(state)"]);
        assert_eq!(view.scope, "global");
        assert!(view.active);
        assert!(!view.client_callable);
        assert_eq!(view.script_lines, 8);
        assert!(view.script.is_some());
        assert!(view.grep.is_none());
        let json = serde_json::to_value(&view).unwrap();
        assert!(json.get("extends_found").is_none());
        assert!(json.get("source_of").is_none());
    }

    #[test]
    fn render_grep_replaces_script_with_slices() {
        let render = mode(false, Some(("canMoveTo", 1)), &[]);
        let view = render_view(&handler(), &render, false, &mut budget());
        assert!(view.script.is_none());
        let grep = view.grep.unwrap();
        assert_eq!(grep.match_count, 1);
        assert_eq!(
            grep.lines,
            [
                "3-    initialize: function(gr) {},",
                "4:    canMoveTo: function(state) {",
                "5-        return true;"
            ]
        );
    }

    #[test]
    fn render_source_of_returns_member_bodies_within_budget() {
        let render = mode(false, None, &["canMoveTo", "missing"]);
        let view = render_view(&handler(), &render, true, &mut budget());
        assert_eq!(view.source_of.len(), 1);
        assert_eq!(view.source_of[0].lines, "4-6");
        assert_eq!(
            view.source_of[0].source,
            "    canMoveTo: function(state) {\n        return true;\n    },"
        );

        let mut tight = budget();
        tight.source_lines = 2;
        let view = render_view(&handler(), &render, true, &mut tight);
        assert!(view.source_of[0].truncated);
        assert_eq!(view.source_of[0].source.lines().count(), 2);
        assert!(tight.truncated);
        assert_eq!(tight.source_lines, 0);
    }

    #[test]
    fn render_marks_missing_parent_in_chain_mode() {
        let mut record = handler();
        record
            .fields
            .insert(EXTENDS_NOT_FOUND.to_string(), serde_json::Value::Bool(true));
        let view = render_view(&record, &mode(false, None, &[]), true, &mut budget());
        assert_eq!(view.extends_found, Some(false));
        assert!(view.script.is_none());
    }

    #[test]
    fn chain_references_exclude_chain_members() {
        let records = [handler()];
        let views: Vec<IncludeView> = records
            .iter()
            .map(|record| render_view(record, &mode(false, None, &[]), true, &mut budget()))
            .collect();
        // `ChangeRequestStateHandler.prototype` names the class itself, and
        // `Object` is built in: nothing is left to suggest.
        assert!(chain_references(&records, &views).is_empty());
    }

    #[test]
    fn invalid_grep_is_a_validation_error() {
        let error = compile_grep("(unclosed").unwrap_err();
        assert!(error.downcast_ref::<ValidationError>().is_some());
    }
}
