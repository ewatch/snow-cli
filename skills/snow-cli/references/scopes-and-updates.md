# Scopes, Update Sets & Customizations

Use when answering: "What artifacts belong to scope `X`?", "What changed in update set `Y`?", "Was this OOTB artifact customized?", or "Why is cross-scope access blocked?"

All inspection commands below are read-only and work with `snow-cli-ro` unless explicitly marked as write/admin.

## 1. Key tables & CLI commands

| Command / Table | What it stores | Key fields to project |
|---|---|---|
| `snow-cli-ro scope list [search]` | Scopes classified by origin (`custom-app`, `store-app`, `plugin`, `platform`, `platform-app`) | `--kind`, `--limit`, `--offset`, `--all` |
| `snow-cli-ro scope inspect <scope>` | Scope metadata + artifact counts (`--details basic`) or rows (`--details full`) | `--limit <n>` (required for `global` with `full`) |
| `snow-cli-ro scope inventory <scope> --limit 200` | Normalized artifact rows across tables (great with `--output csv`) | `--limit <n>` (required for `global`) |
| `sys_update_set` | Update sets (state, scope, parent) | `sys_id,name,state,application.scope,parent.name,sys_updated_on` |
| `sys_update_xml` | Individual captured customer updates | `sys_id,name,type,action,target_name,update_set.name,application.scope,sys_updated_by,sys_updated_on` |
| `sys_update_version` | Version history of a metadata artifact (OOTB vs customized) | `name,state,source_table,source,sys_recorded_at,sys_updated_by` |
| `sys_upgrade_history_log` | Skipped / conflicted records during upgrades | `file_name,target_name,type,disposition,resolution_status,upgrade_history` |
| `sys_scope_privilege` | Cross-scope runtime privileges | `source_scope.scope,target_scope.scope,target_name,target_type,operation,status` |
| `sys_restricted_caller_access` | Restricted Caller Access (RCA) requests/grants | `source_scope.scope,target_scope.scope,source,target,operation,status` |
| `sys_db_object` | Table-level cross-scope access settings | `name,sys_scope.scope,access,read_access,create_access,update_access,delete_access,caller_access` |

## 2. Inventory a scope or custom app

```bash
# 1. Find scopes (e.g. custom apps)
snow-cli-ro scope list --kind custom-app
snow-cli-ro scope list incident --kind plugin --kind store-app

# 2. Fast artifact counts (one lightweight count request per artifact table)
snow-cli-ro scope inspect x_my_app --details basic

# 3. Full normalized artifact inventory (CSV or JSON)
snow-cli-ro scope inventory x_my_app --limit 500
snow-cli-ro --output csv scope inventory x_my_app --limit 500

# 4. Global scope MUST pass --limit when enumerating rows
snow-cli-ro scope inspect global --details basic
snow-cli-ro scope inventory global --limit 200
```

## 3. Audit update sets & check if a record is customized

```bash
# List in-progress or recently completed update sets
snow-cli-ro table list sys_update_set \
  --query 'state=in progress' \
  --fields sys_id,name,state,application.scope,parent.name,sys_updated_on \
  --order-by -sys_updated_on --limit 30

# List changes captured in a specific update set
snow-cli-ro table list sys_update_xml \
  --query 'update_set=<update_set_sys_id>' \
  --fields name,type,action,target_name,sys_updated_by,sys_updated_on \
  --order-by -sys_updated_on --limit 100

# Check version history for a specific artifact (name is "<table_name>_<sys_id>")
snow-cli-ro table list sys_update_version \
  --query 'name=sys_script_include_<record_sys_id>' \
  --fields state,source_table,source,sys_recorded_at,sys_updated_by \
  --order-by -sys_recorded_at --limit 20

# Check skipped upgrade items for an artifact or table
# (often ACL_DENIED over the Table API even for admin; fall back to the UI if so)
snow-cli-ro table list sys_upgrade_history_log \
  --query 'disposition=4^resolution_status=not_reviewed' \
  --fields file_name,target_name,type,disposition,resolution_status,sys_created_on \
  --order-by -sys_created_on --limit 50
```

## 4. Diagnose cross-scope access errors

When a scoped script fails with `Read/Write/Execute operation on ... from scope X was granted/denied` or RCA errors:

```bash
# 1. Check the target table's Application Access settings
snow-cli-ro table list sys_db_object \
  --query 'name=incident' \
  --fields name,sys_scope.scope,access,read_access,create_access,update_access,delete_access,caller_access

# 2. Check Cross-Scope Privilege records for the source scope
snow-cli-ro table list sys_scope_privilege \
  --query 'source_scope.scope=x_my_app' \
  --fields source_scope.scope,target_scope.scope,target_name,target_type,operation,status \
  --order-by status,target_name --limit 100

# 3. Check Restricted Caller Access (RCA) records (denied or requested)
snow-cli-ro table list sys_restricted_caller_access \
  --query 'source_scope.scope=x_my_app^status!=2' \
  --fields source_scope.scope,target_scope.scope,source,target,operation,status --display-value both --limit 50
```

## 5. Session context & moving files (guarded — write/session ops)

Only inspect session context when using a native cookie session profile; never switch scope/update set or run `scope move-file` unless explicitly requested:

```bash
# Read-only session inspection
snow-cli-ro --profile dev-session session show
snow-cli-ro --profile dev-session session options application
snow-cli-ro --profile dev-session session options updateset

# Write op (background script, blocked in snow-cli-ro): always --dry-run first!
snow-cli scope move-file sys_script_include <sys_id> --target-scope x_target_app --dry-run
```

## 6. Gotchas

- **Table permissions cap cross-scope privileges:** A `sys_scope_privilege` record set to `allowed` still cannot grant more access than the target `sys_db_object` allows (`read_access`, `create_access`, `update_access`, `delete_access`).
- **RCA lives in the target scope:** `sys_restricted_caller_access` records are evaluated when `caller_access` is `Caller Restriction` or `Caller Tracking` (common in HRSD, SecOps, and Workflow Studio).
- **`sys_update_xml.name` convention:** Update XML and `sys_update_version` key records by `<table_name>_<sys_id>` (e.g. `sys_script_46d44a4b2f13000044e0bfc8fb99b6fd`).
- **PDI warnings on `scope list`:** PDIs often return HTTP 403 on `sys_store_app` and HTTP 500 on `v_plugin`; `scope list` still succeeds using remaining sources and reports those two as warnings.
- **Session application ≠ guaranteed update-set capture:** Selecting an application/update set via `session switch` does not guarantee every REST write lands in that update set; record scope and endpoint semantics still apply.
