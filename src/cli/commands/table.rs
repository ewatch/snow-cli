use std::io::IsTerminal;

use crate::cli::args::{CheckFields, DomainReadArgs, OutputFormat, TableArgs, TableCommands};
use crate::cli::commands::domain;
use crate::cli::io::{DEFAULT_MAX_STDIN_BYTES, read_to_string_limited};
use crate::cli::output;
use crate::cli::truncation;
use crate::client::{AggregateResult, SnowClient, TableReadOptions, pagination::PaginationConfig};
use crate::models::display_value::{self, DisplayValue};
use crate::models::identifiers::{RecordKey, SysId, TableName};
use crate::models::record::{Record, SingleRecordResponse};
use crate::models::validation::ValidationError;
use field_check::{CheckInputs, FieldChecks};

mod discovery;
mod field_check;
mod having;
mod lookup;

pub async fn handle(
    args: TableArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    match args.command {
        TableCommands::List(crate::cli::args::TableListArgs {
            table,
            query,
            domain,
            fields,
            limit,
            offset,
            all,
            order_by,
            full: _,
            preview,
            domain_read,
            read,
        }) => {
            tracing::info!("Listing records from table: {}", table);
            output::ensure_preview_supported(preview, format)?;

            let effective_limit = if all {
                None
            } else {
                Some(limit.unwrap_or(DEFAULT_LIST_LIMIT))
            };
            let effective_fields = sysparm_fields(
                fields.as_deref(),
                resolve_list_fields(table.as_str(), fields.as_deref()),
                domain.is_some(),
                domain_read,
            );
            let check_mode = CheckFields::resolve(read.check_fields);
            let checks = FieldChecks::collect(
                check_mode,
                CheckInputs {
                    fields: fields.as_deref(),
                    query: query.as_deref(),
                    order_by: order_by.as_ref(),
                    ..CheckInputs::default()
                },
            );

            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
            if check_mode == CheckFields::Error
                && let Some(warnings) = checks
                    .probe(&mut client, &table, domain_read.query_no_domain)
                    .await
                && !warnings.is_empty()
            {
                return Err(field_check::into_error(&table, &warnings));
            }
            let all_fields = effective_fields.is_none();
            let (projection, injected) = if check_mode == CheckFields::Warn {
                checks.augment_projection(effective_fields)
            } else {
                (effective_fields, Vec::new())
            };

            let query =
                domain::constrain_query(&mut client, &table, domain.as_ref(), query).await?;
            domain::warn_if_query_no_domain_ignored(&mut client, domain_read.query_no_domain).await;
            let pagination = PaginationConfig::default()
                .with_limit(effective_limit)
                .with_offset(offset);
            let options = TableReadOptions {
                query_no_domain: domain_read.query_no_domain,
                display_value: read.display_value,
            };

            if all
                && !preview
                && !domain_read.with_domain
                && read.display_value != Some(DisplayValue::Both)
                && matches!(format, OutputFormat::Jsonl | OutputFormat::Csv)
            {
                if matches!(format, OutputFormat::Csv) && !all_fields {
                    let stdout = std::io::stdout();
                    let mut stream =
                        output::CsvRecordStream::new(std::io::BufWriter::new(stdout.lock()));
                    let mut warnings = None;
                    let summary = client
                        .for_each_table_records_with_options(
                            &table,
                            query.as_deref(),
                            projection.as_deref(),
                            &pagination,
                            order_by.as_ref(),
                            &options,
                            |mut page| {
                                if check_mode == CheckFields::Warn
                                    && !checks.is_empty()
                                    && warnings.is_none()
                                {
                                    warnings = Some(checks.missing(&table, &page, false));
                                }
                                field_check::strip_injected(&mut page, &injected);
                                stream.write_page(&page)
                            },
                        )
                        .await?;
                    stream.finish()?;
                    let warnings = match warnings {
                        Some(warnings) => warnings,
                        None if check_mode == CheckFields::Warn
                            && summary.returned == 0
                            && checks.needs_probe_on_empty() =>
                        {
                            checks
                                .probe(&mut client, &table, domain_read.query_no_domain)
                                .await
                                .unwrap_or_default()
                        }
                        None => Vec::new(),
                    };
                    field_check::emit_stderr(&warnings);
                    return Ok(());
                }

                if matches!(format, OutputFormat::Jsonl) {
                    let mut jsonl_buf = Vec::new();
                    let mut warnings = None;
                    let mut first_record_sample: Option<Record> = None;
                    let summary = client
                        .for_each_table_records_with_options(
                            &table,
                            query.as_deref(),
                            projection.as_deref(),
                            &pagination,
                            order_by.as_ref(),
                            &options,
                            |mut page| {
                                if check_mode == CheckFields::Warn && !checks.is_empty() {
                                    if !all_fields && warnings.is_none() {
                                        warnings = Some(checks.missing(&table, &page, false));
                                    } else if all_fields
                                        && first_record_sample.is_none()
                                        && let Some(first) = page.first()
                                    {
                                        first_record_sample = Some(first.clone());
                                    }
                                }
                                field_check::strip_injected(&mut page, &injected);
                                output::write_jsonl_slice(&page, &mut jsonl_buf)
                            },
                        )
                        .await?;
                    let warnings = if let Some(warnings) = warnings {
                        warnings
                    } else if check_mode == CheckFields::Warn && !checks.is_empty() {
                        let sample = first_record_sample.as_slice();
                        checks
                            .check_records(
                                &mut client,
                                &table,
                                sample,
                                all_fields,
                                domain_read.query_no_domain,
                            )
                            .await
                    } else {
                        Vec::new()
                    };
                    field_check::emit_stderr(&warnings);
                    let stdout = std::io::stdout();
                    let mut writer = std::io::BufWriter::new(stdout.lock());
                    output::write_table_list_jsonl_meta(summary, &warnings, &mut writer)?;
                    std::io::Write::write_all(&mut writer, &jsonl_buf)?;
                    std::io::Write::flush(&mut writer)?;
                    return Ok(());
                }
            }

            let mut result = client
                .get_table_records_with_options(
                    &table,
                    query.as_deref(),
                    projection.as_deref(),
                    &pagination,
                    order_by.as_ref(),
                    &options,
                )
                .await?;
            let warnings = if check_mode == CheckFields::Warn {
                checks
                    .check_records(
                        &mut client,
                        &table,
                        &result.records,
                        all_fields,
                        domain_read.query_no_domain,
                    )
                    .await
            } else {
                Vec::new()
            };
            field_check::strip_injected(&mut result.records, &injected);
            apply_display_mode(&mut result.records, read.display_value, format);
            let domains_in_page = if domain_read.with_domain {
                Some(
                    domain::annotate_record_domains(
                        &mut client,
                        &mut result.records,
                        domain_read.query_no_domain,
                    )
                    .await,
                )
            } else {
                None
            };

            let preview_report = if preview {
                truncation::preview_record_fields(
                    &mut result.records,
                    truncation::DEFAULT_FIELD_CHAR_LIMIT,
                    truncation::RecordPreviewRoot::Records,
                    "rerun this command with --full",
                )
            } else {
                truncation::PreviewReport::default()
            };

            field_check::emit_stderr(&warnings);
            output::print_table_list_preview(
                &result,
                format,
                &preview_report,
                &output::TableListExtras {
                    domains_in_page: domains_in_page.as_ref(),
                    warnings: &warnings,
                },
            )?;
            Ok(())
        }

        TableCommands::Get(crate::cli::args::TableGetArgs {
            table,
            record,
            fields,
            lookup,
            full: _,
            preview,
            domain_read,
            read,
        }) => {
            tracing::info!("Getting record {} from table: {}", record, table);
            output::ensure_preview_supported(preview, format)?;
            let raw_field = if lookup.raw {
                Some(single_raw_field(fields.as_deref())?)
            } else {
                None
            };
            let check_mode = CheckFields::resolve(read.check_fields);

            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

            domain::warn_if_query_no_domain_ignored(&mut client, domain_read.query_no_domain).await;
            // Without --fields every field is returned, including the domain ones.
            let projection = sysparm_fields(
                fields.as_deref(),
                resolve_get_fields(fields.as_deref()),
                false,
                domain_read,
            );
            let options = TableReadOptions {
                query_no_domain: domain_read.query_no_domain,
                display_value: read.display_value,
            };
            let mut response = match &record {
                RecordKey::SysId(sys_id) => {
                    get_record_by_sys_id(
                        &mut client,
                        &table,
                        sys_id,
                        projection.as_deref(),
                        &options,
                    )
                    .await?
                }
                RecordKey::Name(name) => {
                    lookup::get_record_by_key(
                        &mut client,
                        &table,
                        name,
                        lookup.key_field.as_ref(),
                        projection.as_deref(),
                        &options,
                    )
                    .await?
                }
            };

            let checks = FieldChecks::collect(
                check_mode,
                CheckInputs {
                    fields: fields.as_deref(),
                    ..CheckInputs::default()
                },
            );
            let warnings = checks.missing(&table, std::slice::from_ref(&response), false);
            if !warnings.is_empty() && check_mode == CheckFields::Error {
                return Err(field_check::into_error(&table, &warnings));
            }
            field_check::emit_stderr(&warnings);

            if let Some(field) = raw_field {
                if let Some(DisplayValue::Both) = read.display_value {
                    display_value::compact_both(&mut response);
                }
                let value = response.fields.get(field).ok_or_else(|| {
                    ValidationError::new(format!(
                        "{table} returned no field '{field}' (unknown or not readable); --raw prints exactly one returned field"
                    ))
                })?;
                return output::print_raw_value(value);
            }

            apply_display_mode(
                std::slice::from_mut(&mut response),
                read.display_value,
                format,
            );
            if domain_read.with_domain {
                domain::annotate_record_domains(
                    &mut client,
                    std::slice::from_mut(&mut response),
                    domain_read.query_no_domain,
                )
                .await;
            }

            let preview_report = if preview {
                truncation::preview_record_fields(
                    std::slice::from_mut(&mut response),
                    truncation::DEFAULT_FIELD_CHAR_LIMIT,
                    truncation::RecordPreviewRoot::Record,
                    "rerun this command with --full",
                )
            } else {
                truncation::PreviewReport::default()
            };

            output::print_record_preview(&response, format, preview, &preview_report)?;
            Ok(())
        }

        TableCommands::Create { table, data } => {
            tracing::info!("Creating record in table: {}", table);

            let body = read_data(data, "table create <table>")?;
            let _: serde_json::Value = serde_json::from_str(&body).map_err(|_| {
                ValidationError::new("Invalid JSON data; provide a valid JSON record.")
            })?;

            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

            let path = format!("/api/now/table/{table}");
            let response: SingleRecordResponse = client.post_json(&path, &body).await?;

            output::print_record(&response.result, format)?;
            Ok(())
        }

        TableCommands::Update {
            table,
            sys_id,
            data,
        } => {
            tracing::info!("Updating record {} in table: {}", sys_id, table);

            let body = read_data(data, "table update <table> <sys_id>")?;
            let _: serde_json::Value = serde_json::from_str(&body).map_err(|_| {
                ValidationError::new("Invalid JSON data; provide a valid JSON record.")
            })?;

            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

            let path = format!("/api/now/table/{table}/{sys_id}");
            let response: SingleRecordResponse = client.patch_json(&path, &body).await?;

            output::print_record(&response.result, format)?;
            Ok(())
        }

        TableCommands::Delete { table, sys_id, yes } => {
            tracing::info!("Deleting record {} from table: {}", sys_id, table);

            if !yes {
                let stdin = std::io::stdin();
                if !stdin.is_terminal() {
                    return Err(ValidationError::new(
                        "Delete requires confirmation. Re-run with --yes for non-interactive use.",
                    )
                    .into());
                }

                eprint!("Delete record {sys_id} from table {table}? [y/N] ");
                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer)?;
                if !answer.trim().eq_ignore_ascii_case("y") {
                    eprintln!("Aborted.");
                    return Ok(());
                }
            }

            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

            let path = format!("/api/now/table/{table}/{sys_id}");
            client.delete(&path).await?;

            output::print_status(&format!("Record {sys_id} deleted from {table}"), format)?;
            Ok(())
        }

        TableCommands::Schema(crate::cli::args::TableSchemaArgs {
            table,
            extended,
            own_only,
            include_inherited: _,
        }) => {
            handle_schema(
                profile,
                format,
                instance,
                timeout_secs,
                &table,
                extended,
                own_only,
            )
            .await
        }

        TableCommands::Find(crate::cli::args::TableFindArgs {
            text,
            limit,
            offset,
            all,
        }) => {
            tracing::info!(text = %text, "Finding tables");
            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
            let window = discovery::FindWindow { limit, offset, all };
            discovery::handle_find(&mut client, &text, window, format).await
        }

        TableCommands::Choices(crate::cli::args::TableChoicesArgs {
            table,
            field,
            language,
        }) => {
            tracing::info!(table = %table, "Listing choices");
            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
            discovery::handle_choices(&mut client, &table, field.as_ref(), &language, format).await
        }

        TableCommands::Stats(crate::cli::args::TableStatsArgs {
            table,
            query,
            domain,
            group_by,
            avg,
            min,
            max,
            sum,
            having,
            check_fields,
        }) => {
            tracing::info!("Fetching aggregate stats for table: {}", table);
            let having = having
                .as_deref()
                .map(having::to_sysparm_having)
                .transpose()?;
            let having_fields = having
                .as_deref()
                .map(having::field_names)
                .unwrap_or_default();
            let check_mode = CheckFields::resolve(check_fields);
            let checks = FieldChecks::collect(
                check_mode,
                CheckInputs {
                    query: query.as_deref(),
                    group_by: group_by.as_deref(),
                    avg: avg.as_deref(),
                    min: min.as_deref(),
                    max: max.as_deref(),
                    sum: sum.as_deref(),
                    having: &having_fields,
                    ..CheckInputs::default()
                },
            );

            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
            // The Aggregate API returns no field names, so one row read verifies them.
            // Pass query_no_domain when --domain targets records outside the session's
            // default domain (the Aggregate API itself ignores sysparm_query_no_domain,
            // so table stats exposes --domain rather than --query-no-domain).
            let warnings = checks
                .probe(&mut client, &table, domain.is_some())
                .await
                .unwrap_or_default();
            if !warnings.is_empty() && check_mode == CheckFields::Error {
                return Err(field_check::into_error(&table, &warnings));
            }

            let query =
                domain::constrain_query(&mut client, &table, domain.as_ref(), query).await?;
            let mut params: Vec<(&str, &str)> = vec![("sysparm_count", "true")];
            for (key, value) in [
                ("sysparm_query", &query),
                ("sysparm_group_by", &group_by),
                ("sysparm_avg_fields", &avg),
                ("sysparm_min_fields", &min),
                ("sysparm_max_fields", &max),
                ("sysparm_sum_fields", &sum),
                ("sysparm_having", &having),
            ] {
                if let Some(v) = value {
                    params.push((key, v.as_str()));
                }
            }

            let response = client.aggregate(&table, &params).await?;
            field_check::emit_stderr(&warnings);
            match flatten_stats_response(response) {
                StatsOutput::Single(record) => output::print_record(&record, format)?,
                StatsOutput::Grouped(records) => output::print_records(&records, format)?,
            }
            Ok(())
        }
    }
}

