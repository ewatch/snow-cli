# `import-set`

Use `import-set` to load records into staging tables through the Import Set API.

```bash
snow-cli import-set <verb> [options]
```

All `import-set` subcommands also accept the global flags from the [command overview](/commands/).

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
snow-cli import-set load imp_user --data '{"user_id":"jdoe","email":"jdoe@example.com","first_name":"John","last_name":"Doe"}'
echo '{"user_id":"asmith","email":"asmith@example.com","first_name":"Alice","last_name":"Smith"}' | snow-cli import-set load imp_user
snow-cli import-set load imp_user --fail-on-error --data '{"user_id":"ci-user","email":"ci-user@example.com","first_name":"CI","last_name":"User"}'
```

> **Warning: payload keys must match the transform map's source fields.**
> `imp_user` is a staging table — its columns don't have to match the target
> table, they have to match whatever the transform map that runs on load
> expects. For the stock "User" transform map (`imp_user` → `sys_user`), the
> source fields are `user_id`, `email`, `first_name`, `last_name`,
> `department`, `phone`, and `location`, and the map **coalesces on `email`**.
> A payload built around `user_name` instead of `user_id`/`email` will still
> load into the staging table and still return exit code 0, but the transform
> either creates a garbage `sys_user` record or fails outright with an error
> like `Error during insert of sys_user (); Target record not found` — and the
> CLI does not surface that as a command failure by default. Always check the
> printed summary (or pass `--fail-on-error`) rather than trusting a zero exit
> code alone.

Notes:

- If `--data` is omitted and stdin is piped in, the command reads JSON from stdin.
- The request is sent to `/api/now/import/{table}`.
- **The transform runs automatically on load.** `/api/now/import/{table}` both stages the row and runs the staging table's transform map in one call, so a successful load already inserts/updates the target record — there is no separate transform step to invoke.
- The command prints a structured summary with counts for inserted, updated, ignored, error, and other result rows.
- `--fail-on-error` is useful for CI or agent workflows where row-level failures must fail the command — without it, `import-set load` can exit 0 even though `summary.error` is non-zero.

## Related pages

- [`data` command reference](/commands/data/)
