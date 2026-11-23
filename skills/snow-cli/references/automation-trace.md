# Automation & Execution Trace

Use when answering: "Why did this field/record change, fail to save, or trigger something unexpected?" or "What automation runs on table `T`?"

All commands below are read-only and use `snow-cli-ro`.

## 1. Key tables

| Table | What it stores | Key fields to project |
|---|---|---|
| `sys_script` | Business Rules (server) | `name,collection,when,order,action_insert,action_update,action_delete,action_query,filter_condition,condition,active,sys_scope` |
| `sys_hub_flow` | Flow Designer flows/subflows | `name,internal_name,type,status,active,sys_scope` |
| `sys_script_include` | Reusable server classes | Use `snow-cli-ro include show <Name> --chain` |
| `sys_data_policy2` | Data Policies (server + optional UI) | `short_description,model_table,conditions,enforce_ui,apply_soap,apply_import_set,active,reverse_if_false` |
| `sys_data_policy_rule` | Data Policy field actions | `sys_data_policy,field,mandatory,disabled` |
| `sys_script_client` | Client Scripts (form only) | `name,table,type,field,ui_type,applies_extended,active,sys_scope` |
| `sys_ui_policy` | UI Policies (form only) | `short_description,table,conditions,order,inherit,on_load,reverse_if_false,active` |
| `sys_ui_policy_action` | UI Policy field actions | `ui_policy,field,mandatory,visible,disabled,cleared` |
| `sys_ui_action` | Buttons / links / context actions | `name,table,action_name,client,condition,order,active` |
| `sysevent_script_action` | Script Actions (async on event) | `name,event_name,condition_script,active` |
| `sysauto_script` | Scheduled Script Executions | `name,run_type,active,sys_scope` |

## 2. Find the table hierarchy first

Server-side automation on a parent table (especially `task` for `incident`/`change_request`/`problem`, or `cmdb_ci` for CI subclasses) also runs on child tables. Always collect the full inheritance chain first:

```bash
# Inspect parent table chain (repeat on the returned `extends` until null)
snow-cli-ro table find incident --limit 5
snow-cli-ro table list sys_db_object --query 'nameINincident,task' --fields name,label,super_class.name,sys_scope.scope
```

Build a comma-separated list of the table plus its ancestors (e.g. `incident,task`) and use `IN` in queries below.

## 3. Trace server-side automation (in execution order)

Typical record-save order:
1. **Before BRs** (`order < 1000`) → **Before engines** (Data Policies, Assignment Rules, Data Lookup, Approval, Field Normalization, Default Workflows) → **Before BRs** (`order >= 1000`)
2. **DB write** (insert / update / delete)
3. **After BRs** (`order < 1000`) → **After engines** (Flow Designer / Workflow Studio triggers, Deferred Workflows, Table Notifications) → **Email notifications** (by weight) → **After BRs** (`order >= 1000`)
4. **Async BRs** & **Events / Script Actions** (run asynchronously via scheduler)

```bash
# 1. Active Business Rules on table and ancestors, sorted by phase & order
snow-cli-ro table list sys_script \
  --query 'collectionINincident,task^active=true' \
  --fields sys_id,name,collection,when,order,action_insert,action_update,action_delete,action_query,filter_condition,condition \
  --order-by when,order --limit 100

# Read a specific Business Rule's script verbatim
snow-cli-ro table get sys_script <sys_id_or_name> --fields script --raw

# 2. Active Data Policies (enforced on Table API / Import Sets when apply_soap / apply_import_set is true)
snow-cli-ro table list sys_data_policy2 \
  --query 'model_tableINincident,task^active=true' \
  --fields sys_id,short_description,model_table,conditions,enforce_ui,apply_soap,apply_import_set,reverse_if_false

snow-cli-ro table list sys_data_policy_rule \
  --query 'sys_data_policy=<policy_sys_id>' \
  --fields field,mandatory,disabled

# 3. Active Flow Designer flows touching the table (via trigger instances or flow search)
snow-cli-ro table list sys_hub_trigger_instance \
  --query 'trigger_inputsLIKEincident^flow.active=true' \
  --fields flow.name,flow.internal_name,flow.sys_id,trigger_definition.name --limit 50

# Compact flow tree (trigger, branches, steps in order)
snow-cli-ro flow show '<flow_name_or_sys_id>'

# 4. Follow Script Includes referenced from any BR/Flow/Action
snow-cli-ro include show ChangeRequestStateHandler --chain --source-of canMoveTo,_resetModel
```

## 4. Trace client-side / UI automation (form-only behavior)

```bash
# Client Scripts (note: parent-table Client Scripts only apply if applies_extended=true)
snow-cli-ro table list sys_script_client \
  --query 'table=incident^active=true^NQtable=task^applies_extended=true^active=true' \
  --fields sys_id,name,table,type,field,ui_type,applies_extended --limit 100

# UI Policies (parent-table UI Policies only apply if inherit=true)
snow-cli-ro table list sys_ui_policy \
  --query 'table=incident^active=true^NQtable=task^inherit=true^active=true' \
  --fields sys_id,short_description,table,order,conditions,on_load,reverse_if_false --order-by order --limit 100

snow-cli-ro table list sys_ui_policy_action --query 'ui_policy=<policy_sys_id>' --fields field,mandatory,visible,disabled,cleared

# UI Actions (buttons/links)
snow-cli-ro table list sys_ui_action \
  --query 'tableINincident,task,global^active=true' \
  --fields sys_id,name,table,action_name,client,order,condition --order-by order --limit 50
```

## 5. Find callers writing to a specific field

When you don't know *what* touches a field (e.g. `assignment_group` or `state`), use Code Search across all script tables at once:

```bash
snow-cli-ro codesearch search 'assignment_group' --limit 25
snow-cli-ro codesearch search 'setValue' --source-table sys_script --scope x_my_app --limit 50
```

## 6. Gotchas

- **Parent tables matter differently per artifact:** Business Rules on `task` always run on `incident`; Client Scripts and UI Policies on `task` only run on `incident` when `applies_extended=true` / `inherit=true`.
- **Client vs API:** Client Scripts and UI Policies only run in the browser UI — they do **not** run on Table API (`snow-cli table create` / `snow-cli table update`), Import Sets, or background scripts. Data Policies *do* run server-side when `apply_soap=true` / `apply_import_set=true`.
- **`setWorkflow(false)` bypass:** A script calling `gr.setWorkflow(false)` skips Business Rules and engines for that operation — search for `setWorkflow` if expected BRs didn't fire.
- **Before Query BRs** (`when=before^action_query=true`) only run on the directly queried table, not on tables reached via reference fields, and can silently hide rows just like ACLs.
- **Async BRs** run on a scheduled job and have no access to `previous` inside the script body (though the condition builder supports `changes()`/`changesTo()`).
- **Flows in `_v2` tables:** `flow show` automatically falls back to published snapshots and `*_v2` tables, where trigger conditions and step notes may appear empty because ServiceNow stores them compressed.
