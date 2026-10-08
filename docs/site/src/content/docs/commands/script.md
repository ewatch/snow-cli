# `script`

Use `script` to execute a ServiceNow background script.

```bash
snow-cli script run [options]
```

All `script` subcommands also accept the global flags from the [command overview](/commands/).

## `script run`

Execute a background script on the target instance.

```bash
snow-cli script run [options]
```

### How to provide the script

You can provide script content in three ways:

- `--code <script>`: inline code
- `--file <path>`: read from a local file
- stdin: pipe the script into the command

Precedence is:

1. `--code`
2. `--file`
3. stdin

Examples:

```bash
snow-cli script run --code 'gs.info("hello from snow-cli")'
snow-cli script run --file ./cleanup.js
printf '%s' 'gs.info("from stdin")' | snow-cli script run
```

### Important options

- `-c, --code <script>`: inline script text
- `-f, --file <path>`: local script file
- `--scope <scope>`: scope to run in. Accepts `global`, a scope name such as
  `x_my_app`, or a scope sys_id; default `global`. For the Script Background
  form, named scopes are resolved to their sys_id before execution.
- `--endpoint <path>`: execution endpoint, default `/sys.scripts.do`
- `--rollback`: record rollback context for database changes
- `--sandbox`: use the restricted KittyScript evaluator (single expression only;
  not a general read-only mode for arbitrary scripts)
- `--scriptlet`: run as a scriptlet with access to global server-side objects
- `--quota-managed-transaction`: use managed transaction limits for long-running scripts

### Script Background form options

When using the default `/sys.scripts.do` endpoint, these flags correspond to
the checkboxes on the ServiceNow Script Background form:

| Form checkbox | CLI flag | CLI default | Form default |
| --- | --- | --- | --- |
| Record for rollback? | `--rollback` | Disabled | Enabled |
| Execute in sandbox? | `--sandbox` | Disabled | Disabled |
| Execute as scriptlet? | `--scriptlet` | Disabled | Disabled |
| Cancel after 4 hours | `--quota-managed-transaction` | Disabled | Enabled |

The CLI intentionally requires opt-in for rollback recording and the managed
transaction limit, even though the browser form enables those options by
default. Pass the corresponding flag only when you need that behavior.

> **`--sandbox` accepts only a single expression.** It runs your script
> through ServiceNow's restricted KittyScript evaluator, which rejects
> anything beyond one expression — even a simple sequence like
> `gs.print('a'); gs.print('b');` fails validation. Multi-statement or
> multiline scripts cannot use this evaluator. Removing `--sandbox` changes
> the safety model: ordinary background scripts can write to the database.
> Do not automatically retry a failed script without reviewing its effects.

### Output

For `/sys.scripts.do`, JSON, JSONL, TOON, and auto output use a structured
execution result rather than the raw server log:

```json
{"output":["a","b"],"duration_ms":6,"scope":"global","history_sys_id":"0123456789abcdef0123456789abcdef","errors":[]}
```

`output` contains script-printed lines without the `*** Script:` prefix;
printed JSON and XML remain strings. Unknown log lines are retained. Timing and
history metadata are parsed only from server log lines, not printed content.
Missing or unrecognized duration/history values are `null`; scope falls back
to the requested scope when the log does not report it.

`--output text` preserves the cleaned execution log, including timing and
history-link lines. For compatibility, `--output csv` also remains a raw-log
passthrough for this command—it is not a tabular CSV result. Custom
`--endpoint` responses keep their existing JSON/raw-response behavior rather
than being interpreted as Script Background logs.

`errors` is empty on successful responses. Known execution failures do **not**
produce a success envelope with an errors list: they retain the stderr-only,
nonzero-exit contract below. Successful output may contain arbitrary script
content; do not print secrets or treat result data as automatically redacted.

### Execution failures

For `/sys.scripts.do`, recognized sandbox-validation, compilation, evaluation,
background-error, and Java/Rhino exception diagnostics cause exit code **1**.
The CLI emits a single JSON error line on stderr with code
`SCRIPT_EXECUTION_FAILED` and a safe classification message. It emits no result
on stdout and does not expose the script, server error text, or stack trace—even
with sensitive HTTP debugging enabled. Any earlier script side effects may
already have occurred; failure does not imply rollback.

Detection uses known ServiceNow diagnostic markers, not arbitrary occurrences
of the word `error`. Normal `*** Script:` print lines and JSON result values
are treated as data. Response layouts vary by release, so exit 0 means no known
failure diagnostic was detected, not proof that the script achieved its intended
business outcome. Custom `--endpoint` responses retain their existing passthrough
behavior; this evaluator-specific detection does not apply to them.

Examples:

```bash
snow-cli script run --code 'gs.info("hello")' --scope x_my_app
snow-cli script run --sandbox --code 'gs.print(1)'
snow-cli script run --code 'gs.sleep(1000); gs.info("done")' --quota-managed-transaction
```

### Hints from live E2E testing

- For a minimal live check, use `--sandbox --code 'gs.print(1)'` to verify auth, form bootstrap, and expression execution. Do not treat `--sandbox` as a general read-only guarantee for arbitrary scripts.
- All three input modes were validated end to end: `--code`, `--file`, and stdin.
- Multiline scripts also worked from both `--file` and stdin.
- Some ServiceNow instances use older background script JavaScript parsing. If the instance reports an older script engine level (for example `Script ES Level: 0`), wrapper syntax such as IIFEs may fail with errors like `Invalid function definition` even though simpler multiline scripts work.
- If you hit parser compatibility issues, prefer plain top-level statements over wrapper patterns.

Example multiline stdin run. This runs without `--sandbox`, because the
sandboxed evaluator only accepts a single expression — a multi-statement
script like this one fails under `--sandbox` (see the note above), even
though it runs verbatim without the flag and prints `start` / `user=...` /
`end`:

```bash
cat <<'EOF' | snow-cli script run
var user = gs.getUserName();
gs.print('start');
gs.print('user=' + user);
gs.print('end');
EOF
```

If you want a `--sandbox` example instead, keep it to a single expression:

```bash
snow-cli script run --sandbox --code "gs.print('user=' + gs.getUserName())"
```

## When to use `script`

Use this command when you need to:

- run a quick background script,
- inspect data from server-side APIs,
- perform one-off maintenance,
- prototype logic before turning it into an app artifact.

For raw REST endpoints instead of background scripts, use [`api`](/commands/api/).
For browser-helper actions through SN-Utils, use [`snu`](/commands/snu/).

## Related pages

- [`api` command reference](/commands/api/)
- [`scope` command reference](/commands/scope/)
