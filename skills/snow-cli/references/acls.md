# ACL & Access Diagnosis

Use when answering: "Why can/can't user X read, write, create, delete, or query table `T` (or field `T.f`)?"

All commands below are read-only and use `snow-cli-ro`.

## 1. Key tables

| Table | What it stores | Key fields to project |
|---|---|---|
| `sys_security_acl` | ACL rules | `name,type,operation,decision_type,active,admin_overrides,applies_to,condition,advanced,script,security_attribute,sys_scope` |
| `sys_security_acl_role` | Roles required by an ACL | `sys_security_acl,sys_user_role.name` |
| `sys_user` | User account & status | `sys_id,user_name,name,active,locked_out,sys_domain` |
| `sys_user_has_role` | Effective user roles (incl. inherited) | `user,role.name,state,inherited,granted_by` |
| `sys_properties` | Security manager defaults | `name,value` (`glide.sm.default_mode`) |

## 2. How ServiceNow evaluates record ACLs

A user must pass **both** a table/row-level ACL and a field-level ACL:

1. **Decision type priority:** **Deny-Unless** ACLs (`decision_type=deny_unless`) are evaluated **before** **Allow-If** ACLs (`decision_type=allow_if`).
   - If *any* matching Deny-Unless ACL fails, access is **denied** immediately (all matching Deny-Unless ACLs must pass).
   - Passing Deny-Unless alone does not grant access if Allow-If ACLs also exist — an Allow-If ACL must match and pass (or none exist).
2. **Match order (most specific → least specific):**
   - **Row level:** `<table>` (same as `<table>.None`) → `<parent_table>` → ... → `*`
   - **Field level:** `<table>.<field>` → `<parent_table>.<field>` → `*.<field>` → `<table>.*` → `<parent_table>.*` → `*.*`
   - Once rules exist at a specificity level (e.g. `incident.short_description`), ServiceNow evaluates *only* that level for Allow-If — it does not fall back to `incident.*` if the specific level fails.
3. **Within a single ACL:** **All** configured checks must pass: `Requires role` (any one listed role matches) **AND** `security_attribute` **AND** `condition` **AND** `script` (when `advanced=true`, sets `answer = true`). **Warning:** an empty ACL (no role, security attribute, condition, or script) **grants access** by default because all zero configured checks pass — a common misconfiguration that leaves a table or field wide open.
4. **`applies_to` vs `condition`:** `applies_to` filters whether the ACL applies to a record at all (case-sensitive); `condition` is an evaluation criterion once applied.

## 3. Step-by-step diagnosis

```bash
# 1. Check the user's status and effective roles
snow-cli-ro table list sys_user \
  --query 'user_name=abel.tuter' \
  --fields sys_id,user_name,active,locked_out,sys_domain

snow-cli-ro table list sys_user_has_role \
  --query 'user=<user_sys_id>^state=active' \
  --fields role.name,inherited,granted_by --all

# 2. Determine table inheritance chain (e.g. incident -> task)
snow-cli-ro table list sys_db_object \
  --query 'nameINincident,task' \
  --fields name,super_class.name

# 3. Fetch all active ACLs for the table/field chain and wildcards for an operation
# Example: diagnosing `read` on `incident.short_description` (parent: `task`)
snow-cli-ro table list sys_security_acl \
  --query 'active=true^type=record^operation=read^nameINincident,incident.short_description,incident.*,task,task.short_description,task.*,*.short_description,*,*.*' \
  --fields sys_id,name,operation,decision_type,admin_overrides,applies_to,condition,advanced,security_attribute.label \
  --order-by name --limit 100

# 4. Fetch required roles for the matching ACL sys_ids
snow-cli-ro table list sys_security_acl_role \
  --query 'sys_security_aclIN<acl_sys_id_1>,<acl_sys_id_2>' \
  --fields sys_security_acl.name,sys_security_acl,sys_user_role.name --limit 100

# 5. Inspect script body on any ACL with advanced=true
snow-cli-ro table get sys_security_acl <acl_sys_id> --fields script --raw

# 6. Check default deny mode (applies when only wildcard table ACLs match)
snow-cli-ro table list sys_properties --query 'name=glide.sm.default_mode' --fields name,value
```

## 4. Check Query ACLs (`query_match` / `query_range`)

If a user can read records generally, or gets unexpected filter failures/blocks on sensitive fields, check Query ACLs:

```bash
snow-cli-ro table list sys_security_acl \
  --query 'active=true^type=record^operationINquery_match,query_range,conditional_table_query_range^nameSTARTSWITHincident^ORnameSTARTSWITHtask' \
  --fields sys_id,name,operation,decision_type,admin_overrides,condition,advanced --limit 50
```

- `query_match` gates exact-match operators (`=`, `!=`, `IN`, `ISEMPTY`, etc.).
- `query_range` gates substring/range operators (`STARTSWITH`, `CONTAINS`, `<`, `>`, etc.) and sorting.

## 5. Non-ACL visibility blockers to rule out

If ACLs look permissive but the user still cannot see rows or fields, check:

1. **Before Query Business Rules:** `table list sys_script --query 'collectionINincident,task^when=before^action_query=true^active=true'` — these rewrite the query before ACLs run.
2. **Domain Separation:** see `references/domain-separation.md` (`snow-cli-ro domain status` / `domain table <table>`).
3. **Data Filters / Security Data Filters** (`sys_security_data_filter`): row-level filters evaluated independently of classic ACLs.
4. **Cross-scope table access** (when accessed from a scoped script, not a user): see `references/scopes-and-updates.md`.

## 6. Gotchas

- **Pre-query vs post-query ACLs:** Before executing a query, ServiceNow evaluates **role checks only** (no record context yet). After fetching rows, it evaluates full role + condition + script. A user whose role passes pre-query but fails a record-dependent condition may see a blank field value rather than a hidden column.
- **`admin_overrides=false`:** Even `admin` users must satisfy roles/conditions/scripts when `admin_overrides` is false (common on HR, SecOps, and Deny-Unless ACLs).
- **`create` ACLs with field conditions:** New records have empty fields before insert, so a `create` ACL checking a populated field value often evaluates false.
- **Reading ACL tables requires privilege:** Non-admin `snow-cli` credentials frequently get 403 or empty results on `sys_security_acl` / `sys_properties`. Verify with `table stats sys_security_acl` before assuming no ACLs exist.
- **Non-record ACLs:** Scripted REST endpoints (`type=rest_endpoint`), Processors (`type=processor`), and client-callable Script Includes (`type=client_callable_script_include`) use `operation=execute`; UI Pages use `type=ui_page` with `operation=read`.
