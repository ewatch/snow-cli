use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;

use crate::cli::args::{OutputFormat, ScopeArgs, ScopeCommands, ScopeDetailLevel, ScopeListKind};
use crate::cli::output;
use crate::client::pagination::PaginationConfig;
use crate::models::identifiers::{EncodedQueryValue, SysId, TableName};
use crate::models::record::Record;

struct ArtifactDefinition {
    artifact_type: &'static str,
    category: &'static str,
    table: &'static str,
    fields: &'static str,
    name_field: &'static str,
}

struct MoveFileRequest<'a> {
    table: &'a TableName,
    sys_id: &'a SysId,
    target_scope: &'a str,
    dry_run: bool,
    yes: bool,
}

const ARTIFACT_DEFINITIONS: &[ArtifactDefinition] = &[
    ArtifactDefinition {
        artifact_type: "tables",
        category: "data_model_logic",
        table: "sys_db_object",
        fields: "sys_id,name,label,super_class",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "script_includes",
        category: "server_logic",
        table: "sys_script_include",
        fields: "sys_id,name,api_name,active,client_callable",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "business_rules",
        category: "server_logic",
        table: "sys_script",
        fields: "sys_id,name,collection,when,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "scheduled_scripts",
        category: "server_logic",
        table: "sysauto_script",
        fields: "sys_id,name,run_type,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "processors",
        category: "server_logic",
        table: "sys_processor",
        fields: "sys_id,name,type,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "transform_maps",
        category: "server_logic",
        table: "sys_transform_map",
        fields: "sys_id,name,target_table,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "transform_entries",
        category: "server_logic",
        table: "sys_transform_entry",
        fields: "sys_id,target_field,source_field,map",
        name_field: "target_field",
    },
    ArtifactDefinition {
        artifact_type: "transform_scripts",
        category: "server_logic",
        table: "sys_transform_script",
        fields: "sys_id,name,map,when,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "client_scripts",
        category: "client_logic",
        table: "sys_script_client",
        fields: "sys_id,name,table,type,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "ui_actions",
        category: "client_logic",
        table: "sys_ui_action",
        fields: "sys_id,name,table,action_name,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "ui_pages",
        category: "client_logic",
        table: "sys_ui_page",
        fields: "sys_id,name,category,sys_name",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "ui_policies",
        category: "client_logic",
        table: "sys_ui_policy",
        fields: "sys_id,short_description,table,active",
        name_field: "short_description",
    },
    ArtifactDefinition {
        artifact_type: "ui_policy_actions",
        category: "client_logic",
        table: "sys_ui_policy_action",
        fields: "sys_id,ui_policy,field,mandatory,visible,read_only",
        name_field: "field",
    },
    ArtifactDefinition {
        artifact_type: "flows",
        category: "flow_logic",
        table: "sys_hub_flow",
        fields: "sys_id,name,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "flow_actions",
        category: "flow_logic",
        table: "sys_hub_action_type_definition",
        fields: "sys_id,name,scope,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "flow_action_instances",
        category: "flow_logic",
        table: "sys_hub_action_instance",
        fields: "sys_id,name,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "flow_versions",
        category: "flow_logic",
        table: "sys_hub_flow_version",
        fields: "sys_id,name,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "flow_trigger_definitions",
        category: "flow_logic",
        table: "sys_hub_trigger_definition",
        fields: "sys_id,name,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "scripted_rest_apis",
        category: "integration_logic",
        table: "sys_ws_definition",
        fields: "sys_id,name,base_api_path,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "scripted_rest_operations",
        category: "integration_logic",
        table: "sys_ws_operation",
        fields: "sys_id,name,http_method,relative_path,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "rest_messages",
        category: "integration_logic",
        table: "sys_rest_message",
        fields: "sys_id,name,rest_endpoint,authentication_type",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "rest_message_functions",
        category: "integration_logic",
        table: "sys_rest_message_fn",
        fields: "sys_id,function_name,http_method,rest_endpoint",
        name_field: "function_name",
    },
    ArtifactDefinition {
        artifact_type: "acls",
        category: "security_logic",
        table: "sys_security_acl",
        fields: "sys_id,name,operation,type,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "acl_roles",
        category: "security_logic",
        table: "sys_security_acl_role",
        fields: "sys_id,sys_security_acl,sys_user_role",
        name_field: "sys_user_role",
    },
    ArtifactDefinition {
        artifact_type: "scope_privileges",
        category: "security_logic",
        table: "sys_scope_privilege",
        fields: "sys_id,target_scope,target_name,operation,status",
        name_field: "target_name",
    },
    ArtifactDefinition {
        artifact_type: "event_registrations",
        category: "event_notification_logic",
        table: "sysevent_register",
        fields: "sys_id,event_name,description,fired_by",
        name_field: "event_name",
    },
    ArtifactDefinition {
        artifact_type: "email_notifications",
        category: "event_notification_logic",
        table: "sysevent_email_action",
        fields: "sys_id,name,event_name,active",
        name_field: "name",
    },
    ArtifactDefinition {
        artifact_type: "properties",
        category: "data_model_logic",
        table: "sys_properties",
        fields: "sys_id,name,type,description",
        name_field: "name",
    },
];

