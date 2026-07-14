use super::*;

pub(super) async fn handle_list(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    search: Option<&EncodedQueryValue>,
    kinds: &[ScopeListKind],
    text_options: ScopeListTextOptions,
) -> anyhow::Result<()> {
    let payload = list_scopes(profile, instance, timeout_secs, search, kinds).await?;

    match format {
        OutputFormat::Json | OutputFormat::Jsonl | OutputFormat::Toon | OutputFormat::Auto => {
            output::print_output(&payload, format)
        }
        OutputFormat::Csv => output::print_list(&payload.rows, format),
        OutputFormat::Text => print_scope_list_text(&payload, text_options),
    }
}

pub(super) async fn list_scopes(
    profile: &str,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    search: Option<&EncodedQueryValue>,
    kinds: &[ScopeListKind],
) -> anyhow::Result<ScopeListOutput> {
    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
    let pagination = PaginationConfig::default();
    let scope_query = build_scope_search_query(search)?;
    let plugin_query = build_plugin_search_query(search)?;

    let sys_scope = TableName::from_static("sys_scope");
    let scopes = client
        .get_table_records(
            &sys_scope,
            scope_query.as_deref(),
            Some("sys_id,scope,name,version"),
            &pagination,
            None,
        )
        .await?;

    let mut warnings = Vec::new();
    let sys_store_app = TableName::from_static("sys_store_app");
    let store_apps = query_optional_table(
        &mut client,
        &sys_store_app,
        scope_query.as_deref(),
        "sys_id,scope,name,version,vendor",
        &mut warnings,
    )
    .await;
    let v_plugin = TableName::from_static("v_plugin");
    let plugins = query_optional_table(
        &mut client,
        &v_plugin,
        plugin_query.as_deref(),
        "sys_id,id,name,active",
        &mut warnings,
    )
    .await;

    let rows = filter_scope_list_rows(build_scope_list_rows(scopes, store_apps, plugins), kinds);
    let counts = ScopeListCounts::from_rows(&rows);

    Ok(ScopeListOutput {
        search: search.map(ToString::to_string),
        kind_filter: kinds.iter().map(|kind| kind.as_str().to_string()).collect(),
        counts,
        rows,
        warnings,
    })
}

pub(super) fn filter_scope_list_rows(
    rows: Vec<ScopeListRow>,
    kinds: &[ScopeListKind],
) -> Vec<ScopeListRow> {
    if kinds.is_empty() {
        return rows;
    }

    let allowed = kinds
        .iter()
        .map(ScopeListKind::as_str)
        .collect::<HashSet<_>>();
    rows.into_iter()
        .filter(|row| allowed.contains(row.kind.as_str()))
        .collect()
}

pub(super) async fn query_optional_table(
    client: &mut crate::client::SnowClient,
    table: &TableName,
    query: Option<&str>,
    fields: &str,
    warnings: &mut Vec<String>,
) -> Vec<Record> {
    let pagination = PaginationConfig::default();
    match client
        .get_table_records(table, query, Some(fields), &pagination, None)
        .await
    {
        Ok(records) => records,
        Err(err) => {
            warnings.push(format!("Failed to query {table}: {err}"));
            Vec::new()
        }
    }
}

pub(super) fn build_scope_search_query(
    search: Option<&EncodedQueryValue>,
) -> anyhow::Result<Option<String>> {
    Ok(search.map(|search| {
        format!("scope={search}^ORsys_id={search}^ORscopeLIKE{search}^ORnameLIKE{search}")
    }))
}

pub(super) fn build_plugin_search_query(
    search: Option<&EncodedQueryValue>,
) -> anyhow::Result<Option<String>> {
    Ok(search.map(|search| format!("id={search}^ORidLIKE{search}^ORnameLIKE{search}")))
}

pub(super) fn build_scope_list_rows(
    scopes: Vec<Record>,
    store_apps: Vec<Record>,
    plugins: Vec<Record>,
) -> Vec<ScopeListRow> {
    let store_scope_names = store_apps
        .iter()
        .map(|record| field_text(record, "scope"))
        .filter(|scope| !scope.is_empty())
        .collect::<HashSet<_>>();
    let seen_scope_names = scopes
        .iter()
        .map(|record| field_text(record, "scope"))
        .filter(|scope| !scope.is_empty())
        .collect::<HashSet<_>>();

    let mut rows = scopes
        .into_iter()
        .map(|record| {
            let scope = field_text(&record, "scope");
            let kind = classify_scope_kind(&scope, store_scope_names.contains(&scope));
            ScopeListRow {
                kind: kind.to_string(),
                scope,
                name: field_text(&record, "name"),
                version: field_text(&record, "version"),
                identifier: String::new(),
                source_table: "sys_scope".to_string(),
                sys_id: field_text(&record, "sys_id"),
            }
        })
        .collect::<Vec<_>>();

    rows.extend(
        store_apps
            .into_iter()
            .filter(|record| {
                let scope = field_text(record, "scope");
                scope.is_empty() || !seen_scope_names.contains(&scope)
            })
            .map(|record| ScopeListRow {
                kind: "store_app".to_string(),
                scope: field_text(&record, "scope"),
                name: field_text(&record, "name"),
                version: field_text(&record, "version"),
                identifier: String::new(),
                source_table: "sys_store_app".to_string(),
                sys_id: field_text(&record, "sys_id"),
            }),
    );

    rows.extend(plugins.into_iter().map(|record| ScopeListRow {
        kind: "plugin".to_string(),
        scope: String::new(),
        name: field_text(&record, "name"),
        version: String::new(),
        identifier: field_text(&record, "id"),
        source_table: "v_plugin".to_string(),
        sys_id: field_text(&record, "sys_id"),
    }));

    rows.sort_by(|left, right| {
        (
            left.kind.as_str(),
            left.scope.as_str(),
            left.name.as_str(),
            left.identifier.as_str(),
            left.sys_id.as_str(),
        )
            .cmp(&(
                right.kind.as_str(),
                right.scope.as_str(),
                right.name.as_str(),
                right.identifier.as_str(),
                right.sys_id.as_str(),
            ))
    });
    rows
}

