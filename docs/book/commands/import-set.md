# `import-set`

Use `import-set` to load records into staging tables through the Import Set API.

```bash
snow-cli import-set <verb> [options]
```

All `import-set` subcommands also accept the global flags from the [command overview](../commands.md).

## `import-set load <table>`

Post one JSON object into a staging table.

```bash
snow-cli import-set load <table> [options]
```

Important options:

- `--data <json>`: JSON object to load
- `--fail-on-error`: exit non-zero when the response contains row-level errors

Examples:

```bash
snow-cli import-set load imp_user --data '{"user_name":"snow-cli-user","email":"snow-cli-user@example.com"}'
echo '{"user_name":"stdin-user","email":"stdin-user@example.com"}' | snow-cli import-set load imp_user
snow-cli import-set load imp_user --fail-on-error --data '{"user_name":"ci-user","email":"ci-user@example.com"}'
```

Notes:

- If `--data` is omitted and stdin is piped in, the command reads JSON from stdin.
- The request is sent to `/api/now/import/{table}`.
- The command prints a structured summary with counts for inserted, updated, ignored, error, and other result rows.
- `--fail-on-error` is useful for CI or agent workflows where row-level failures must fail the command.

## `import-set transform <sys_id>`

Reserved for separate transform execution.

```bash
snow-cli import-set transform <sys_id>
```

Current status:

- the command surface exists,
- the implementation is still a placeholder,
- some instances already run the transform automatically during `import-set load`.

In other words, `import-set load` is the working path today.

## Related pages

- [`data` command reference](./data.md)
