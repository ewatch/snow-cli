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

# Fetch every matching record with every field, uncapped (bypasses the bounded defaults)
snow-cli table list incident --all --fields '*' --full

# Handle slow instances
snow-cli --timeout-secs 60 table list incident --limit 5

# Pipe credentials for CI (see auth docs for all env vars)
SNOW_CLI_PASSWORD='<password>' snow-cli table get incident <sys_id>
```

### Bounded list defaults and result metadata

`table list` is tuned for agent workflows. Without `--limit` or `--all` it
returns at most 20 records, and without `--fields` it returns a compact
table-aware field projection (pass `--fields '*'` for every field). Except in
CSV output, list responses carry result metadata so truncation is always
detectable:

```json
{ "total": 4381, "returned": 20, "truncated": true, "records": [ ... ] }
```

`total` comes from the server's `X-Total-Count` header and is omitted when the
server does not report one. For complete data set extraction, use
`snow-cli data export`, which keeps its explicit full-export semantics.

Field content is bounded too: unless `--full` is passed, `table list` and
`table get` cap each field value at 2,000 characters. Oversized values are cut
with an inline size hint so the truncation is self-describing:

```json
{ "script": "var gr = new GlideRecord('incident');… [truncated 2000 of 48213 chars; use --full]" }
```

When any field was capped, list metadata additionally carries
`"fields_truncated": true` (omitted otherwise).

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
