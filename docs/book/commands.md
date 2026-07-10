# Command reference

`snow-cli` uses a noun-verb command style:

```bash
snow-cli <noun> <verb> [options]
```

Examples:

```bash
snow-cli profile add dev --instance https://dev.service-now.com --auth-method basic --username admin
snow-cli auth login
snow-cli table list incident --query 'active=true' --limit 20
```

Use the pages in this section for command-specific guidance. The built-in help remains the authoritative source for exact usage:

```bash
snow-cli --help
snow-cli <noun> --help
snow-cli <noun> <verb> --help
```

## Global flags

Every command supports these top-level flags:

- `--profile <name>`: use a specific saved profile
- `--instance <url>`: temporarily override the instance URL from the active profile
- `--output <json|csv|jsonl|toon|text|auto>`: choose the stdout format; when omitted, `SNOW_CLI_OUTPUT` and then `profile output` are consulted before falling back to JSON
- `--timeout-secs <seconds>`: override the HTTP timeout for the current command
- `--read-only`: block commands and HTTP methods that can mutate ServiceNow
- `-v`, `-vv`, `-vvv`: increase log verbosity on stderr

## Command pages

| Command | What it is for |
|---|---|
| [`profile`](./commands/profile.md) | Create, edit, inspect, and switch connection profiles |
| [`auth`](./commands/auth.md) | Log in, log out, inspect auth status, and print tokens |
| [`table`](./commands/table.md) | CRUD operations and schema inspection for ServiceNow tables |
| [`data`](./commands/data.md) | Export, validate, and import data artifacts |
| [`seed`](./commands/seed.md) | Planned test-data workflows |
| [`scope`](./commands/scope.md) | List scopes, inspect scopes, export inventory, and move files between scopes |
| [`attachment`](./commands/attachment.md) | List, download, and upload attachments |
| [`import-set`](./commands/import-set.md) | Load records into staging tables |
| [`api`](./commands/api.md) | Send raw REST requests to arbitrary endpoints |
| [`script`](./commands/script.md) | Run background scripts |
| [`snu`](./commands/snu.md) | Drive the SN-Utils browser helper tab |
| [`codesearch`](./commands/codesearch.md) | Search code and metadata on an instance |
| [`skill`](./commands/skill.md) | Install agent skills from local bundles or URL-hosted manifests |
| [`completions`](./commands/completions.md) | Generate shell completion scripts |

## Common patterns

### Use a saved profile

```bash
snow-cli --profile prod table list incident --limit 10
```

### Pipe JSON into commands that accept stdin

```bash
echo '{"short_description":"Created from stdin"}' | snow-cli table create incident
echo '{"user_name":"import-user"}' | snow-cli import-set load imp_user
```

### Ask the CLI for help at the exact level you need

```bash
snow-cli auth --help
snow-cli auth login --help
snow-cli scope move-file --help
```

### Handle slow or hibernating instances

```bash
snow-cli --timeout-secs 60 table list incident --limit 5
```

The default timeout is 90 seconds. Increase it when an instance is slow,
hibernating, or under load.

### Pass credentials via environment variable (CI/headless)

Instead of storing credentials in the OS keychain, you can provide them
through environment variables for one-off or CI usage:

```bash
SNOW_CLI_PASSWORD='<password>' snow-cli table list incident --limit 5
SNOW_CLI_API_TOKEN='<token>' snow-cli table list incident --limit 5
```

The keychain is tried first; env vars are used as fallback.