/// Fetch one record by sys_id (`GET /api/now/table/{table}/{sys_id}`).
async fn get_record_by_sys_id(
    client: &mut SnowClient,
    table: &TableName,
    sys_id: &SysId,
    projection: Option<&str>,
    options: &TableReadOptions,
) -> anyhow::Result<Record> {
    let path = format!("/api/now/table/{table}/{sys_id}");
    let mut params = Vec::new();
    if options.query_no_domain {
        params.push(("sysparm_query_no_domain", "true"));
    }
    if let Some(fields) = projection {
        params.push(("sysparm_fields", fields));
    }
    params.extend(crate::client::display_value_params(options.display_value));

    let response: SingleRecordResponse = if params.is_empty() {
        client.get_json(&path).await?
    } else {
        client.get_json_with_params(&path, &params).await?
    };
    Ok(response.result)
}

/// Shape `--display-value both` output: compact `{value, display}` pairs,
/// split uniformly into `<field>.display` columns for tabular formats (CSV,
/// TOON, and Auto where TOON competes on tabular layout).
fn apply_display_mode(records: &mut [Record], mode: Option<DisplayValue>, format: &OutputFormat) {
    if mode != Some(DisplayValue::Both) {
        return;
    }
    for record in records.iter_mut() {
        display_value::compact_both(record);
    }
    if matches!(
        format,
        OutputFormat::Csv | OutputFormat::Toon | OutputFormat::Auto
    ) {
        display_value::split_both_columns_uniform(records);
    }
}

