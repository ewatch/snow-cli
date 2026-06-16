# snow-cli

`snow-cli` is a cross-platform command-line interface for working with ServiceNow instances. It is designed for developers, automation scripts, CI jobs, coding agents, and LLM workflows that need machine-readable ServiceNow access.

## Documentation

Read the published documentation at **https://ewatch.github.io/snow-cli/**.

The docs include installation instructions, a quick start, configuration and authentication guidance, and the command overview.

## Quick start

```bash
snow-cli profile add dev \
  --instance https://your-instance.service-now.com \
  --auth-method basic \
  --username your-user

snow-cli auth login
snow-cli table list incident --query 'active=true' --limit 20
```

### Common patterns

```bash
# List records with a subset of fields in CSV format
snow-cli --output csv table list incident --fields number,short_description,priority --limit 10

# Get a single record
snow-cli table get incident <sys_id> --fields number,short_description,state

# Inspect table schema before building a query
snow-cli table schema incident --extended | head -20

# Export records to a portable file
snow-cli data export incident --fields number,short_description --limit 50 --out incidents.json

# Handle slow instances
snow-cli --timeout-secs 60 table list incident --limit 5

# Pipe credentials for CI (see auth docs for all env vars)
SNOW_CLI_PASSWORD='<password>' snow-cli table get incident <sys_id>
```

## Agent-safe read-only access

For agent harnesses that should not mutate ServiceNow through snow-cli, use the
read-only executable:

```bash
snow-cli-ro --profile readonly table list incident --query 'active=true' --limit 20
snow-cli-ro --profile readonly api get /api/x_myapp/status
```

`snow-cli-ro` exposes a reduced read-only command surface and runs with a locked
read-only policy. The full binary also supports `--read-only` for the same policy
enforcement while retaining the full parser surface. For stronger guarantees,
expose only `snow-cli-ro` to agents and use read-only ServiceNow credentials.
See [`docs/design/read-only-mode.md`](docs/design/read-only-mode.md).

## Development

```bash
cargo fmt -- --check
cargo test
cargo clippy -- -D warnings
```

See the [documentation site](https://ewatch.github.io/snow-cli/) and the Markdown files under [`docs/`](docs/) for more details.
