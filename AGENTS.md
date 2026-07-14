# AGENTS.md — Coding Agent Onboarding

This file helps coding agents (LLMs, AI assistants, automated tools) quickly
understand, navigate, build, test, and contribute to the snow-cli project.

## What This Project Is

**snow-cli** is a cross-platform CLI written in Rust that serves as the primary
gateway for LLMs, coding agents, and humans to interact with ServiceNow instances.
It compiles to two cross-platform binaries with no runtime dependencies.

- **Binary names:** `snow-cli` and `snow-cli-ro` (read-only mode)
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

# Check code quality, including test targets
cargo clippy --all-targets -- -D warnings

# Format code
cargo fmt

# Format check (CI-friendly, no modifications)
cargo fmt -- --check

# Run the CLI
cargo run --bin snow-cli -- --help
cargo run --bin snow-cli -- profile --help
cargo run --bin snow-cli -- table list --help
cargo run --bin snow-cli-ro -- --help
```

## Navigation And Sources Of Truth

- `Cargo.toml` defines package metadata, supported binaries, and dependencies.
- `src/` contains the implementation. Start at `src/main.rs` or `src/lib.rs`; CLI definitions and handlers are under `src/cli/`.
- `tests/` contains integration tests; colocated `#[cfg(test)]` modules contain unit tests.
- `docs/adr/README.md` indexes accepted architectural decisions. Read relevant ADRs before changing an established design.
- `docs/design/` contains technical designs, `docs/guides/` contains contributor workflows, and `docs/book/` is user-facing documentation.
- `cargo run --bin snow-cli -- --help` and `cargo run --bin snow-cli-ro -- --help` are the authoritative command surfaces.

Keep this file limited to durable contributor rules and entry points. Do not add dependency inventories, complete file trees, command inventories, test counts, implementation-status lists, or roadmaps here; they drift. Update their authoritative source instead.

## Architectural Invariants

- Preserve the noun-verb CLI model unless an ADR changes it.
- Keep credentials out of plaintext configuration; use the OS keychain integration.
- Send structured command results to stdout and logs or errors to stderr.

### Module boundaries

- Preserve the dependency direction `models -> config -> auth -> client -> cli/commands`; `policy` and `snu` are cross-cutting modules.
- `src/client/` is the only HTTP seam. Commands must use its interface and must not import `reqwest`, construct HTTP clients, or expose transport response types.
- Active-policy mutation is process initialization owned by the CLI launch paths in `src/lib.rs`; command implementations must not mutate or re-read it.
- Process termination belongs only in the binary roots. Any new module-boundary exception requires an explicit structural-test allow-list change and rationale.

## How to Pick Up Work

1. Run `bd ready --json` to find unblocked work.
2. Claim an issue with `bd update <id> --status in_progress --json`.
3. Implement it, write tests, ensure `cargo test` and `cargo clippy --all-targets -- -D warnings` pass.
4. Close the issue with `bd close <id> --reason "Done" --json`.
5. Commit `.beads/issues.jsonl` together with code changes so issue state stays in sync.

The historical Markdown phase backlog remains under `docs/backlog/`, but active tracking uses `bd`.

## How to Add a New Command

Follow `docs/guides/adding-commands.md`; it is the authoritative command-extension workflow.

`snow graphql` is an optional command backed by `/api/now/graphql`. It requires an
administrator to enable Now GraphQL on the target instance and does not discover
schemas or enable the feature. It is excluded from read-only mode because GraphQL
documents may contain mutations.

## Testing

See `docs/guides/testing.md` for full details.

```bash
cargo test              # All tests
cargo test --lib        # Unit tests only
cargo test --test '*'   # Integration tests only
cargo clippy --all-targets -- -D warnings # Lint check, including test targets
cargo fmt -- --check    # Format check
```

**Testing approach:** Mock-based; no real ServiceNow instance is required.

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
- **Output:** Structured data goes to stdout via `src/cli/output.rs`.
  Errors go to stderr as JSON via `src/error.rs`.
- **No `unwrap()` in production code.** Use `?` operator or explicit error handling.
  `unwrap()` is acceptable in tests.
- **Run `cargo clippy --all-targets -- -D warnings` before committing.** All warnings are errors.
- **Run `cargo fmt` before committing.** Code must be formatted.

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