/// The one field `--raw` prints; `--fields` must name exactly one field.
fn single_raw_field(fields: Option<&str>) -> anyhow::Result<&str> {
    let names: Vec<&str> = fields
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    match names.as_slice() {
        [name] if *name != "*" => Ok(name),
        _ => Err(ValidationError::new(
            "--raw prints one field; pass exactly one name in --fields (e.g. --fields script)",
        )
        .into()),
    }
}

/// Bounded default record count when `--limit` and `--all` are both omitted.
pub(crate) const DEFAULT_LIST_LIMIT: usize = 20;

/// Compact default field projection for `table list` when `--fields` is omitted.
///
/// ServiceNow silently drops field names a table does not have, so the
/// fallback set for unknown tables is safe: it only narrows the response
/// to whichever of these common identifying fields exist.
fn default_list_fields(table: &str) -> &'static str {
    match table {
        "task" | "incident" | "problem" | "change_request" | "change_task" | "sc_task"
        | "sc_req_item" | "sc_request" => {
            "sys_id,number,short_description,state,priority,assigned_to,sys_updated_on"
        }
        "sys_user" => "sys_id,user_name,name,email,active,sys_updated_on",
        "sys_user_group" => "sys_id,name,description,manager,active",
        "kb_knowledge" => "sys_id,number,short_description,workflow_state,sys_updated_on",
        t if t == "cmdb_ci" || t.starts_with("cmdb_ci_") => {
            "sys_id,name,sys_class_name,operational_status,sys_updated_on"
        }
        _ => "sys_id,number,name,short_description,state,sys_updated_on",
    }
}

