# `scope`

Use `scope` to inspect ServiceNow application scope metadata and work with scope-related artifacts.

```bash
snow-cli scope <verb> [options]
```

All `scope` subcommands also accept the global flags from the [command overview](/commands/).

## `scope list [search]`

List scopes and classify them by origin.

```bash
snow-cli scope list
snow-cli scope list incident
```

Important options:

- `[search]`: optional search term for partial matches or exact scope names
- `--kind <kind>`: filter by one or more kinds
- `--show-source-table`: include the source table column in text output
- `--show-sys-id`: include the sys_id column in text output

Supported `--kind` values:

- `store-app`
- `plugin`
- `custom-app`
- `platform`
- `platform-app`

Examples:

```bash
snow-cli scope list
snow-cli scope list sn_ot_incident_mgmt
snow-cli scope list incident --kind plugin --kind store-app
```

Note: on PDIs, `scope list` may warn that `sys_store_app` (HTTP 403) and
`v_plugin` (HTTP 500) cannot be queried. The command still succeeds using the
remaining classification sources — the warnings just mean that particular
origin data is incomplete for those two kinds.

## `scope inspect <scope>`

Inspect a scope and summarize its metadata.

```bash
snow-cli scope inspect <scope> [options]
```

`<scope>` can be either:

- a scope name such as `x_my_app`, or
- a scope `sys_id`.

Important options:

- `--details <basic|full>`: choose whether to include the full artifact list
- `--limit <n>`: cap the records enumerated per table when `--details full`
  lists artifacts. Required for the `global` scope (see below).

Examples:

```bash
snow-cli scope inspect x_my_app
snow-cli scope inspect global
snow-cli scope inspect 4f7f9bfe1b2a9010d9f2ed7c2e4bcb12 --details full --limit 1000
```

Use `basic` when you only need counts and summary data. Use `full` when you want normalized artifact rows in the response.

**How counts are gathered:** `--details basic` tallies each artifact table
using the Table API's `X-Total-Count` header — one lightweight request per
table — instead of downloading every record. This makes `basic` inspection
fast and reliable even for the `global` scope. Counts trust the server-side
`sys_scope` filter and are not re-validated row by row, so they can differ
slightly from a full enumeration if a table ignores the filter.

**Platform (`global`) scope:** `scope inspect global --details basic` is
supported and cheap, but dictionary and choice counts are skipped there (they
would require enumerating every table in the instance) and reported as a
warning. `--details full` on `global` enumerates artifacts and therefore
**requires `--limit <n>`**, which caps records fetched per table. Any table
truncated at the cap is reported in the `warnings` list.

## `scope inventory <scope>`

Export normalized artifact rows for a scope.

```bash
snow-cli scope inventory <scope> [--limit <n>]
```

This is useful for:

- machine-readable analysis,
- CSV export,
- comparing application contents,
- downstream scripting.

Important options:

- `--limit <n>`: cap the records enumerated per table. Without it, a default
  per-table cap is applied and truncation is surfaced as a warning; enumerating
  the `global` scope **requires** an explicit `--limit`.

Examples:

```bash
snow-cli --output csv scope inventory x_my_app
snow-cli scope inventory global --limit 500
```

**Platform (`global`) scope:** `global`'s artifacts span the entire base
instance, so `scope inventory global` refuses to run unbounded. Pass
`--limit <n>` to cap records per table. Tables truncated at the cap are listed
in `warnings`.

## `scope move-file <table> <sys_id> --target-scope <scope>`

Move one application file into another custom scope without changing its `sys_id`.

```bash
snow-cli scope move-file <table> <sys_id> --target-scope <scope> [options]
```

Important options:

- `--target-scope <scope>`: required target scope name or scope `sys_id`
- `--dry-run`: validate and preview the move without persisting it
- `--yes`: confirm execution when warnings are reported

Examples:

```bash
snow-cli scope move-file sys_script_include 4f7f9bfe1b2a9010d9f2ed7c2e4bcb12 \
  --target-scope x_target_app --dry-run

snow-cli scope move-file sys_script_include 4f7f9bfe1b2a9010d9f2ed7c2e4bcb12 \
  --target-scope x_target_app --yes
```

Notes:

- `scope move-file` runs a background script behind the scenes.
- `--dry-run` is the safest way to see warnings and proposed field changes before you commit the move.

## Related pages

- [`script` command reference](/commands/script/)
