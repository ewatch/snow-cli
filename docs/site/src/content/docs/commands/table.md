# `table`

Use `table` for generic CRUD operations against ServiceNow tables.

```bash
snow-cli table <verb> [options]
```

All `table` subcommands also accept the global flags from the [command overview](/commands/).

## `table list <table>`

List records from a table.

```bash
snow-cli table list <table> [options]
```

Important options:

- `--query <encoded-query>`: ServiceNow encoded query string
- `--fields <a,b,c>`: comma-separated field list (default: a compact table-aware projection; pass `'*'` for all fields)
- `--limit <n>`: maximum number of records to return (default: 20)
- `--offset <n>`: skip the first `n` matching records (default: 0); works with `--limit` or `--all`
- `--all`: fetch every matching record instead of the bounded default
- `--order-by <spec>`: sort results. `field` sorts ascending, `-field` or `field:desc` descending; comma-separate several keys (`-priority,number`). Sent as `ORDERBY`/`ORDERBYDESC` clauses appended to `--query`, because the Table API has no separate sort parameter
- `--display-value [raw|display|both]`: return labels instead of stored codes. A bare `--display-value` means `display`; `false`, `true`, and `all` are accepted as aliases. See [Display values](#display-values)
- `--reference-links`: include ServiceNow reference link URLs (`{"link","value"}`) on reference fields. By default `table list` and `table get` send `sysparm_exclude_reference_link=true` so references return plain values
- `--check-fields warn|error|off`: how to treat names in `--fields`, `--query`, and `--order-by` that ServiceNow does not return (default `warn`, or `SNOW_CLI_CHECK_FIELDS`). See [Unknown-field warnings](#unknown-field-warnings)
- `--preview`: opt in to shortening oversized string fields and emitting separate preview metadata (JSON, JSONL, TOON, text, and auto output only)
- `--full`: explicit lossless escape hatch for commands copied from preview workflows; conflicts with `--preview`

Examples:

```bash
snow-cli table list incident --query 'active=true' --limit 20
snow-cli table list incident --query 'active=true' --fields number,priority,state,assignment_group --order-by -sys_created_on --limit 5 --display-value
snow-cli table list incident --fields number,priority,state --display-value both --output csv
snow-cli table list sys_user --fields sys_id,user_name,email --order-by user_name
snow-cli table list incident --fields number,sys_created_on --order-by -sys_created_on --limit 5  # newest first
snow-cli table list incident --order-by -sys_created_on --limit 3 --offset 3  # second page
snow-cli table list incident --all --fields '*' --full   # everything, uncapped
snow-cli table list incident --preview   # shorten oversized fields with metadata
```

Notes:

- `table list` auto-paginates until it reaches the requested limit or exhausts the result set.
- Output is bounded by default: at most 20 records without `--limit` or `--all`, and a compact field projection without `--fields`.
- Output is lossless by default, including explicit `--output json`, detail reads, `auto`, and export/artifact workflows.
- Pass `--preview` to shorten field values longer than 2,000 characters. The shortened value has no inline marker.
- Preview metadata appears outside the data value. It reports `path`, `original_chars`, `shown_chars`, and `escape_hatch` for each shortened field. Replace `--preview` with `--full`, or omit `--preview`, to retrieve complete field content.
- `--preview` is supported for JSON, JSONL, TOON, text, and auto output. It is rejected with CSV output because CSV has no metadata channel.
- JSONL output keeps its meta line plus record lines when no field is shortened. When preview metadata is present, JSONL emits one envelope line so each `path` points into the same JSON value.
- Except in CSV output, `table list` responses carry `total` when the server reports `X-Total-Count`, `returned`, `offset`, `next_offset`, `truncated`, and `fields_truncated` when `--preview` shortened a field. Use `next_offset` as the next call's `--offset` value. For repeatable pages, specify `--order-by`.
- For complete data set extraction prefer `data export`, which keeps full-export semantics.
- Use `--output csv` when you want tabular export. CSV output is lossless and does not carry preview metadata.

## `table get <table> <sys_id|name>`

Fetch a single record by `sys_id`, or by name.

```bash
snow-cli table get <table> <sys_id|name> [options]
```

Important options:

- `--fields <a,b,c>`: restrict the returned fields
- `--key-field <field>`: field matched when the record is not a 32-character hex `sys_id`. The default is `number` for task-style numbers such as `INC0010001`, and `name` otherwise
- `--raw`: print the single `--fields` value verbatim, without JSON escaping, followed by a newline. Requires exactly one field
- `--display-value [raw|display|both]`, `--reference-links`, and `--check-fields warn|error|off`: as on `table list`. Unknown `--fields` names are reported on stderr only, because `table get` prints the bare record
- `--preview`: opt in to shortening oversized string fields and wrapping the record as `{ "preview": ..., "record": ... }` (JSON, JSONL, TOON, text, and auto output only)
- `--full`: explicit lossless escape hatch for commands copied from preview workflows; conflicts with `--preview`

`table get` is lossless by default. With `--preview`, preview-capable formats wrap a shortened record as `{ "preview": ..., "record": ... }`. Replace `--preview` with `--full`, or omit `--preview`, to return the record as the exact Table API value. `--preview` is rejected with CSV output.

A name lookup is one Table API read. No match fails with `RECORD_NOT_FOUND`
(exit 4, like a missing `sys_id`). Several matches fail with
`VALIDATION_ERROR` (exit 2) and list up to five candidates' `sys_id` and scope
in `issues`; pass one of those `sys_id` values instead.

Examples:

```bash
snow-cli table get incident 46d44a4b2f13000044e0bfc8fb99b6fd --fields number,short_description,state
snow-cli table get incident INC0010001 --display-value both
snow-cli table get sys_script_include ChangeRequestStateHandler --fields name,api_name,script
snow-cli table get sys_script_include ChangeRequestStateHandler --fields script --raw
```

`--raw` output can be piped straight into line tools, for example
`snow-cli table get sys_script_include MyUtil --fields script --raw | grep -n initialize`.
For script includes, [`include show`](/commands/include/) adds the parsed
`extends` chain and client-side `--grep`.

## Display values

By default the Table API returns stored values: `priority` is `"4"`, `state`
is `"1"`, and references are plain `sys_id` strings (`table list` and
`table get` send `sysparm_exclude_reference_link=true` by default; pass
`--reference-links` to opt back in to ServiceNow's `{"link","value"}` objects).
`--display-value` on `table list`, `table get`, and `table stats` asks
ServiceNow for labels instead:

| Mode | Sent | Field value |
|---|---|---|
| (flag absent) | `sysparm_exclude_reference_link=true` (on `list`/`get`) | stored value; plain string references |
| `raw` | `sysparm_display_value=false` | stored value |
| `display` (bare flag) | `sysparm_display_value=true` | label, such as `"4 - Low"` |
| `both` | `sysparm_display_value=all` | `{"value":"4","display":"4 - Low"}` where they differ; a plain string where they are equal |

`display` renders dates in the session user's timezone and format
(`sys_created_on` may differ by hours from the stored UTC value); use `both`
when you need raw values as well. In CSV and TOON output, `both` adds a
`<field>.display` column next to `<field>` instead of a nested object. The list
envelope (`total`, `returned`, `offset`, `next_offset`, `truncated`, `records`)
is the same in every mode.

## Unknown-field warnings

ServiceNow silently drops unknown or unreadable names from `sysparm_fields`
and silently ignores encoded-query terms on them, so
`--query 'active=true^assigned_two.name=Abel Tuter'` returns the unfiltered
`active=true` result. `table list` checks the names in `--fields`, `--query`,
and `--order-by`, and `table stats` the names in `--query` and `--group-by`:

- `warn` (default): each missing name is reported under `warnings` in the
  list envelope (and the JSONL meta line), and as one
  `{"warning":{...}}` JSON line on stderr. CSV output carries it on stderr only.
- `error`: the command fails with `VALIDATION_ERROR` (exit 2) and one
  `unknown_field` issue per name. For `table list` the main query is not sent.
- `off`: no check.

```json
"warnings":[{"code":"UNKNOWN_FIELD","field":"assigned_two.name","in":"query","message":"incident returned no field 'assigned_two.name' (unknown or not readable) from --query; ServiceNow ignores that term, so the result is NOT filtered by it"}]
```

The check is cheap. For `table list` the query and sort fields are added to
the request's field list and removed from the output again, so a non-empty
page costs no extra request. An empty page whose query or sort names a field
costs one `sysparm_limit=1` read without the query. `table stats` always needs
that one read when `--query` or `--group-by` is set. If that read fails, or the
table has no readable row, the names stay unverified and the command continues.
Fields hidden by ACLs are reported too, because ServiceNow ignores them in the
same way. `--fields '*'` (no projection) verifies only the first segment of
dot-walked names. The `warnings` key is absent when there is nothing to report.

## `table create <table>`

Create a new record.

```bash
snow-cli table create <table> --data '{"field":"value"}'
```

Important options:

- `--data <json>`: JSON object to send to the Table API

If `--data` is omitted and stdin is piped in, the command reads JSON from stdin.

Examples:

```bash
snow-cli table create incident --data '{"short_description":"VPN down"}'
echo '{"short_description":"Created from stdin"}' | snow-cli table create incident
```

## `table update <table> <sys_id>`

Patch an existing record.

```bash
snow-cli table update <table> <sys_id> --data '{"field":"value"}'
```

Important options:

- `--data <json>`: JSON object with fields to change

If `--data` is omitted and stdin is piped in, the command reads JSON from stdin.

Example:

```bash
snow-cli table update incident 46d44a4b2f13000044e0bfc8fb99b6fd --data '{"state":"2"}'
```

## `table delete <table> <sys_id>`

Delete a record.

```bash
snow-cli table delete <table> <sys_id> [--yes] [--ignore-not-found]
```

Important options:

- `--yes`: skip the confirmation prompt
- `--ignore-not-found`: converge to exit `0` with `"outcome": "already_absent"` when ServiceNow returns a verified HTTP `404` `RECORD_NOT_FOUND` (`"No Record found"`) error envelope

Output:

`table delete` emits a structured result through the configured `--output` format (`json`, `text`, `csv`, `jsonl`, `toon`, or `auto`):

```json
{"status":"success","outcome":"deleted","table":"incident","sys_id":"46d44a4b2f13000044e0bfc8fb99b6fd","message":"Record 46d44a4b2f13000044e0bfc8fb99b6fd deleted from incident"}
```

When `--ignore-not-found` converges on an already-absent record:

```json
{"status":"success","outcome":"already_absent","table":"incident","sys_id":"46d44a4b2f13000044e0bfc8fb99b6fd","message":"Record 46d44a4b2f13000044e0bfc8fb99b6fd already absent in incident"}
```

Notes:

- In an interactive shell, the command asks for confirmation unless `--yes` is used.
- In non-interactive environments, use `--yes` explicitly (including when `--ignore-not-found` is set).
- Read-only enforcement (`snow-cli --read-only` and `snow-cli-ro`) always rejects `table delete`, even when `--ignore-not-found` is passed.
- The command sends a single `DELETE /api/now/table/{table}/{sys_id}` request without a preflight `GET`.
- By default (`--ignore-not-found` omitted), deleting an absent record fails with exit `4` (`RECORD_NOT_FOUND`).
- With `--ignore-not-found`, only a verified ServiceNow HTTP `404` `RECORD_NOT_FOUND` (`"No Record found"`) response converges to exit `0`. Wrong tables (`400 INVALID_TABLE`), generic proxy or wrong-path `404`s (`404 NOT_FOUND`), ACL/permission denials (`403 ACL_DENIED` / `FORBIDDEN`), authentication errors (`401`), and `5xx` or transport failures still fail non-zero.
- ServiceNow returns `"No Record found"` both when a record does not exist and when ACLs or domain separation hide it, which is why `--ignore-not-found` is strictly opt-in.
- Convergent `--ignore-not-found` semantics apply only to `table delete`. Create (`table create`), update (`table update`), import (`data import`, `import-set`), and script (`script run`) mutations are **not** made blindly idempotent.

Examples:

```bash
snow-cli table delete incident 46d44a4b2f13000044e0bfc8fb99b6fd --yes
snow-cli table delete incident 46d44a4b2f13000044e0bfc8fb99b6fd --yes --ignore-not-found
```

## `table schema <table>`

Inspect table columns using `sys_dictionary`. By default the output is the
table's effective schema: columns defined on the table plus those inherited
from its parent tables (for example `incident` inherits `short_description`,
`priority`, and `state` from `task`). Each column carries a `table` field naming
the table that defines it; when a child table overrides a parent column, only
the child's definition is shown.

```bash
snow-cli table schema <table> [options]
```

Important options:

- `--extended`: include metadata such as required, read-only, max length, default, and reference table
- `--own-only`: only show columns defined on the table itself, without inherited columns

Examples:

```bash
snow-cli table schema incident
snow-cli table schema incident --extended
snow-cli table schema incident --own-only
```

This is especially useful before building imports, exports, or scripted automation.

## `table find <text>`

Find tables whose name or label contains `<text>`. Use it when you do not know
a table's exact name.

```bash
snow-cli table find flow
snow-cli table find 'service catalog' --limit 50
```

Each row has `name`, `label`, `extends` (the parent table, or `null` for a
base table), and `scope`. Rows are sorted by name and use the `table list`
envelope (`total`, `returned`, `offset`, `next_offset`, `truncated`,
`records`). Output is bounded to 20 tables; use `--limit`, `--offset`, or
`--all`. Query operators such as `^` and `=` are rejected in `<text>`.

## `table choices <table> [field]`

List active choice values and their labels, so codes such as `state: "2"` can
be read as `In Progress`.

```bash
snow-cli table choices incident priority
snow-cli table choices incident
snow-cli table choices change_request state --language de
```

Choice lists are often defined on a parent table: `incident.priority` choices
live on `task`. The command walks the table hierarchy and, for each field,
returns the choices of the most specific table that defines any. Each row has
`element`, `value`, `label`, `sequence`, `table` (where the choices are
defined), and `dependent_value` when set. `--language` defaults to `en`.

## `table stats <table>`

Count and aggregate records using the ServiceNow Aggregate API
(`GET /api/now/stats/{table}`).

```bash
snow-cli table stats <table> [options]
```

Important options:

- `--query <encoded-query>`: ServiceNow encoded query string
- `--group-by <a,b>`: comma-separated fields to group by (one result row per group)
- `--avg <a,b>` / `--min <a,b>` / `--max <a,b>` / `--sum <a,b>`: per-field aggregates
- `--having <clause>`: keep only groups whose aggregate matches, such as `count>5` or
  `avg(priority)>=2`. Separate clauses with commas to require all of them.
- `--display-value [raw|display|both]`: return labels instead of stored codes/sys_ids for grouped fields (bare flag means `display`). See [Display values](#display-values)
- `--check-fields warn|error|off`: verify `--query` and `--group-by` field names with one extra row read (see [Unknown-field warnings](#unknown-field-warnings)); warnings go to stderr

Without aggregate flags, the command returns the matching record count — a
cheap way to answer "how many rows match?" without fetching records:

```bash
snow-cli table stats incident --query 'active=true'
```

With `--group-by`, one row per group is returned, including the count and any
requested aggregates (flattened as `avg_<field>`, `min_<field>`, and so on):

```bash
snow-cli table stats incident --group-by state --avg priority
```

Notes:

- Numeric strings in the response are converted to numbers where they parse cleanly.
- Group values (like `state`) stay strings because they are categorical codes.
- `table stats` is available in `snow-cli-ro` because it only reads data.
- `--having` translates `count>5` into the Aggregate API's native
  `count^*^>^5` form and `avg(priority)>=2` into `avg^priority^>=^2`.
  Aggregates are `count`, `avg`, `min`, `max`, and `sum`; operators are `>`,
  `>=`, `<`, `<=`, `=`, and `!=`. A clause that already contains `^` is sent
  unchanged.

## Common examples

```bash
snow-cli table list incident --query 'priority=1^active=true'
snow-cli table get sys_user <sys_id>
snow-cli table create cmdb_ci --data '{"name":"router-01"}'
snow-cli table update incident <sys_id> --data '{"assigned_to":"6816f79cc0a8016401c5a33be04be441"}'
snow-cli table delete incident <sys_id> --yes
```

### Real-world workflow

```bash
# 1. List records with just a few fields
snow-cli table list incident --limit 5 --fields number,short_description,priority --output csv

# 2. Inspect available columns for the table
snow-cli table schema incident --extended

# 3. Get full details on one record
snow-cli table get incident <sys_id> --fields number,short_description,state,assignment_group

# 4. Create a new record
snow-cli table create incident --data '{"short_description":"Disk space low on server-01","category":"hardware","urgency":"2"}'

# 5. Update it
snow-cli table update incident <sys_id> --data '{"state":"2","assigned_to":"<user_sys_id>"}'

# 6. Clean up
snow-cli table delete incident <sys_id> --yes
```

### JSON quoting in the shell

The `--data` value must be valid JSON, and the shell decides what actually
reaches the CLI. Two forms work:

```bash
# Recommended — single quotes, simplest to read and least escaping
snow-cli table create incident --data '{"short_description":"VPN issue"}'

# Also correct — double quotes with escaped inner quotes; the shell
# unescapes \" back to " before the CLI ever sees the argument
snow-cli table create incident --data "{\"short_description\":\"VPN issue\"}"
```

What actually breaks is leaving `--data` unquoted. The shell then splits on
spaces and strips the JSON's own quote characters, so the CLI receives a
mangled argument instead of one JSON object:

```bash
# Wrong — unquoted; the shell tokenizes this into multiple words
snow-cli table create incident --data {"short_description":"VPN issue"}
```

Prefer single quotes for everyday use — they need no escaping — and reach for
the double-quoted form only when the value must interpolate a shell variable.

## Recipes for agents

`snow-cli table --help` ends with these recipes:

```bash
snow-cli table list incident --fields number,priority,state --display-value
snow-cli table list incident --order-by -sys_created_on --limit 10 --offset 10
snow-cli table list incident --query 'active=true^assigned_to.name=Abel Tuter' --check-fields error
snow-cli include show ChangeRequestStateHandler --chain --grep 'canMoveTo' --context 10
snow-cli table get sys_script_include ChangeRequestStateHandler --fields script --raw
```

## Related pages

- [`include` command reference](/commands/include/)

- [`data` command reference](/commands/data/)
- [`attachment` command reference](/commands/attachment/)
- [`api` command reference](/commands/api/)

## Domain separation

Use `--domain global|<domain_sys_id>` on `table list` and `table stats` to filter
exact record ownership within server visibility. On `table list` and `table get`,
`--query-no-domain` explicitly requests the privileged Table API option. It is
never enabled automatically. `--with-domain` adds each record's domain name and,
for lists, a per-domain `domains_in_page` summary. See the [domain guide](/commands/domain/) for
inherited-field validation, query branching, role requirements, and how to
interpret empty results.