/// Append a single field to a comma-separated projection if it is not already
/// present, avoiding a leading comma when `projection` is empty.
fn push_projection_field(projection: &mut String, field: &str) {
    if !projection
        .split(',')
        .any(|existing| existing.trim() == field)
    {
        if !projection.is_empty() {
            projection.push(',');
        }
        projection.push_str(field);
    }
}

/// Append the domain fields `--with-domain` reports, skipping ones already
/// requested.
fn add_domain_fields(projection: &mut String) {
    for field in ["sys_domain", "sys_domain_path"] {
        push_projection_field(projection, field);
    }
}

/// Normalized form of a caller-supplied `--fields` value.
#[derive(Debug, PartialEq, Eq)]
enum NormalizedFields {
    /// `*` (with optional surrounding whitespace) requests all fields.
    All,
    /// Blank or comma/whitespace-only input specifies no explicit fields.
    Empty,
    /// One or more trimmed, non-empty field names joined with commas.
    Explicit(String),
}

fn normalize_fields(raw: &str) -> NormalizedFields {
    let mut names = Vec::new();
    for segment in raw.split(',').map(str::trim) {
        if segment == "*" {
            return NormalizedFields::All;
        }
        if !segment.is_empty() {
            names.push(segment);
        }
    }
    if names.is_empty() {
        NormalizedFields::Empty
    } else {
        NormalizedFields::Explicit(names.join(","))
    }
}

/// The `sysparm_fields` a record read sends, given the caller's `--fields`,
/// the projection resolved without domain flags (`None` requests every
/// field), whether a `--domain` filter applies, and the domain read flags.
///
/// `--with-domain` appends `sys_domain` and `sys_domain_path` unless already
/// requested. Otherwise a domain-sensitive read on the default projection
/// (`--domain` or `--query-no-domain`) appends `sys_domain` so each record
/// shows where it lives; explicit `--fields` stay authoritative.
fn sysparm_fields(
    user_fields: Option<&str>,
    resolved: Option<String>,
    domain_filter: bool,
    domain_read: DomainReadArgs,
) -> Option<String> {
    let mut projection = resolved?;
    let has_explicit_user_fields = matches!(
        user_fields.map(normalize_fields),
        Some(NormalizedFields::Explicit(_))
    );
    if domain_read.with_domain {
        add_domain_fields(&mut projection);
    } else if !has_explicit_user_fields && (domain_filter || domain_read.query_no_domain) {
        push_projection_field(&mut projection, "sys_domain");
    }
    Some(projection)
}

