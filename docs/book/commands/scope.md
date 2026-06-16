# `scope`

Use `scope` to inspect ServiceNow application scope metadata and work with scope-related artifacts.

```bash
snow-cli scope <verb> [options]
```

All `scope` subcommands also accept the global flags from the [command overview](../commands.md).

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

Examples:

```bash
snow-cli scope inspect x_my_app
snow-cli scope inspect 4f7f9bfe1b2a9010d9f2ed7c2e4bcb12 --details full
```

Use `basic` when you only need counts and summary data. Use `full` when you want normalized artifact rows in the response.

**Performance note:** `scope inspect global` or other large platform scopes
may time out because they contain thousands of artifacts. Prefer inspecting
a specific custom scope (like `x_my_app`) or use `--details basic` on large
scopes.

## `scope inventory <scope>`

Export normalized artifact rows for a scope.

```bash
snow-cli scope inventory <scope>
```

This is useful for:

- machine-readable analysis,
- CSV export,
- comparing application contents,
- downstream scripting.

Example:

```bash
snow-cli --output csv scope inventory x_my_app
```

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

- [`script` command reference](./script.md)
