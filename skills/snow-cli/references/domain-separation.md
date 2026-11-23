# Domain Separation Diagnosis

Use when working on a domain-separated instance, or answering: "Why are records missing/hidden?", "Which domain owns these records?", or "Which domain overrides apply to this process?"

All `domain` commands and `--domain`/`--with-domain`/`--query-no-domain` reads work in `snow-cli-ro` (except `--counts all` and `session switch domain`).

## 1. Key CLI commands & tables

| Command / Table | What it tells you |
|---|---|
| `snow-cli-ro domain status` | Domain activation/query-method properties (when readable) + authenticated user's **home domain** (`sys_user.sys_domain`) |
| `snow-cli-ro domain list` | Visible domain records (`sys_id`, name, parent, active, path) |
| `snow-cli-ro domain table <table>` | Whether `sys_domain`, `sys_domain_path`, `sys_overrides` exist (including inherited) and which table defines them |
| `snow-cli-ro domain table <table> --counts` | Per-domain counts across visible domains + `cross_domain` hidden total (when user has `admin` or `query_no_domain_table_api`) |
| `snow-cli-ro domain inspect <domain_sys_id_or_global>` | Domain metadata, direct children, Contains relationships (both directions), and user/group visibility grants |
| `table list\|stats ... --domain <id_or_global>` | Exact record ownership filter (excludes children and `global`) |
| `table list\|get ... --with-domain` | Adds `sys_domain`, `sys_domain_path`, resolved `sys_domain_name`, and `domains_in_page` summary |
| `table list\|get ... --query-no-domain` | Sends `sysparm_query_no_domain=true` (requires `admin` or `query_no_domain_table_api`; ACLs still apply) |
| `domain` / `domain_contains` / `sys_user_visibility` / `sys_user_group_visibility` | Underlying domain hierarchy & visibility tables (inspected for you by `domain list` and `domain inspect`) |

## 2. Step-by-step diagnosis

```bash
# 1. Check whether domain separation is active and what the user's home domain is
snow-cli-ro domain status

# 2. List visible domains (use sys_ids, not names/paths, in downstream queries)
snow-cli-ro domain list --limit 100

# 3. Check if the table is domain-separated (walks parent hierarchy) and count records per domain
# Note: put the table name BEFORE bare --counts!
snow-cli-ro domain table incident --counts

# 4. Inspect a specific domain's hierarchy, Contains links, and visibility grants
snow-cli-ro domain inspect <domain_sys_id> --limit 100

# 5. Annotate query results with each record's resolved domain name
snow-cli-ro table list incident \
  --query 'active=true' \
  --fields number,short_description,state \
  --with-domain --limit 20

# 6. Filter exact domain ownership (excludes child domains and global)
snow-cli-ro table list incident --domain <domain_sys_id> --query 'active=true' --limit 20
snow-cli-ro table list incident --domain global --limit 20
snow-cli-ro table stats incident --domain <domain_sys_id> --group-by state

# 7. Privileged cross-domain read (explicit opt-in; requires admin or query_no_domain_table_api)
snow-cli-ro table list incident --query-no-domain --fields sys_id,number,sys_domain --with-domain --limit 20
snow-cli-ro table get incident <sys_id> --query-no-domain --with-domain
```

## 3. Process overrides (`sys_overrides`)

Process records (e.g. Business Rules `sys_script`, UI Policies `sys_ui_policy`, Script Includes `sys_script_include`, Notifications `sysevent_email_action`) can be overridden per domain via `sys_overrides`:

```bash
# Check if a process table supports domain overrides
snow-cli-ro domain table sys_script

# Find domain-specific overrides of a Business Rule or process record
snow-cli-ro table list sys_script \
  --query 'collection=incident^sys_overridesISNOTEMPTY' \
  --fields sys_id,name,active,sys_overrides,sys_overrides.name \
  --with-domain --limit 50
```

## 4. Session domain context (native cookie sessions)

If working with a native cookie session profile (`session show`), remember that **home domain**, **session picker domain**, and **record `sys_domain`** are three distinct concepts:

```bash
# Read-only session checks
snow-cli-ro --profile dev-session session show
snow-cli-ro --profile dev-session session options domain

# Switching changes picker context (mutates session; do not run unless asked)
snow-cli --profile dev-session session switch domain <domain_sys_id>
```

## 5. Gotchas

- **`DOMAIN_SEPARATION_INACTIVE`:** `domain list` and `domain inspect` return this when the domain tables don't exist (domain separation not installed). Treat the instance as unseparated; `domain status` still works.
- **Empty results ≠ missing records:** On a domain-separated instance, `total: 0` may mean the record sits in a sibling/ungranted domain. Always check `domain status` and `domain table <table> --counts` (`cross_domain.hidden`) before concluding a record doesn't exist.
- **`enabled: null` does NOT mean disabled:** Non-admin users usually cannot read domain system properties. `separation_in_use: true` proves separation when the user's home domain is not `global`; `null` means unknown.
- **`--domain <id>` is exact ownership only:** It excludes descendant domains and shared `global` records unless queried separately.
- **`--query-no-domain` silent ignore:** Without `admin` or `query_no_domain_table_api`, ServiceNow ignores `sysparm_query_no_domain=true` and returns only visible domains (look for the CLI's stderr warning). Also, the Aggregate API ignores it — `table stats` does not support `--query-no-domain`.
- **`--counts all` requires admin & writes script history:** It executes a read-only `GlideAggregate` background script (`queryNoDomain()`) and is blocked in `snow-cli-ro`.
- **Contains ≠ process inheritance:** Domain `Contains` relationships grant **data visibility**, not process override inheritance.
- **Never hardcode `sys_domain_path` filters:** Paths change whenever the domain tree is reparented; always key off `sys_domain` `sys_id` (or `global`).