/// Resolve the effective `sysparm_fields` for `table list`.
///
/// Caller-supplied `--fields` is authoritative; `"*"` (trimmed) requests every
/// field (no projection); omission or blank input falls back to the compact
/// default set.
fn resolve_list_fields(table: &str, fields: Option<&str>) -> Option<String> {
    match fields.map(normalize_fields) {
        Some(NormalizedFields::All) => None,
        Some(NormalizedFields::Explicit(f)) => Some(f),
        Some(NormalizedFields::Empty) | None => Some(default_list_fields(table).to_string()),
    }
}

/// Resolve the effective `sysparm_fields` for `table get`.
///
/// `"*"` or blank input omits `sysparm_fields` (returning all fields); explicit
/// comma-separated names are trimmed and stripped of empty segments.
fn resolve_get_fields(fields: Option<&str>) -> Option<String> {
    match fields.map(normalize_fields) {
        Some(NormalizedFields::Explicit(f)) => Some(f),
        Some(NormalizedFields::All | NormalizedFields::Empty) | None => None,
    }
}

/// Flattened Aggregate API output: a single stats row for ungrouped queries,
/// or one row per group for `--group-by` queries.
enum StatsOutput {
    Single(crate::models::record::Record),
    Grouped(Vec<crate::models::record::Record>),
}

/// Flatten an Aggregate API result (`GET /api/now/stats/{table}`).
///
/// Each entry is flattened into a single-level row:
/// groupby field values keep their field names, `stats.count` becomes `count`,
/// and per-field aggregates become `avg_<field>`, `min_<field>`, `max_<field>`,
/// and `sum_<field>`. Numeric strings are converted to JSON numbers where they
/// parse cleanly.
fn flatten_stats_response(result: AggregateResult) -> StatsOutput {
    match result {
        AggregateResult::Single(entry) => StatsOutput::Single(flatten_stats_entry(&entry)),
        AggregateResult::Grouped(entries) => {
            StatsOutput::Grouped(entries.iter().map(flatten_stats_entry).collect())
        }
    }
}

/// Flatten one Aggregate API result entry into a flat row.
fn flatten_stats_entry(entry: &serde_json::Value) -> crate::models::record::Record {
    let mut fields = std::collections::BTreeMap::new();

    // Groupby field values keep their raw string values (they are categorical,
    // e.g. state "1"), so they are inserted as-is.
    if let Some(groupby) = entry.get("groupby_fields").and_then(|v| v.as_array()) {
        for pair in groupby {
            if let (Some(field), Some(value)) = (
                pair.get("field").and_then(|v| v.as_str()),
                pair.get("value"),
            ) {
                fields.insert(field.to_string(), value.clone());
            }
        }
    }

    if let Some(stats) = entry.get("stats").and_then(|v| v.as_object()) {
        for (stat, value) in stats {
            match value {
                serde_json::Value::Object(per_field) => {
                    for (field, field_value) in per_field {
                        fields.insert(format!("{stat}_{field}"), numify(field_value));
                    }
                }
                other => {
                    fields.insert(stat.clone(), numify(other));
                }
            }
        }
    }

    crate::models::record::Record { fields }
}

/// Convert a JSON string that parses cleanly as a number into a JSON number.
///
/// Non-string and non-numeric values are returned unchanged.
fn numify(value: &serde_json::Value) -> serde_json::Value {
    if let serde_json::Value::String(text) = value {
        let trimmed = text.trim();
        if let Ok(int) = trimmed.parse::<i64>() {
            return serde_json::Value::from(int);
        }
        if let Ok(float) = trimmed.parse::<f64>()
            && float.is_finite()
        {
            return serde_json::Value::from(float);
        }
    }
    value.clone()
}

