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
- `--fail-on-error`: compatibility flag; row-level errors now fail by default
- `--allow-row-errors`: explicitly accept partial failures, exiting 0 with a stderr
  warning; conflicts with `--fail-on-error`

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
> load into the staging table, but the transform may create an incorrect
> `sys_user` record or fail with an error such as
> `Error during insert of sys_user (); Target record not found`. Reported
> row errors now fail the command by default, but a successful transform does
> not prove that the payload matched your business intent. Inspect the summary
> and target records as appropriate.

Notes:

- If `--data` is omitted and stdin is piped in, the command reads JSON from stdin.
- The request is sent to `/api/now/import/{table}`.
- **The transform runs automatically on load.** `/api/now/import/{table}` both stages the row and runs the staging table's transform map in one call, so a successful load already inserts/updates the target record — there is no separate transform step to invoke.
- The command prints a structured summary with counts for inserted, updated, ignored, error, and other result rows.
- Supported output formats are JSON, JSONL, TOON, text, and auto. CSV is rejected
  before reading input or sending a request, with `VALIDATION_ERROR` and exit **2**.
  Use `--output json` to override a CSV environment/config default.
- Row-level errors exit **1** by default with a JSON stderr error code
  `IMPORT_SET_ROW_ERRORS`. The detailed result and summary are still printed on
  stdout, including for partial failures. Error classification uses normalized
  `status: error` or a nonempty `error_message`; rows are counted once.
- `--allow-row-errors` restores intentional partial-success behavior: exit 0
  with a warning on stderr. `--fail-on-error` remains accepted for compatibility
  and cannot be combined with this opt-out.
- This is a change from the former default, which exited 0 on row failures.
  Existing automation that intentionally accepts partial failures must opt in.
  Do not blindly rerun a failed load: other rows may already have changed, and
  a nonzero exit does not roll back the transform. Inspect the existing stdout
  report first; no repeat HTTP request is needed to see the failure details.

## Related pages

- [`data` command reference](/commands/data/)
