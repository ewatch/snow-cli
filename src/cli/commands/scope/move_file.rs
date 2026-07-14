use super::*;

pub(super) async fn handle_move_file(
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

pub(super) fn build_move_file_script(
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