pub async fn handle(
    args: ScopeArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    match args.command {
        ScopeCommands::List {
            search,
            kind,
            show_source_table,
            show_sys_id,
        } => {
            handle_list(
                profile,
                format,
                instance,
                timeout_secs,
                search.as_ref(),
                &kind,
                ScopeListTextOptions {
                    show_source_table,
                    show_sys_id,
                },
            )
            .await
        }
        ScopeCommands::Inspect { scope, details } => {
            handle_inspect(profile, format, instance, timeout_secs, &scope, details).await
        }
        ScopeCommands::Inventory { scope } => {
            handle_inventory(profile, format, instance, timeout_secs, &scope).await
        }
        ScopeCommands::MoveFile {
            table,
            sys_id,
            target_scope,
            dry_run,
            yes,
        } => {
            handle_move_file(
                profile,
                format,
                instance,
                timeout_secs,
                MoveFileRequest {
                    table: &table,
                    sys_id: &sys_id,
                    target_scope: &target_scope,
                    dry_run,
                    yes,
                },
            )
            .await
        }
    }
}

async fn handle_list(
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

async fn handle_inspect(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    scope_input: &EncodedQueryValue,
    details: ScopeDetailLevel,
) -> anyhow::Result<()> {
    let collected = collect_scope_data(profile, instance, timeout_secs, scope_input).await?;
    let rows = collected.to_inventory_rows();

    let payload = ScopeInspectOutput {
        scope: collected.scope,
        details: match details {
            ScopeDetailLevel::Basic => "basic".to_string(),
            ScopeDetailLevel::Full => "full".to_string(),
        },
        summary: collected.summary,
        artifacts: if matches!(details, ScopeDetailLevel::Full) {
            Some(rows)
        } else {
            None
        },
        warnings: collected.warnings,
    };

    match format {
        OutputFormat::Json | OutputFormat::Jsonl | OutputFormat::Toon | OutputFormat::Auto => {
            output::print_output(&payload, format)
        }
        OutputFormat::Csv => {
            let csv_rows = payload
                .summary
                .to_csv_rows(&payload.scope.scope, &payload.scope.sys_id);
            output::print_list(&csv_rows, format)
        }
        OutputFormat::Text => output::print_output(&payload, format),
    }
}

async fn handle_inventory(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    scope_input: &EncodedQueryValue,
) -> anyhow::Result<()> {
    let collected = collect_scope_data(profile, instance, timeout_secs, scope_input).await?;
    let rows = collected.to_inventory_rows();

    match format {
        OutputFormat::Json | OutputFormat::Jsonl | OutputFormat::Toon | OutputFormat::Auto => {
            let payload = ScopeInventoryOutput {
                scope: collected.scope,
                summary: collected.summary,
                rows,
                warnings: collected.warnings,
            };
            output::print_output(&payload, format)
        }
        OutputFormat::Csv => output::print_list(&rows, format),
        OutputFormat::Text => {
            let payload = ScopeInventoryOutput {
                scope: collected.scope,
                summary: collected.summary,
                rows,
                warnings: collected.warnings,
            };
            output::print_output(&payload, format)
        }
    }
}

async fn handle_move_file(
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    request: MoveFileRequest<'_>,
) -> anyhow::Result<()> {
    let script = build_move_file_script(
        request.table.as_str(),
        request.sys_id.as_str(),
        request.target_scope,
        request.dry_run,
        request.yes,
    )?;
    let response_body = crate::cli::commands::script::run_background_script(
        profile,
        instance,
        timeout_secs,
        &script,
        "global",
        None,
    )
    .await?;

    let payload: MoveFileOutput = serde_json::from_str(&response_body).map_err(|error| {
        anyhow::anyhow!(
            "scope move-file returned non-JSON output: {error}. Raw output: {response_body}"
        )
    })?;

    if !payload.ok {
        let mut message = payload
            .error
            .clone()
            .unwrap_or_else(|| "Application file move failed.".to_string());
        if !payload.warnings.is_empty() {
            message.push_str(" Warnings: ");
            message.push_str(&payload.warnings.join(" | "));
        }
        anyhow::bail!(message);
    }

    output::print_output(&payload, format)
}

fn build_move_file_script(
    table: &str,
    sys_id: &str,
    target_scope: &str,
    dry_run: bool,
    yes: bool,
) -> anyhow::Result<String> {
    let input = serde_json::json!({
        "table": table,
        "sysId": sys_id,
        "targetScope": target_scope,
        "dryRun": dry_run,
        "force": yes,
    });
    let input_json = serde_json::to_string(&input)?;

    Ok(format!(
        r#"(function() {{
var input = {input_json};
var output = {{
  ok: false,
  dry_run: !!input.dryRun,
  table: String(input.table || ''),
  sys_id: String(input.sysId || ''),
  changed_fields: [],
  warnings: [],
  requires_confirmation: false,
  before: {{}},
  after: {{}}
}};

function pushUnique(list, value) {{
  if (list.indexOf(value) === -1) {{
    list.push(value);
  }}
}}

function text(value) {{
  return value ? String(value) : '';
}}

function startsWith(value, prefix) {{
  return value.indexOf(prefix) === 0;
}}

function rewritePrefix(value, oldPrefix, newPrefix) {{
  if (!value || !startsWith(value, oldPrefix)) {{
    return value;
  }}
  return newPrefix + value.substring(oldPrefix.length);
}}

function rewritePath(value, oldScopeName, newScopeName) {{
  if (!value) {{
    return value;
  }}
  var segment = '/' + oldScopeName + '/';
  if (value.indexOf(segment) >= 0) {{
    return value.replace(segment, '/' + newScopeName + '/');
  }}
  if (startsWith(value, oldScopeName + '/')) {{
    return newScopeName + value.substring(oldScopeName.length);
  }}
  return value;
}}

function getTableDefinition(tableName) {{
  if (!tableName) {{
    return null;
  }}

  var definition = new GlideRecord('sys_db_object');
  if (!definition.get('name', tableName)) {{
    return null;
  }}

  return definition;
}}

function tableExtends(tableName, ancestorName) {{
  if (!tableName || !ancestorName) {{
    return false;
  }}

  var visited = {{}};
  var currentName = tableName;
  while (currentName && !visited[currentName]) {{
    visited[currentName] = true;
    if (currentName === ancestorName) {{
      return true;
    }}

    var definition = getTableDefinition(currentName);
    if (!definition) {{
      return false;
    }}

    var parentId = text(definition.getValue('super_class'));
    if (!parentId) {{
      return false;
    }}

    var parent = new GlideRecord('sys_db_object');
    if (!parent.get(parentId)) {{
      return false;
    }}

    currentName = text(parent.getValue('name'));
  }}

  return false;
}}

function fail(message) {{
  output.error = message;
  gs.print(JSON.stringify(output));
}}

try {{
  if (!input.table || !input.sysId || !input.targetScope) {{
    fail('table, sys_id, and --target-scope are required.');
    return;
  }}

  var target = new GlideRecord('sys_scope');
  var targetQuery = target.addQuery('scope', input.targetScope);
  targetQuery.addOrCondition('sys_id', input.targetScope);
  target.query();
  if (!target.next()) {{
    fail('Target scope not found in sys_scope: ' + input.targetScope);
    return;
  }}

  output.target_scope = {{
    sys_id: text(target.getUniqueValue()),
    scope: text(target.getValue('scope')),
    name: text(target.getValue('name')),
    version: text(target.getValue('version'))
  }};

  if (!startsWith(output.target_scope.scope, 'x_') && output.target_scope.scope !== 'global') {{
    fail('Target scope must be a custom application scope (x_*) or global.');
    return;
  }}

  var record = new GlideRecord(input.table);
  if (!record.isValid()) {{
    fail('Table is not valid: ' + input.table);
    return;
  }}
  if (!tableExtends(input.table, 'sys_metadata')) {{
    fail('Unsupported record: table must extend sys_metadata and represent an application file.');
    return;
  }}
  if (!record.get(input.sysId)) {{
    fail('Record not found: ' + input.table + ' / ' + input.sysId);
    return;
  }}
  if (!record.isValidField('sys_scope') || !record.isValidField('sys_package')) {{
    fail('Unsupported record: sys_scope/sys_package are not both available on this table.');
    return;
  }}

  var sourceScopeId = text(record.getValue('sys_scope'));
  if (!sourceScopeId) {{
    fail('Source record does not have a current sys_scope value.');
    return;
  }}

  var source = new GlideRecord('sys_scope');
  if (!source.get(sourceScopeId)) {{
    fail('Source scope could not be resolved from sys_scope: ' + sourceScopeId);
    return;
  }}

  output.source_scope = {{
    sys_id: text(source.getUniqueValue()),
    scope: text(source.getValue('scope')),
    name: text(source.getValue('name')),
    version: text(source.getValue('version'))
  }};

  if (!startsWith(output.source_scope.scope, 'x_') && output.source_scope.scope !== 'global') {{
    fail('Source scope must be a custom application scope (x_*) or global.');
    return;
  }}

  if (output.source_scope.sys_id === output.target_scope.sys_id) {{
    fail('Source and target scope are the same.');
    return;
  }}

  output.before = {{
    sys_scope: sourceScopeId,
    sys_package: text(record.getValue('sys_package')),
    sys_name: record.isValidField('sys_name') ? text(record.getValue('sys_name')) : '',
    sys_update_name: record.isValidField('sys_update_name') ? text(record.getValue('sys_update_name')) : '',
    api_name: record.isValidField('api_name') ? text(record.getValue('api_name')) : '',
    path: record.isValidField('path') ? text(record.getValue('path')) : ''
  }};

  output.after = {{
    sys_scope: output.target_scope.sys_id,
    sys_package: output.target_scope.sys_id,
    sys_name: output.before.sys_name,
    sys_update_name: output.before.sys_update_name,
    api_name: output.before.api_name,
    path: output.before.path
  }};

  if (output.before.sys_name) {{
    output.after.sys_name = rewritePrefix(
      output.before.sys_name,
      output.source_scope.scope + '_',
      output.target_scope.scope + '_'
    );
    if (output.after.sys_name === output.before.sys_name && output.before.sys_name.indexOf(output.source_scope.scope) >= 0) {{
      pushUnique(output.warnings, 'Field sys_name contains the source scope but was not safely rewritten.');
    }}
  }}

  if (output.before.sys_update_name) {{
    output.after.sys_update_name = rewritePrefix(
      output.before.sys_update_name,
      output.source_scope.scope + '_',
      output.target_scope.scope + '_'
    );
    if (output.after.sys_update_name === output.before.sys_update_name && output.before.sys_update_name.indexOf(output.source_scope.scope) >= 0) {{
      pushUnique(output.warnings, 'Field sys_update_name contains the source scope but was not safely rewritten.');
    }}
  }}

  if (output.before.api_name) {{
    output.after.api_name = rewritePrefix(
      output.before.api_name,
      output.source_scope.scope + '.',
      output.target_scope.scope + '.'
    );
    if (output.after.api_name === output.before.api_name && output.before.api_name.indexOf(output.source_scope.scope) >= 0) {{
      pushUnique(output.warnings, 'Field api_name contains the source scope but was not safely rewritten.');
    }}
  }}

  if (output.before.path) {{
    output.after.path = rewritePath(
      output.before.path,
      output.source_scope.scope,
      output.target_scope.scope
    );
    if (output.after.path === output.before.path && output.before.path.indexOf(output.source_scope.scope) >= 0) {{
      pushUnique(output.warnings, 'Field path contains the source scope but was not safely rewritten.');
    }}
  }}

  var changedFields = ['sys_scope', 'sys_package', 'sys_name', 'sys_update_name', 'api_name', 'path'];
  for (var c = 0; c < changedFields.length; c++) {{
    var fieldName = changedFields[c];
    if (text(output.before[fieldName]) !== text(output.after[fieldName])) {{
      output.changed_fields.push(fieldName);
    }}
  }}

  var elements = record.getElements();
  for (var i = 0; i < elements.size(); i++) {{
    var element = elements.get(i);
    var fieldName = text(element.getName());
    if (!fieldName || fieldName === 'sys_scope' || fieldName === 'sys_package' || fieldName === 'sys_name' || fieldName === 'sys_update_name' || fieldName === 'api_name' || fieldName === 'path') {{
      continue;
    }}

    var internalType = '';
    try {{
      internalType = text(element.getED().getInternalType());
    }} catch (ignored) {{}}

    if (internalType !== 'string' && internalType !== 'script' && internalType !== 'html' && internalType !== 'xml' && internalType !== 'translated_text' && internalType !== 'wide_text') {{
      continue;
    }}

    var fieldValue = text(record.getValue(fieldName));
    if (!fieldValue) {{
      continue;
    }}

    if (fieldValue.indexOf(output.source_scope.scope) >= 0 || fieldValue.indexOf(output.source_scope.sys_id) >= 0) {{
      pushUnique(
        output.warnings,
        'Field ' + fieldName + ' contains source scope identifiers and was not rewritten automatically.'
      );
    }}
  }}

  if (output.warnings.length > 0) {{
    output.requires_confirmation = true;
  }}

  if (!input.dryRun && output.requires_confirmation && !input.force) {{
    output.error = 'Risky record contains additional scope-coupled values. Re-run with --yes after reviewing warnings.';
    gs.print(JSON.stringify(output));
    return;
  }}

  if (!input.dryRun) {{
    record.setValue('sys_scope', output.after.sys_scope);
    record.setValue('sys_package', output.after.sys_package);

    if (record.isValidField('sys_name') && output.after.sys_name !== output.before.sys_name) {{
      record.setValue('sys_name', output.after.sys_name);
    }}
    if (record.isValidField('sys_update_name') && output.after.sys_update_name !== output.before.sys_update_name) {{
      record.setValue('sys_update_name', output.after.sys_update_name);
    }}
    if (record.isValidField('api_name') && output.after.api_name !== output.before.api_name) {{
      record.setValue('api_name', output.after.api_name);
    }}
    if (record.isValidField('path') && output.after.path !== output.before.path) {{
      record.setValue('path', output.after.path);
    }}

    var updatedSysId = record.update();
    if (!updatedSysId) {{
      fail('ServiceNow rejected the update for ' + input.table + ' / ' + input.sysId);
      return;
    }}
  }}

  output.ok = true;
  gs.print(JSON.stringify(output));
}} catch (error) {{
  output.error = text(error);
  gs.print(JSON.stringify(output));
}}
}})();"#
    ))
}

