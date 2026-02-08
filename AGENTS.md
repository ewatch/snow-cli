# AGENTS.md — Coding Agent Onboarding

This file helps coding agents (LLMs, AI assistants, automated tools) quickly
understand, navigate, build, test, and contribute to the snow-cli project.

## What This Project Is

**snow-cli** is a cross-platform CLI written in Rust that serves as the primary
gateway for LLMs, coding agents, and humans to interact with ServiceNow instances.
It compiles to a single static binary with no runtime dependencies.

- **Binary name:** `snow-cli`
- **Language:** Rust (latest stable, edition 2024)
- **Config file:** `~/.servicenow/config.toml`
- **License:** MIT

## Quick Start

```bash
# Build the project
cargo build

# Run tests (all unit + integration)
cargo test

# Run with verbose output to see test details
cargo test -- --nocapture

# Run only unit tests
cargo test --lib

# Run only integration tests
cargo test --test '*'

# Check code quality
cargo clippy -- -D warnings

# Format code
cargo fmt

# Format check (CI-friendly, no modifications)
cargo fmt -- --check

# Run the CLI
cargo run -- --help
cargo run -- config --help
cargo run -- table list --help
```

## Project Structure

```
snow-cli/
├── Cargo.toml                 # Dependencies and project metadata
├── LICENSE                    # MIT license
├── AGENTS.md                  # This file — agent onboarding
├── docs/
│   ├── PLAN.md                # Project plan, phases, command reference
│   ├── adr/                   # Architecture Decision Records
│   │   ├── README.md          # ADR index and template
│   │   ├── 0001-use-rust.md
│   │   ├── 0002-noun-verb-commands.md
│   │   ├── 0003-os-keychain-credentials.md
│   │   ├── 0004-toml-config-format.md
│   │   └── 0005-json-error-output.md
│   ├── design/                # Technical design documents
│   │   ├── README.md
│   │   ├── authentication.md  # Auth architecture and trait design
│   │   └── http-client.md     # HTTP client, pagination, error handling
│   ├── backlog/               # Work items organized by phase
│   │   ├── README.md          # Phase index
│   │   ├── phase-1-foundation.md
│   │   ├── phase-2-auth.md
│   │   ├── phase-3-table-api.md
│   │   ├── phase-4-commands.md
│   │   └── phase-5-distribution.md
│   └── guides/                # Developer guides
│       ├── README.md
│       ├── testing.md         # How to write and run tests
│       └── adding-commands.md # How to add a new CLI command
├── src/
│   ├── main.rs                # Entry point — CLI parsing, tracing init, dispatch
│   ├── error.rs               # CliError enum, JSON error output to stderr
│   ├── cli/
│   │   ├── mod.rs
│   │   ├── args.rs            # clap derive definitions for all commands
│   │   ├── output.rs          # JSON/CSV output formatting
│   │   └── commands/
│   │       ├── mod.rs
│   │       ├── config.rs      # config subcommands
│   │       ├── auth.rs        # auth subcommands
│   │       ├── table.rs       # table API commands
│   │       ├── incident.rs    # incident shortcuts
│   │       ├── attachment.rs  # attachment commands
│   │       ├── import_set.rs  # import set commands
│   │       ├── api.rs         # raw REST API commands
│   │       └── completions.rs # shell completions generation
│   ├── auth/
│   │   ├── mod.rs             # Authenticator trait + factory
│   │   └── basic.rs           # Basic auth implementation
│   ├── client/
│   │   ├── mod.rs             # SnowClient — HTTP client wrapper
│   │   ├── pagination.rs      # Auto-pagination for Table API
│   │   └── error.rs           # API error types and HTTP status mapping
│   ├── config/
│   │   ├── mod.rs
│   │   ├── profile.rs         # AppConfig, Profile, AuthMethod (TOML serde)
│   │   └── credentials.rs     # OS keychain integration via keyring crate
│   └── models/
│       ├── mod.rs
│       ├── record.rs          # Generic table record (HashMap-based)
│       ├── incident.rs        # Typed incident fields
│       └── attachment.rs      # Attachment metadata
└── tests/
    ├── common/
    │   └── mod.rs             # Shared test helpers
    └── test_cli.rs            # End-to-end CLI invocation tests
```

