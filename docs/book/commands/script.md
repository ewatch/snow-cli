# `script`

Use `script` to execute a ServiceNow background script.

```bash
snow-cli script run [options]
```

All `script` subcommands also accept the global flags from the [command overview](../commands.md).

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
- `--scope <scope>`: scope to run in, default `global`
- `--endpoint <path>`: execution endpoint, default `/sys.scripts.do`
- `--rollback`: record rollback context for database changes
- `--sandbox`: prevent database writes
- `--scriptlet`: run as a scriptlet with access to global server-side objects
- `--quota-managed-transaction`: use managed transaction limits for long-running scripts

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

Example multiline stdin run:

```bash
cat <<'EOF' | snow-cli script run --sandbox
var user = gs.getUserName();
gs.print('start');
gs.print('user=' + user);
gs.print('end');
EOF
```

## When to use `script`

Use this command when you need to:

- run a quick background script,
- inspect data from server-side APIs,
- perform one-off maintenance,
- prototype logic before turning it into an app artifact.

For raw REST endpoints instead of background scripts, use [`api`](./api.md).

## Related pages

- [`api` command reference](./api.md)
- [`scope` command reference](./scope.md)