async fn collect_scope_data(
    profile: &str,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
    scope_input: &EncodedQueryValue,
) -> anyhow::Result<CollectedScopeData> {
    let mut client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
    let pagination = PaginationConfig::default();

    let scope_query = format!("scope={scope_input}^ORsys_id={scope_input}");
    let sys_scope = TableName::from_static("sys_scope");
    let scopes = client
        .get_table_records(
            &sys_scope,
            Some(&scope_query),
            Some("sys_id,scope,name,version"),
            &pagination,
            None,
        )
        .await?;

    let scope_record = scopes
        .first()
        .ok_or_else(|| anyhow::anyhow!("Scope '{scope_input}' was not found in sys_scope"))?;

    let scope = ScopeInfo {
        sys_id: field_text(scope_record, "sys_id"),
        scope: field_text(scope_record, "scope"),
        name: field_text(scope_record, "name"),
        version: field_text(scope_record, "version"),
    };

    let mut warnings = Vec::new();
    let mut artifact_sets = Vec::new();

    for definition in ARTIFACT_DEFINITIONS {
        let records = fetch_records_for_scope(
            &mut client,
            &scope.sys_id,
            definition.table,
            definition.fields,
            &mut warnings,
        )
        .await;

        artifact_sets.push(CollectedArtifactSet {
            category: definition.category.to_string(),
            artifact_type: definition.artifact_type.to_string(),
            source_table: definition.table.to_string(),
            name_field: definition.name_field.to_string(),
            records,
        });
    }

    let table_names = artifact_sets
        .iter()
        .find(|set| set.artifact_type == "tables")
        .map(|set| {
            set.records
                .iter()
                .filter_map(|record| record.get_str("name").map(ToString::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let table_name_refs = table_names.iter().map(String::as_str).collect::<Vec<_>>();

    let dictionary = fetch_dictionary_records(&mut client, &table_name_refs, &mut warnings).await;
    artifact_sets.push(CollectedArtifactSet {
        category: "data_model_logic".to_string(),
        artifact_type: "dictionary_fields".to_string(),
        source_table: "sys_dictionary".to_string(),
        name_field: "element".to_string(),
        records: dictionary,
    });

    let choices = fetch_choice_records(&mut client, &table_name_refs, &mut warnings).await;
    artifact_sets.push(CollectedArtifactSet {
        category: "data_model_logic".to_string(),
        artifact_type: "choices".to_string(),
        source_table: "sys_choice".to_string(),
        name_field: "label".to_string(),
        records: choices,
    });

    let known_source_tables = artifact_sets
        .iter()
        .map(|set| set.source_table.clone())
        .collect::<HashSet<_>>();

    let other_rows = fetch_other_metadata_rows(
        &mut client,
        &scope.scope,
        &scope.sys_id,
        &known_source_tables,
        &mut warnings,
    )
    .await;

    let mut data = CollectedScopeData {
        scope,
        summary: ScopeSummary {
            total_artifacts: 0,
            artifact_counts: BTreeMap::new(),
            category_counts: BTreeMap::new(),
        },
        artifact_sets,
        other_rows,
        warnings,
    };

    let rows = data.to_inventory_rows();
    data.summary = ScopeSummary::from_rows(&rows);

    Ok(data)
}

async fn fetch_records_for_scope(
    client: &mut crate::client::SnowClient,
    scope_sys_id: &str,
    table: &'static str,
    fields: &str,
    warnings: &mut Vec<String>,
) -> Vec<Record> {
    let query = format!("sys_scope={scope_sys_id}");
    let fields = format!("{fields},sys_scope");
    let pagination = PaginationConfig::default();
    let table_name = TableName::from_static(table);
    match client
        .get_table_records(&table_name, Some(&query), Some(&fields), &pagination, None)
        .await
    {
        Ok(records) => {
            if records.is_empty() {
                return records;
            }

            let has_scope_field = records
                .iter()
                .any(|record| record.fields.contains_key("sys_scope"));
            if !has_scope_field {
                warnings.push(format!(
                    "Skipped {table}: records returned without sys_scope field, cannot verify scope-safe filtering"
                ));
                return Vec::new();
            }

            let original_count = records.len();
            let filtered = records
                .into_iter()
                .filter(|record| field_text(record, "sys_scope") == scope_sys_id)
                .collect::<Vec<_>>();

            if filtered.len() != original_count {
                warnings.push(format!(
                    "Filtered {table} from {original_count} to {} records after sys_scope validation",
                    filtered.len()
                ));
            }

            filtered
        }
        Err(err) => {
            warnings.push(format!("Failed to query {table}: {err}"));
            Vec::new()
        }
    }
}

async fn fetch_dictionary_records(
    client: &mut crate::client::SnowClient,
    table_names: &[&str],
    warnings: &mut Vec<String>,
) -> Vec<Record> {
    if table_names.is_empty() {
        return Vec::new();
    }

    for table_name in table_names {
        if let Err(err) = table_name.parse::<TableName>() {
            warnings.push(format!(
                "Skipped sys_dictionary query for invalid table name '{table_name}': {err}"
            ));
            return Vec::new();
        }
    }

    let query = build_dictionary_query(table_names);
    let pagination = PaginationConfig::default().with_page_size(200);
    let sys_dictionary = TableName::from_static("sys_dictionary");

    match client
        .get_table_records(
            &sys_dictionary,
            Some(&query),
            Some("sys_id,name,element,column_label,internal_type,reference"),
            &pagination,
            None,
        )
        .await
    {
        Ok(records) => records,
        Err(err) => {
            warnings.push(format!("Failed to query sys_dictionary: {err}"));
            Vec::new()
        }
    }
}

async fn fetch_choice_records(
    client: &mut crate::client::SnowClient,
    table_names: &[&str],
    warnings: &mut Vec<String>,
) -> Vec<Record> {
    if table_names.is_empty() {
        return Vec::new();
    }

    for table_name in table_names {
        if let Err(err) = table_name.parse::<TableName>() {
            warnings.push(format!(
                "Skipped sys_choice query for invalid table name '{table_name}': {err}"
            ));
            return Vec::new();
        }
    }

    let query = format!("nameIN{}", table_names.join(","));
    let pagination = PaginationConfig::default().with_page_size(200);
    let sys_choice = TableName::from_static("sys_choice");

    match client
        .get_table_records(
            &sys_choice,
            Some(&query),
            Some("sys_id,name,element,value,label,inactive"),
            &pagination,
            None,
        )
        .await
    {
        Ok(records) => records,
        Err(err) => {
            warnings.push(format!("Failed to query sys_choice: {err}"));
            Vec::new()
        }
    }
}

async fn fetch_other_metadata_rows(
    client: &mut crate::client::SnowClient,
    scope: &str,
    scope_sys_id: &str,
    known_source_tables: &HashSet<String>,
    warnings: &mut Vec<String>,
) -> Vec<ScopeInventoryRow> {
    let query = format!("sys_scope={scope_sys_id}");
    let pagination = PaginationConfig::default();
    let sys_metadata = TableName::from_static("sys_metadata");
    let metadata_records = match client
        .get_table_records(
            &sys_metadata,
            Some(&query),
            Some("sys_id,name,sys_class_name"),
            &pagination,
            None,
        )
        .await
    {
        Ok(records) => records,
        Err(err) => {
            warnings.push(format!(
                "Failed to query sys_metadata for other artifacts: {err}"
            ));
            return Vec::new();
        }
    };

    metadata_records
        .iter()
        .filter_map(|record| {
            let class_name = field_text(record, "sys_class_name");
            if !class_name.is_empty() && known_source_tables.contains(&class_name) {
                return None;
            }

            let source_table = if class_name.is_empty() {
                "unknown".to_string()
            } else {
                class_name
            };

            Some(ScopeInventoryRow {
                scope: scope.to_string(),
                scope_sys_id: scope_sys_id.to_string(),
                category: "other".to_string(),
                artifact_type: "other".to_string(),
                source_table,
                sys_id: field_text(record, "sys_id"),
                name: field_text(record, "name"),
            })
        })
        .collect()
}

fn build_dictionary_query(table_names: &[&str]) -> String {
    format!(
        "nameIN{}^elementISNOTEMPTY^element!=sys_tags",
        table_names.join(",")
    )
}

fn field_text(record: &Record, field: &str) -> String {
    record
        .fields
        .get(field)
        .and_then(value_as_text)
        .unwrap_or_default()
}

fn value_as_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Object(map) => map
            .get("value")
            .and_then(|inner| inner.as_str())
            .map(ToString::to_string),
        _ => None,
    }
}

async fn list_scopes(
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

fn filter_scope_list_rows(rows: Vec<ScopeListRow>, kinds: &[ScopeListKind]) -> Vec<ScopeListRow> {
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

async fn query_optional_table(
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

fn build_scope_search_query(search: Option<&EncodedQueryValue>) -> anyhow::Result<Option<String>> {
    Ok(search.map(|search| {
        format!("scope={search}^ORsys_id={search}^ORscopeLIKE{search}^ORnameLIKE{search}")
    }))
}

fn build_plugin_search_query(search: Option<&EncodedQueryValue>) -> anyhow::Result<Option<String>> {
    Ok(search.map(|search| format!("id={search}^ORidLIKE{search}^ORnameLIKE{search}")))
}

fn build_scope_list_rows(
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

fn classify_scope_kind(scope: &str, is_store_app: bool) -> &'static str {
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

fn print_scope_list_text(
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

#[derive(Debug, Clone, Copy)]
struct ScopeListTextOptions {
    show_source_table: bool,
    show_sys_id: bool,
}

fn group_scope_rows_by_kind(rows: &[ScopeListRow]) -> Vec<(&str, Vec<&ScopeListRow>)> {
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

fn scope_row_key(row: &ScopeListRow) -> &str {
    if row.scope.is_empty() {
        row.identifier.as_str()
    } else {
        row.scope.as_str()
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct MoveFileOutput {
    ok: bool,
    dry_run: bool,
    table: String,
    sys_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_scope: Option<ScopeInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target_scope: Option<ScopeInfo>,
    #[serde(default)]
    before: MoveFileState,
    #[serde(default)]
    after: MoveFileState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    changed_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
    #[serde(default)]
    requires_confirmation: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct MoveFileState {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    sys_scope: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    sys_package: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    sys_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    sys_update_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    api_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    path: String,
}

#[derive(Debug, serde::Serialize)]
struct ScopeInspectOutput {
    scope: ScopeInfo,
    details: String,
    summary: ScopeSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    artifacts: Option<Vec<ScopeInventoryRow>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ScopeInfo {
    sys_id: String,
    scope: String,
    name: String,
    version: String,
}

#[derive(Debug, serde::Serialize)]
struct ScopeListOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    search: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    kind_filter: Vec<String>,
    counts: ScopeListCounts,
    rows: Vec<ScopeListRow>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
struct ScopeListCounts {
    total: usize,
    by_kind: BTreeMap<String, usize>,
}

impl ScopeListCounts {
    fn from_rows(rows: &[ScopeListRow]) -> Self {
        let mut by_kind = BTreeMap::new();
        for row in rows {
            *by_kind.entry(row.kind.clone()).or_insert(0) += 1;
        }

        Self {
            total: rows.len(),
            by_kind,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct ScopeListRow {
    kind: String,
    scope: String,
    name: String,
    version: String,
    identifier: String,
    source_table: String,
    sys_id: String,
}

#[derive(Debug, serde::Serialize)]
struct ScopeSummary {
    total_artifacts: usize,
    artifact_counts: BTreeMap<String, usize>,
    category_counts: BTreeMap<String, usize>,
}

impl ScopeSummary {
    fn from_rows(rows: &[ScopeInventoryRow]) -> Self {
        let mut artifact_counts = BTreeMap::new();
        let mut category_counts = BTreeMap::new();

        for row in rows {
            *artifact_counts
                .entry(row.artifact_type.clone())
                .or_insert(0) += 1;
            *category_counts.entry(row.category.clone()).or_insert(0) += 1;
        }

        Self {
            total_artifacts: rows.len(),
            artifact_counts,
            category_counts,
        }
    }

    fn to_csv_rows(&self, scope: &str, scope_sys_id: &str) -> Vec<ScopeInspectCsvRow> {
        self.artifact_counts
            .iter()
            .map(|(artifact, count)| ScopeInspectCsvRow {
                scope: scope.to_string(),
                scope_sys_id: scope_sys_id.to_string(),
                artifact: artifact.clone(),
                count: *count,
            })
            .collect()
    }
}

#[derive(Debug, serde::Serialize)]
struct ScopeInspectCsvRow {
    scope: String,
    scope_sys_id: String,
    artifact: String,
    count: usize,
}

#[derive(Debug, serde::Serialize)]
struct ScopeInventoryOutput {
    scope: ScopeInfo,
    summary: ScopeSummary,
    rows: Vec<ScopeInventoryRow>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct ScopeInventoryRow {
    scope: String,
    scope_sys_id: String,
    category: String,
    artifact_type: String,
    source_table: String,
    sys_id: String,
    name: String,
}

struct CollectedArtifactSet {
    category: String,
    artifact_type: String,
    source_table: String,
    name_field: String,
    records: Vec<Record>,
}

struct CollectedScopeData {
    scope: ScopeInfo,
    summary: ScopeSummary,
    artifact_sets: Vec<CollectedArtifactSet>,
    other_rows: Vec<ScopeInventoryRow>,
    warnings: Vec<String>,
}

impl CollectedScopeData {
    fn to_inventory_rows(&self) -> Vec<ScopeInventoryRow> {
        let mut rows = Vec::new();
        for set in &self.artifact_sets {
            rows.extend(map_inventory_rows(
                &self.scope,
                &set.category,
                &set.artifact_type,
                &set.source_table,
                &set.records,
                &set.name_field,
            ));
        }
        rows.extend(self.other_rows.clone());
        rows
    }
}

fn map_inventory_rows(
    scope: &ScopeInfo,
    category: &str,
    artifact_type: &str,
    source_table: &str,
    records: &[Record],
    name_field: &str,
) -> Vec<ScopeInventoryRow> {
    records
        .iter()
        .map(|record| ScopeInventoryRow {
            scope: scope.scope.clone(),
            scope_sys_id: scope.sys_id.clone(),
            category: category.to_string(),
            artifact_type: artifact_type.to_string(),
            source_table: source_table.to_string(),
            sys_id: field_text(record, "sys_id"),
            name: field_text(record, name_field),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_dictionary_query() {
        let query = build_dictionary_query(&["x_app_table", "x_app_second"]);
        assert_eq!(
            query,
            "nameINx_app_table,x_app_second^elementISNOTEMPTY^element!=sys_tags"
        );
    }

    #[test]
    fn test_build_scope_search_query() {
        let global: EncodedQueryValue = "global".parse().unwrap();
        assert_eq!(
            build_scope_search_query(Some(&global)).unwrap(),
            Some("scope=global^ORsys_id=global^ORscopeLIKEglobal^ORnameLIKEglobal".to_string())
        );
        assert_eq!(build_scope_search_query(None).unwrap(), None);
        // Invalid characters are now rejected at construction time, before
        // `build_scope_search_query` is ever called.
        assert!("global^ORactive=true".parse::<EncodedQueryValue>().is_err());
    }

    #[test]
    fn test_build_scope_list_rows_classifies_scope_origins() {
        let scopes = vec![
            record(&[
                ("sys_id", "scope-store"),
                ("scope", "sn_store_app"),
                ("name", "Store App"),
                ("version", "1.0.0"),
            ]),
            record(&[
                ("sys_id", "scope-custom"),
                ("scope", "x_acme_ops"),
                ("name", "Acme Ops"),
                ("version", "1.0.0"),
            ]),
            record(&[
                ("sys_id", "scope-platform"),
                ("scope", "global"),
                ("name", "Global"),
                ("version", ""),
            ]),
            record(&[
                ("sys_id", "scope-oob"),
                ("scope", "sn_ot_incident_mgmt"),
                ("name", "OT Incident Management"),
                ("version", "2.0.0"),
            ]),
        ];
        let store_apps = vec![record(&[
            ("sys_id", "store-1"),
            ("scope", "sn_store_app"),
            ("name", "Store App"),
            ("version", "1.0.0"),
        ])];
        let plugins = vec![record(&[
            ("sys_id", "plugin-1"),
            ("id", "com.snc.example"),
            ("name", "Example Plugin"),
        ])];

        let rows = build_scope_list_rows(scopes, store_apps, plugins);

        assert!(
            rows.iter()
                .any(|row| row.scope == "sn_store_app" && row.kind == "store_app")
        );
        assert!(
            rows.iter()
                .any(|row| row.scope == "x_acme_ops" && row.kind == "custom_app")
        );
        assert!(
            rows.iter()
                .any(|row| row.scope == "global" && row.kind == "platform")
        );
        assert!(
            rows.iter()
                .any(|row| row.scope == "sn_ot_incident_mgmt" && row.kind == "platform_app")
        );
        assert!(
            rows.iter()
                .any(|row| row.identifier == "com.snc.example" && row.kind == "plugin")
        );
    }

    #[test]
    fn test_filter_scope_list_rows_by_kind() {
        let rows = vec![
            ScopeListRow {
                kind: "store_app".to_string(),
                scope: "sn_store_app".to_string(),
                name: "Store App".to_string(),
                version: "1.0.0".to_string(),
                identifier: String::new(),
                source_table: "sys_scope".to_string(),
                sys_id: "1".to_string(),
            },
            ScopeListRow {
                kind: "plugin".to_string(),
                scope: String::new(),
                name: "Plugin".to_string(),
                version: String::new(),
                identifier: "com.snc.example".to_string(),
                source_table: "v_plugin".to_string(),
                sys_id: "2".to_string(),
            },
        ];

        let filtered = filter_scope_list_rows(rows, &[ScopeListKind::Plugin]);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].kind, "plugin");
    }

    #[test]
    fn test_group_scope_rows_by_kind_preserves_display_order() {
        let rows = vec![
            ScopeListRow {
                kind: "plugin".to_string(),
                scope: String::new(),
                name: "Plugin".to_string(),
                version: String::new(),
                identifier: "com.snc.example".to_string(),
                source_table: "v_plugin".to_string(),
                sys_id: "2".to_string(),
            },
            ScopeListRow {
                kind: "custom_app".to_string(),
                scope: "x_acme_app".to_string(),
                name: "Acme App".to_string(),
                version: "1.0.0".to_string(),
                identifier: String::new(),
                source_table: "sys_scope".to_string(),
                sys_id: "1".to_string(),
            },
        ];

        let grouped = group_scope_rows_by_kind(&rows);

        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].0, "custom_app");
        assert_eq!(grouped[1].0, "plugin");
    }

    fn record(fields: &[(&str, &str)]) -> Record {
        Record {
            fields: fields
                .iter()
                .map(|(key, value)| (key.to_string(), serde_json::json!(value)))
                .collect(),
        }
    }

    #[test]
    fn test_value_as_text_from_reference_object() {
        let value = serde_json::json!({"link": "https://example", "value": "abc123"});
        assert_eq!(value_as_text(&value), Some("abc123".to_string()));
    }

    #[test]
    fn test_scope_summary_counts_by_category_and_artifact_type() {
        let rows = vec![
            ScopeInventoryRow {
                scope: "x_app".to_string(),
                scope_sys_id: "id".to_string(),
                category: "server_logic".to_string(),
                artifact_type: "script_includes".to_string(),
                source_table: "sys_script_include".to_string(),
                sys_id: "1".to_string(),
                name: "SI1".to_string(),
            },
            ScopeInventoryRow {
                scope: "x_app".to_string(),
                scope_sys_id: "id".to_string(),
                category: "other".to_string(),
                artifact_type: "other".to_string(),
                source_table: "x_custom_meta".to_string(),
                sys_id: "2".to_string(),
                name: "Meta".to_string(),
            },
        ];

        let summary = ScopeSummary::from_rows(&rows);
        assert_eq!(summary.total_artifacts, 2);
        assert_eq!(summary.category_counts.get("server_logic"), Some(&1));
        assert_eq!(summary.category_counts.get("other"), Some(&1));
        assert_eq!(summary.artifact_counts.get("script_includes"), Some(&1));
        assert_eq!(summary.artifact_counts.get("other"), Some(&1));
    }

    #[test]
    fn test_build_move_file_script_contains_inputs() {
        let script =
            build_move_file_script("sys_script_include", "abc123", "x_target_app", true, false)
                .unwrap();

        assert!(script.contains("sys_script_include"));
        assert!(script.contains("abc123"));
        assert!(script.contains("x_target_app"));
        assert!(script.contains("\"dryRun\":true"));
        assert!(script.contains("\"force\":false"));
        assert!(script.contains("sys_update_name"));
        assert!(script.contains("api_name"));
    }

    #[test]
    fn test_build_move_file_script_limits_records_to_sys_metadata_tables() {
        let script =
            build_move_file_script("sys_script_include", "abc123", "x_target_app", true, false)
                .unwrap();

        assert!(script.contains("tableExtends(input.table, 'sys_metadata')"));
        assert!(script.contains("Unsupported record: table must extend sys_metadata"));
        assert!(script.contains("new GlideRecord('sys_db_object')"));
        assert!(script.contains("getValue('super_class')"));
    }
}
