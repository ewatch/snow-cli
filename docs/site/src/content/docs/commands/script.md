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
- `--sandbox`: prevent database writes
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
> multiline scripts must run **without** `--sandbox`. Also note that script
> errors currently do not affect the CLI's exit code (it exits 0 even when the
> server-side script fails to parse or execute), so always check the printed
> output rather than relying on the exit code alone.

Examples:

```bash
snow-cli script run --code 'gs.info("hello")' --scope x_my_app
snow-cli script run --file ./job.js --sandbox
snow-cli script run --code 'gs.sleep(1000); gs.info("done")' --quota-managed-transaction
```

### Hints from live E2E testing

- For safe live verification on a real instance, start with `--sandbox` so you can confirm auth, form bootstrap, and script execution without writing records.
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