pub(super) fn classify_scope_kind(scope: &str, is_store_app: bool) -> &'static str {
    if is_store_app {
        "store_app"
    } else if scope == "global" {
        "platform"
    } else if scope.starts_with("x_") {
        "custom_app"
    } else {
        "platform_app"
    }
}

pub(super) fn print_scope_list_text(
    payload: &ScopeListOutput,
    options: ScopeListTextOptions,
) -> anyhow::Result<()> {
    let mut out = String::new();

    if let Some(search) = &payload.search {
        writeln!(&mut out, "Search: {search}")?;
    }
    if !payload.kind_filter.is_empty() {
        writeln!(&mut out, "Kinds: {}", payload.kind_filter.join(", "))?;
    }
    writeln!(&mut out, "Total: {}", payload.counts.total)?;

    if !payload.counts.by_kind.is_empty() {
        let counts = payload
            .counts
            .by_kind
            .iter()
            .map(|(kind, count)| format!("{kind}={count}"))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(&mut out, "By kind: {counts}")?;
    }

    if payload.rows.is_empty() {
        writeln!(&mut out)?;
        writeln!(&mut out, "No matching scopes found.")?;
    } else {
        for (kind, rows) in group_scope_rows_by_kind(&payload.rows) {
            writeln!(&mut out)?;
            writeln!(&mut out, "{}", kind.to_ascii_uppercase())?;

            let name_width = rows
                .iter()
                .map(|row| row.name.len())
                .max()
                .unwrap_or(0)
                .max(4);
            let key_width = rows
                .iter()
                .map(|row| scope_row_key(row).len())
                .max()
                .unwrap_or(0)
                .max(5);
            let source_table_width = rows
                .iter()
                .map(|row| row.source_table.len())
                .max()
                .unwrap_or(0)
                .max("source_table".len());
            let sys_id_width = rows
                .iter()
                .map(|row| row.sys_id.len())
                .max()
                .unwrap_or(0)
                .max("sys_id".len());

            for row in rows {
                let key = scope_row_key(row);
                let version = if row.version.is_empty() {
                    "-"
                } else {
                    row.version.as_str()
                };
                write!(
                    &mut out,
                    "- {:name_width$}  {:key_width$}  {}",
                    row.name,
                    key,
                    version,
                    name_width = name_width,
                    key_width = key_width,
                )?;
                if options.show_source_table {
                    write!(
                        &mut out,
                        "  {:source_table_width$}",
                        row.source_table,
                        source_table_width = source_table_width,
                    )?;
                }
                if options.show_sys_id {
                    write!(
                        &mut out,
                        "  {:sys_id_width$}",
                        row.sys_id,
                        sys_id_width = sys_id_width,
                    )?;
                }
                writeln!(&mut out)?;
            }
        }
    }

    if !payload.warnings.is_empty() {
        writeln!(&mut out)?;
        writeln!(&mut out, "Warnings:")?;
        for warning in &payload.warnings {
            writeln!(&mut out, "- {warning}")?;
        }
    }

    print!("{out}");
    Ok(())
}
pub(super) fn group_scope_rows_by_kind(rows: &[ScopeListRow]) -> Vec<(&str, Vec<&ScopeListRow>)> {
    let ordered_kinds = [
        "store_app",
        "custom_app",
        "plugin",
        "platform",
        "platform_app",
    ];

    ordered_kinds
        .iter()
        .filter_map(|kind| {
            let matches = rows
                .iter()
                .filter(|row| row.kind == *kind)
                .collect::<Vec<_>>();
            if matches.is_empty() {
                None
            } else {
                Some((*kind, matches))
            }
        })
        .collect()
}

pub(super) fn scope_row_key(row: &ScopeListRow) -> &str {
    if row.scope.is_empty() {
        row.identifier.as_str()
    } else {
        row.scope.as_str()
    }
}