/// `table schema` — Fetch column metadata from sys_dictionary.
///
/// Queries sys_dictionary for the given table to retrieve column names, types,
/// and labels. With `--extended`, also shows required, read-only, max_length,
/// default_value, and reference table. By default the effective schema is
/// returned, including columns inherited from parent tables (e.g. `incident`
/// extends `task`), each tagged with its defining table; `--own-only` limits
/// the output to columns defined on the table itself.
async fn handle_schema(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    table: &TableName,
    extended: bool,
    own_only: bool,
) -> anyhow::Result<()> {
    tracing::info!("Fetching schema for table: {}", table);

    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;

    let tables = if own_only {
        vec![table.clone()]
    } else {
        crate::cli::commands::dictionary::fetch_table_hierarchy(&mut client, table).await?
    };

    // Select fields based on compact vs extended mode
    let fields = if extended {
        "element,internal_type,column_label,mandatory,read_only,display,max_length,default_value,reference,name"
    } else {
        "element,internal_type,column_label,name"
    };

    let records =
        crate::cli::commands::dictionary::fetch_dictionary_columns(&mut client, &tables, fields)
            .await?;

    if records.is_empty() {
        output::print_status(
            &format!("No columns found for table '{table}'. Verify the table name."),
            format,
        )?;
        return Ok(());
    }

    // Build schema entries from the dictionary records
    let entries: Vec<SchemaEntry> = records
        .iter()
        .map(|r| SchemaEntry {
            column: r.get_str("element").unwrap_or("").to_string(),
            r#type: field_value_as_text(r, "internal_type").unwrap_or_default(),
            label: r.get_str("column_label").unwrap_or("").to_string(),
            table: if own_only {
                None
            } else {
                Some(r.get_str("name").unwrap_or("").to_string())
            },
            required: if extended {
                Some(r.get_str("mandatory").unwrap_or("false") == "true")
            } else {
                None
            },
            read_only: if extended {
                Some(r.get_str("read_only").unwrap_or("false") == "true")
            } else {
                None
            },
            display: if extended {
                Some(r.get_str("display").unwrap_or("false") == "true")
            } else {
                None
            },
            max_length: if extended {
                r.get_str("max_length").map(|s| s.to_string())
            } else {
                None
            },
            default_value: if extended {
                r.get_str("default_value").map(|s| s.to_string())
            } else {
                None
            },
            reference: if extended {
                let val = field_value_as_text(r, "reference").unwrap_or_default();
                if val.is_empty() { None } else { Some(val) }
            } else {
                None
            },
        })
        .collect();

    print_schema(&entries, extended, format)?;
    Ok(())
}

/// A single column's schema metadata.
#[derive(Debug, serde::Serialize)]
struct SchemaEntry {
    column: String,
    r#type: String,
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    table: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    read_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_length: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
}

