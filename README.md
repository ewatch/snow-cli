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

## Development

```bash
cargo fmt -- --check
cargo test
cargo clippy -- -D warnings
```

See the [documentation site](https://ewatch.github.io/snow-cli/) and the Markdown files under [`docs/`](docs/) for more details.
