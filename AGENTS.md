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
│   │       ├── table.rs       # table API commands + schema
│   │       ├── api.rs         # raw REST API commands
│   │       ├── script.rs      # background script execution
│   │       ├── codesearch.rs   # code search commands
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
    ├── test_cli.rs            # End-to-end CLI invocation tests
    ├── test_table.rs          # Table API + schema wiremock integration tests
    ├── test_api_script.rs     # API + script integration tests
    └── test_codesearch.rs     # Code search integration tests
```

## Key Architectural Decisions

Read `docs/adr/` for full context. Summary:

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Language | Rust | Single binary, no runtime dependency, strong types |
| Command pattern | Noun-verb (`snow-cli table list incident`) | Discoverable, natural for humans and agents |
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

- **Rust guidelines:** Follow the Microsoft Pragmatic Rust
  [Universal Guidelines](https://microsoft.github.io/rust-guidelines/guidelines/universal/index.html)
  when making changes. Prefer adopting these practices early in each slice:
  keep lint overrides narrow and justified with `#[expect(..., reason = "...")]`,
  keep sensitive values out of `Debug` and logs, document production constants
  whose values encode external behavior, use structured `tracing` fields for
  operational events, and keep module interfaces small and purposeful.
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

Phases 1 (Foundation), 2 (Authentication), and 3 (Table API) are **complete**.
Phase 4 (Commands) is **in progress** — most commands are done, `attachment` remains.

What's implemented and working:
- Full CLI structure with clap, profile management (add, edit, remove, default, current, list, find, sdk, show)
- All auth commands (login, logout, status, token)
- Basic Auth, OAuth2 (client_credentials + password grant), and API Key authenticators
- HTTP client with auto-pagination, 401 retry, and error mapping
- OS keychain credential storage with env var fallback
- Table API CRUD: list (auto-paginated), get, create, update (PATCH), delete (with --yes confirmation)
- Table schema command: query sys_dictionary for column metadata (compact/extended/inherited)
- Code search command: search via /api/sn_codesearch/code_search/search endpoint
- Raw API commands: get, post, put, delete with custom headers
- Script run command
- Client builder helper (`build_client`) for config profile → authenticated SnowClient
- JSON and CSV output for dynamic Record fields (sorted column headers, missing field handling)
- Stdin reading for create/update when --data not provided
- Shell completions generation (bash, zsh, fish, powershell, elvish)
- 210 tests (141 unit + 69 integration), zero clippy warnings

Next up: `attachment` commands (Phase 4), then Phase 5 (polish and distribution).

## Task Tracking

The project was started with the docs folder which is still valid. However from now on we can use 'bd' for task tracking (see Issue Tracking with bd).

## Issue Tracking with bd (beads)

**IMPORTANT**: This project uses **bd (beads)** for ALL issue tracking. Do NOT use markdown TODOs, task lists, or other tracking methods.

### Why bd?

- Dependency-aware: Track blockers and relationships between issues
- Git-friendly: Auto-syncs to JSONL for version control
- Agent-optimized: JSON output, ready work detection, discovered-from links
- Prevents duplicate tracking systems and confusion

### Quick Start

**Check for ready work:**
```bash
bd ready --json
```

**Create new issues:**
```bash
bd create "Issue title" -t bug|feature|task -p 0-4 --json
bd create "Issue title" -p 1 --deps discovered-from:bd-123 --json
bd create "Subtask" --parent <epic-id> --json  # Hierarchical subtask (gets ID like epic-id.1)
```

**Claim and update:**
```bash
bd update bd-42 --status in_progress --json
bd update bd-42 --priority 1 --json
```

**Complete work:**
```bash
bd close bd-42 --reason "Completed" --json
```

### Issue Types

- `bug` - Something broken
- `feature` - New functionality
- `task` - Work item (tests, docs, refactoring)
- `epic` - Large feature with subtasks
- `chore` - Maintenance (dependencies, tooling)

### Priorities

- `0` - Critical (security, data loss, broken builds)
- `1` - High (major features, important bugs)
- `2` - Medium (default, nice-to-have)
- `3` - Low (polish, optimization)
- `4` - Backlog (future ideas)

### Workflow for AI Agents

1. **Check ready work**: `bd ready` shows unblocked issues
2. **Claim your task**: `bd update <id> --status in_progress`
3. **Work on it**: Implement, test, document
4. **Discover new work?** Create linked issue:
   - `bd create "Found bug" -p 1 --deps discovered-from:<parent-id>`
5. **Complete**: `bd close <id> --reason "Done"`
6. **Commit together**: Always commit the `.beads/issues.jsonl` file together with the code changes so issue state stays in sync with code state

### Writing Self-Contained Issues

Issues must be fully self-contained - readable without any external context (plans, chat history, etc.). A future session should understand the issue completely from its description alone.

**Required elements:**
- **Summary**: What and why in 1-2 sentences
- **Files to modify**: Exact paths (with line numbers if relevant)
- **Implementation steps**: Numbered, specific actions
- **Example**: Show before → after transformation when applicable

**Optional but helpful:**
- Edge cases or gotchas to watch for
- Test references (point to test files or test_data examples)
- Dependencies on other issues

**Bad example:**
```
Implement the refactoring from the plan
```

**Good example:**
```
Add timeout parameter to fetchUser() in src/api/users.ts

1. Add optional timeout param (default 5000ms)
2. Pass to underlying fetch() call
3. Update tests in src/api/users.test.ts

Example: fetchUser(id) → fetchUser(id, { timeout: 3000 })
Depends on: bd-abc123 (fetch wrapper refactor)
```

### Dependencies: Think "Needs", Not "Before"

`bd dep add X Y` = "X needs Y" = Y blocks X

**TRAP**: Temporal words ("Phase 1", "before", "first") invert your thinking!
```
WRONG: "Phase 1 before Phase 2" → bd dep add phase1 phase2
RIGHT: "Phase 2 needs Phase 1" → bd dep add phase2 phase1
```
**Verify**: `bd blocked` - tasks blocked by prerequisites, not dependents.

### Auto-Sync

bd automatically syncs with git:
- Exports to `.beads/issues.jsonl` after changes (5s debounce)
- Imports from JSONL when newer (e.g., after `git pull`)
- No manual export/import needed!

### GitHub Copilot Integration

If using GitHub Copilot, also create `.github/copilot-instructions.md` for automatic instruction loading.
Run `bd onboard` to get the content, or see step 2 of the onboard instructions.