/// Print schema entries in the requested format.
///
/// For CSV output, builds a fixed column set (including all extended columns
/// whenever `--extended` is requested, and `table` whenever any entry carries
/// an inherited table name) so every row aligns cleanly even when optional
/// fields such as `reference` or `default_value` are absent on some rows.
fn print_schema(
    entries: &[SchemaEntry],
    extended: bool,
    format: &OutputFormat,
) -> anyhow::Result<()> {
    if !matches!(format, OutputFormat::Csv) {
        return output::print_list(entries, format);
    }

    let include_table = entries.iter().any(|entry| entry.table.is_some());
    let rows: Vec<indexmap::IndexMap<String, serde_json::Value>> = entries
        .iter()
        .map(|entry| {
            let mut row = indexmap::IndexMap::new();
            row.insert("column".to_string(), serde_json::json!(entry.column));
            row.insert("type".to_string(), serde_json::json!(entry.r#type));
            row.insert("label".to_string(), serde_json::json!(entry.label));
            if include_table {
                row.insert("table".to_string(), serde_json::json!(entry.table));
            }
            if extended {
                row.insert("required".to_string(), serde_json::json!(entry.required));
                row.insert("read_only".to_string(), serde_json::json!(entry.read_only));
                row.insert("display".to_string(), serde_json::json!(entry.display));
                row.insert(
                    "max_length".to_string(),
                    serde_json::json!(entry.max_length),
                );
                row.insert(
                    "default_value".to_string(),
                    serde_json::json!(entry.default_value),
                );
                row.insert("reference".to_string(), serde_json::json!(entry.reference));
            }
            row
        })
        .collect();
    output::write_ordered_objects_csv(&rows, &mut std::io::stdout())
}

fn field_value_as_text(record: &crate::models::record::Record, field: &str) -> Option<String> {
    match record.fields.get(field) {
        Some(serde_json::Value::String(text)) => Some(text.clone()),
        Some(serde_json::Value::Object(map)) => map
            .get("value")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned),
        _ => None,
    }
}

/// Read JSON data from `--data` flag or stdin.
///
/// If `data` is `Some`, use it directly.
/// If `None`, read from stdin (but only if stdin is not a TTY,
/// to avoid hanging waiting for interactive input).
fn read_data(data: Option<String>, command_hint: &str) -> anyhow::Result<String> {
    let stdin = std::io::stdin();
    read_data_from(data, stdin.lock(), stdin.is_terminal(), command_hint)
}

/// Internal implementation that accepts a generic reader and TTY flag.
///
/// This enables testing without relying on actual stdin behavior.
fn read_data_from<R: std::io::Read>(
    data: Option<String>,
    reader: R,
    is_tty: bool,
    command_hint: &str,
) -> anyhow::Result<String> {
    if let Some(d) = data {
        return Ok(d);
    }

    if is_tty {
        return Err(ValidationError::new(format!(
            "No data provided. Use --data or pipe JSON to snow-cli {command_hint}."
        ))
        .into());
    }

    let buf = read_to_string_limited(reader, DEFAULT_MAX_STDIN_BYTES, "JSON stdin input")?;

    if buf.trim().is_empty() {
        return Err(ValidationError::new(format!(
            "No data received from stdin. Pipe valid JSON to snow-cli {command_hint}."
        ))
        .into());
    }

    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_read_data_from_flag() {
        let data = read_data_from(
            Some(r#"{"key":"value"}"#.to_string()),
            Cursor::new(b""),
            false,
            "table create <table>",
        )
        .unwrap();
        assert_eq!(data, r#"{"key":"value"}"#);
    }

    #[test]
    fn test_read_data_from_flag_ignores_stdin() {
        // Even when stdin has data, --data flag takes precedence
        let data = read_data_from(
            Some(r#"{"flag":"data"}"#.to_string()),
            Cursor::new(b"stdin data"),
            false,
            "table create <table>",
        )
        .unwrap();
        assert_eq!(data, r#"{"flag":"data"}"#);
    }

    #[test]
    fn test_read_data_from_stdin_pipe() {
        let input = r#"{"piped":"data"}"#;
        let data = read_data_from(
            None,
            Cursor::new(input.as_bytes()),
            false,
            "table create <table>",
        )
        .unwrap();
        assert_eq!(data, r#"{"piped":"data"}"#);
    }

    #[test]
    fn test_read_data_from_tty_no_data_errors() {
        let result = read_data_from(
            None,
            Cursor::new(b""),
            true,
            "table update <table> <sys_id>",
        );
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("No data provided"));
        assert!(msg.contains("table update <table> <sys_id>"));
    }

    #[test]
    fn test_read_data_from_empty_stdin_errors() {
        let result = read_data_from(None, Cursor::new(b""), false, "table create <table>");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No data received from stdin")
        );
    }

    #[test]
    fn test_read_data_from_whitespace_stdin_errors() {
        let result = read_data_from(
            None,
            Cursor::new(b"   \n  \t  "),
            false,
            "table create <table>",
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No data received from stdin")
        );
    }

    #[test]
    fn test_read_data_from_stdin_multiline() {
        let input = "{\n  \"key\": \"value\"\n}";
        let data = read_data_from(
            None,
            Cursor::new(input.as_bytes()),
            false,
            "table create <table>",
        )
        .unwrap();
        assert_eq!(data, input);
    }

    #[test]
    fn test_sysparm_fields_domain_projection_rules() {
        let flags = |query_no_domain, with_domain| DomainReadArgs {
            query_no_domain,
            with_domain,
        };
        let list = |fields: Option<&str>, domain_filter, read| {
            sysparm_fields(
                fields,
                resolve_list_fields("sys_user", fields),
                domain_filter,
                read,
            )
        };
        let default = default_list_fields("sys_user");

        assert_eq!(list(None, false, flags(false, false)).unwrap(), default);
        // --with-domain deduplicates against what is already requested.
        assert_eq!(
            list(None, false, flags(false, true)).unwrap(),
            format!("{default},sys_domain,sys_domain_path")
        );
        assert_eq!(
            list(Some("sys_id,sys_domain"), false, flags(true, true)).as_deref(),
            Some("sys_id,sys_domain,sys_domain_path")
        );
        // Domain-sensitive reads add sys_domain to the default projection only.
        assert_eq!(
            list(None, true, flags(false, false)).unwrap(),
            format!("{default},sys_domain")
        );
        assert_eq!(
            list(None, false, flags(true, false)).unwrap(),
            format!("{default},sys_domain")
        );
        assert_eq!(
            list(Some("sys_id"), true, flags(true, false)).as_deref(),
            Some("sys_id")
        );
        // "*" sends no projection at all.
        assert_eq!(list(Some("*"), true, flags(true, true)), None);

        // table get: no projection unless --fields was given.
        assert_eq!(sysparm_fields(None, None, false, flags(true, true)), None);
        assert_eq!(
            sysparm_fields(
                Some("number"),
                Some("number".into()),
                false,
                flags(true, true)
            )
            .as_deref(),
            Some("number,sys_domain,sys_domain_path")
        );
        assert_eq!(
            sysparm_fields(
                Some("number"),
                Some("number".into()),
                false,
                flags(true, false)
            )
            .as_deref(),
            Some("number")
        );
    }

    #[test]
    fn test_resolve_list_fields_explicit_fields_are_authoritative() {
        assert_eq!(
            resolve_list_fields("incident", Some("sys_id,number")),
            Some("sys_id,number".to_string())
        );
    }

    #[test]
    fn test_resolve_list_fields_star_requests_all_fields() {
        assert_eq!(resolve_list_fields("incident", Some("*")), None);
        assert_eq!(resolve_list_fields("incident", Some(" * ")), None);
        assert_eq!(resolve_list_fields("incident", Some("number, *")), None);
    }

    #[test]
    fn test_resolve_fields_trims_and_skips_empty_segments() {
        assert_eq!(
            resolve_list_fields("incident", Some(" sys_id , , number , ")),
            Some("sys_id,number".to_string())
        );
        assert_eq!(
            resolve_list_fields("incident", Some("")),
            Some(default_list_fields("incident").to_string())
        );
        assert_eq!(
            resolve_list_fields("incident", Some(" , , ")),
            Some(default_list_fields("incident").to_string())
        );
        assert_eq!(resolve_get_fields(Some(" * ")), None);
        assert_eq!(resolve_get_fields(Some("")), None);
        assert_eq!(
            resolve_get_fields(Some(" number , , state ")),
            Some("number,state".to_string())
        );

        let mut empty = String::new();
        add_domain_fields(&mut empty);
        assert_eq!(empty, "sys_domain,sys_domain_path");
    }

    #[test]
    fn test_resolve_list_fields_defaults_to_compact_projection() {
        let fields = resolve_list_fields("incident", None).unwrap();
        assert!(fields.contains("sys_id"));
        assert!(fields.contains("number"));
        assert!(fields.contains("short_description"));
        // Compact: a handful of columns, not the whole record
        assert!(fields.split(',').count() <= 8);
    }

    #[test]
    fn test_default_list_fields_table_aware() {
        assert!(default_list_fields("sys_user").contains("user_name"));
        assert!(default_list_fields("cmdb_ci_server").contains("sys_class_name"));
        assert!(default_list_fields("change_request").contains("number"));
        // Unknown tables get the conservative fallback
        assert!(default_list_fields("x_custom_table").contains("sys_id"));
        assert!(default_list_fields("x_custom_table").contains("sys_updated_on"));
    }

    #[test]
    fn test_flatten_stats_response_ungrouped_count() {
        let response = serde_json::json!({
            "result": {
                "stats": {"count": "4381"}
            }
        });

        match flatten_stats_response(AggregateResult::from_envelope(response).unwrap()) {
            StatsOutput::Single(record) => {
                assert_eq!(
                    serde_json::to_value(&record).unwrap(),
                    serde_json::json!({"count": 4381})
                );
            }
            StatsOutput::Grouped(_) => panic!("Expected single stats row"),
        }
    }

    #[test]
    fn test_flatten_stats_response_grouped_with_aggregates() {
        let response = serde_json::json!({
            "result": [
                {
                    "stats": {
                        "count": "8",
                        "avg": {"priority": "2.5"},
                        "max": {"priority": "4"}
                    },
                    "groupby_fields": [{"field": "state", "value": "1"}]
                },
                {
                    "stats": {
                        "count": "3",
                        "avg": {"priority": "1.5"},
                        "max": {"priority": "2"}
                    },
                    "groupby_fields": [{"field": "state", "value": "2"}]
                }
            ]
        });

        match flatten_stats_response(AggregateResult::from_envelope(response).unwrap()) {
            StatsOutput::Grouped(records) => {
                assert_eq!(records.len(), 2);
                assert_eq!(
                    serde_json::to_value(&records[0]).unwrap(),
                    serde_json::json!({
                        "state": "1",
                        "count": 8,
                        "avg_priority": 2.5,
                        "max_priority": 4
                    })
                );
                assert_eq!(
                    serde_json::to_value(&records[1]).unwrap(),
                    serde_json::json!({
                        "state": "2",
                        "count": 3,
                        "avg_priority": 1.5,
                        "max_priority": 2
                    })
                );
            }
            StatsOutput::Single(_) => panic!("Expected grouped stats rows"),
        }
    }

    #[test]
    fn test_flatten_stats_response_multiple_groupby_fields() {
        let response = serde_json::json!({
            "result": [
                {
                    "stats": {"count": "5"},
                    "groupby_fields": [
                        {"field": "state", "value": "1"},
                        {"field": "priority", "value": "3"}
                    ]
                }
            ]
        });

        match flatten_stats_response(AggregateResult::from_envelope(response).unwrap()) {
            StatsOutput::Grouped(records) => {
                assert_eq!(
                    serde_json::to_value(&records[0]).unwrap(),
                    serde_json::json!({"state": "1", "priority": "3", "count": 5})
                );
            }
            StatsOutput::Single(_) => panic!("Expected grouped stats rows"),
        }
    }

    #[test]
    fn test_numify_converts_clean_numeric_strings_only() {
        assert_eq!(numify(&serde_json::json!("8")), serde_json::json!(8));
        assert_eq!(numify(&serde_json::json!("2.5")), serde_json::json!(2.5));
        assert_eq!(numify(&serde_json::json!("-4")), serde_json::json!(-4));
        // Non-numeric, empty, and non-finite strings are left untouched.
        assert_eq!(
            numify(&serde_json::json!("high")),
            serde_json::json!("high")
        );
        assert_eq!(numify(&serde_json::json!("")), serde_json::json!(""));
        assert_eq!(numify(&serde_json::json!("NaN")), serde_json::json!("NaN"));
        // Non-string values pass through unchanged.
        assert_eq!(numify(&serde_json::json!(7)), serde_json::json!(7));
        assert_eq!(numify(&serde_json::Value::Null), serde_json::Value::Null);
    }

    #[test]
    fn test_field_value_as_text_supports_string_and_link_object() {
        let string_record = crate::models::record::Record {
            fields: std::collections::BTreeMap::from([(
                "internal_type".to_string(),
                serde_json::json!("string"),
            )]),
        };
        assert_eq!(
            field_value_as_text(&string_record, "internal_type"),
            Some("string".to_string())
        );

        let object_record = crate::models::record::Record {
            fields: std::collections::BTreeMap::from([(
                "internal_type".to_string(),
                serde_json::json!({
                    "link": "https://example.com/api/now/table/sys_glide_object?name=reference",
                    "value": "reference"
                }),
            )]),
        };
        assert_eq!(
            field_value_as_text(&object_record, "internal_type"),
            Some("reference".to_string())
        );
    }
}
