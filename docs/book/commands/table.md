# `table`

Use `table` for generic CRUD operations against ServiceNow tables.

```bash
snow-cli table <verb> [options]
```

All `table` subcommands also accept the global flags from the [command overview](../commands.md).

## `table list <table>`

List records from a table.

```bash
snow-cli table list <table> [options]
```

Important options:

- `--query <encoded-query>`: ServiceNow encoded query string
- `--fields <a,b,c>`: comma-separated field list
- `--limit <n>`: maximum number of records to return
- `--order-by <field>`: sort by a field

Examples:

```bash
snow-cli table list incident --query 'active=true' --limit 20
snow-cli table list sys_user --fields sys_id,user_name,email --order-by user_name
```

Notes:

- `table list` auto-paginates until it reaches the requested limit or exhausts the result set.
- Use `--output csv` when you want tabular export.

## `table get <table> <sys_id>`

Fetch a single record.

```bash
snow-cli table get <table> <sys_id> [options]
```

Important options:

- `--fields <a,b,c>`: restrict the returned fields

Example:

```bash
snow-cli table get incident 46d44a4b2f13000044e0bfc8fb99b6fd --fields number,short_description,state
```

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
snow-cli table delete <table> <sys_id> [--yes]
```

Important options:

- `--yes`: skip the confirmation prompt

Notes:

- In an interactive shell, the command asks for confirmation unless `--yes` is used.
- In non-interactive environments, use `--yes` explicitly.

Example:

```bash
snow-cli table delete incident 46d44a4b2f13000044e0bfc8fb99b6fd --yes
```

## `table schema <table>`

Inspect table columns using `sys_dictionary`.

```bash
snow-cli table schema <table> [options]
```

Important options:

- `--extended`: include metadata such as required, read-only, max length, default, and reference table
- `--include-inherited`: include fields inherited from parent tables

Examples:

```bash
snow-cli table schema incident
snow-cli table schema incident --extended
snow-cli table schema incident --extended --include-inherited
```

This is especially useful before building imports, exports, or scripted automation.

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

The `--data` value must be valid JSON. When using single quotes on the command
line, the shell passes the string verbatim to the CLI. Double quotes inside
the JSON must not be escaped:

```bash
# Correct
snow-cli table create incident --data '{"short_description":"VPN issue"}'

# Wrong — inner escaped quotes are not valid JSON
snow-cli table create incident --data "{\"short_description\":\"VPN issue\"}"
```

## Related pages

- [`data` command reference](./data.md)
- [`attachment` command reference](./attachment.md)
- [`api` command reference](./api.md)
