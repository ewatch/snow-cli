# `codesearch`

Use `codesearch` to search code and related metadata on a ServiceNow instance.

```bash
snow-cli codesearch search <query> [options]
```

All `codesearch` subcommands also accept the global flags from the [command overview](/commands/).

## `codesearch search <query>`

Search instance code for a text query.

```bash
snow-cli codesearch search <query> [options]
```

Important options:

- `--source-table <table>`: limit results to a specific source table such as `sys_script_include` or `sys_script`
- `--scope <app>`: search only the named application scope (for example, `x_my_app` or `global`)
- `--limit <n>`: maximum hits per source table, default `10`
- `--current-scope`: search only in the current scope
- `--search-group <name>`: advanced search-group override

Examples:

```bash
snow-cli codesearch search GlideRecord
snow-cli codesearch search GlideRecord --source-table sys_script_include
snow-cli codesearch search gs.info --current-scope
snow-cli codesearch search GlideRecord --scope x_my_app
snow-cli codesearch search BusinessRule --limit 250
```

Notes:

- The default search group is `sn_devstudio.Studio Search Group`.
- For grouped responses, empty source-table groups are omitted. Structured output includes `searched_tables` so the number of tables checked remains visible. CSV flattens the hits and includes a `recordType` column.

## Related pages

- [`scope` command reference](/commands/scope/)
- [`api` command reference](/commands/api/)