## Key Architectural Decisions

Read `docs/adr/` for full context. Summary:

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Language | Rust | Single binary, no runtime dependency, strong types |
| Command pattern | Noun-verb (`snow-cli incident list`) | Discoverable, natural for humans and agents |
| Config format | TOML (`~/.servicenow/config.toml`) | Human-readable, comments, Rust-native |
| Credentials | OS keychain (macOS Keychain, Linux Secret Service, Windows Credential Manager) | Secure, no plaintext secrets |
| Error output | Structured JSON on stderr | Machine-parseable for agents |
| Data output | JSON (default) or CSV on stdout | Agent-friendly, pipeable |

## Command Structure

The CLI follows a **noun-verb** pattern:

```
snow-cli [GLOBAL FLAGS] <NOUN> <VERB> [OPTIONS]
```

See `docs/PLAN.md` for the complete command reference.

## How to Pick Up Work

1. Read `docs/backlog/README.md` for the current phase.
2. Open the current phase file (e.g., `docs/backlog/phase-1-foundation.md`).
3. Find the next unchecked `[ ]` item.
4. Implement it, write tests, ensure `cargo test` and `cargo clippy` pass.
5. Mark the item as `[x]` when done.

## How to Add a New Command

See `docs/guides/adding-commands.md` for a step-by-step guide:

1. Define the clap subcommand in `src/cli/args.rs`
2. Create a handler in `src/cli/commands/<noun>.rs`
3. Register the module in `src/cli/commands/mod.rs`
4. Wire up the dispatch in `src/main.rs`
5. Write unit tests in the handler file and integration tests in `tests/`

## Testing

See `docs/guides/testing.md` for full details.

```bash
cargo test              # All tests
cargo test --lib        # Unit tests only
cargo test --test '*'   # Integration tests only
cargo clippy            # Lint check
cargo fmt -- --check    # Format check
```

**Testing approach:** Mock-based (no real ServiceNow instance required).
Uses `wiremock` for HTTP mocking and `assert_cmd` for CLI invocation tests.

**Convention:** Tests go in `#[cfg(test)] mod tests` blocks within source files
(unit tests) or in `tests/test_*.rs` files (integration tests).

## Code Conventions

- **Error handling:** Use `anyhow::Result` for functions that can fail.
  Use `thiserror` for defining error enum variants in `src/error.rs`.
- **Async:** All I/O operations are async (tokio). Command handlers are
  `async fn handle(...) -> anyhow::Result<()>`.
- **Logging:** Use `tracing::{info, debug, warn, error}` macros.
  All log output goes to stderr. Never print logs to stdout.
- **Output:** Structured data (JSON/CSV) goes to stdout via `src/cli/output.rs`.
  Errors go to stderr as JSON via `src/error.rs`.
- **No `unwrap()` in production code.** Use `?` operator or explicit error handling.
  `unwrap()` is acceptable in tests.
- **Run `cargo clippy -- -D warnings` before committing.** All warnings are errors.
- **Run `cargo fmt` before committing.** Code must be formatted.

## Dependencies

Key crates and their purposes:

| Crate | Purpose |
|-------|---------|
| `clap` + `clap_complete` | CLI argument parsing, shell completions |
| `tokio` | Async runtime |
| `reqwest` | HTTP client (with rustls for TLS) |
| `serde` + `serde_json` + `toml` + `csv` | Serialization (JSON, TOML, CSV) |
| `keyring` | OS-native credential storage |
| `tracing` + `tracing-subscriber` | Structured logging |
| `thiserror` + `anyhow` | Error handling |
| `async-trait` | Async trait support |
| `http` | HTTP types (HeaderMap, etc.) |
| `base64` | Base64 encoding for Basic auth |
| `wiremock` | HTTP mocking (dev) |
| `assert_cmd` + `predicates` | CLI testing (dev) |
| `tempfile` | Temporary files for tests (dev) |

## Current Status

The project is in **Phase 1 — Foundation**. The CLI structure, module stubs,
and argument parsing are in place. Command handlers currently contain `todo!()`
stubs that need to be implemented.

See `docs/backlog/phase-1-foundation.md` for the detailed work item list.
