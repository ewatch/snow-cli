# Phase 1 — Foundation

Core project scaffolding, configuration management, and error handling.

## Prerequisite — First Build

Before starting any work items below, complete these one-time setup steps:

- [x] Run `cargo build` to download dependencies and verify compilation
  - The initial crate download was interrupted by a network timeout.
    All source files and `Cargo.toml` are in place; only the dependency
    fetch + compile step remains.
  - Fix any compilation errors that arise.
- [x] Run `cargo test` to verify all unit and integration tests pass
- [x] Run `cargo clippy -- -D warnings` to verify no lint warnings
- [x] Run `cargo fmt -- --check` to verify formatting
- [ ] Create the initial git commit with all scaffolding files

## Work Items — Scaffolding (Done)

- [x] Initialize Rust project with Cargo
- [x] Set up clap CLI structure with global flags and subcommands
- [x] Create module stubs for all major components
- [x] Create AGENTS.md for agent onboarding
- [x] Create docs/ structure (PLAN.md, ADRs, design docs, backlog, guides)
- [x] Define all clap argument types in `src/cli/args.rs`
- [x] Create command handler stubs with `todo!()` in `src/cli/commands/`
- [x] Define `CliError` enum with JSON serialization in `src/error.rs`
- [x] Define `AppConfig`, `Profile`, `AuthMethod` with TOML serde in `src/config/profile.rs`
- [x] Define `Authenticator` trait and `BasicAuth` stub in `src/auth/`
- [x] Define `SnowClient` struct in `src/client/mod.rs`
- [x] Define `Record`, `Incident`, `Attachment` models in `src/models/`
- [x] Write unit tests for args parsing, config round-trip, error serialization,
      models deserialization, and API error mapping
- [x] Write integration tests for CLI help/version/completions in `tests/test_cli.rs`

## Work Items — Implementation (TODO)

- [ ] Implement config module (connect stubs to real logic)
  - [ ] TOML config file loading and saving (stubs exist in `profile.rs`)
  - [ ] Default config creation on first run
  - [ ] Config path resolution (XDG / platform-specific)
- [ ] Implement credential storage (connect stubs to real logic)
  - [ ] Keyring integration — stubs exist in `credentials.rs`, need testing
  - [ ] Fallback behavior for headless environments
- [ ] Implement `config` commands (replace `todo!()` in `src/cli/commands/config.rs`)
  - [ ] `config init` — interactive setup wizard
  - [ ] `config set-profile <name>` — create/update profile
  - [ ] `config list-profiles` — list all profiles
  - [ ] `config use-profile <name>` — set default profile
  - [ ] `config show` — show current config
- [ ] Set up tracing-based logging (basic setup exists in `main.rs`)
  - [ ] Verify verbosity flag parsing works end-to-end
  - [ ] Ensure log output goes only to stderr
- [ ] Build core HTTP client wrapper (struct exists in `src/client/mod.rs`)
  - [ ] Instance URL resolution from profile
  - [ ] Authenticated request method using `Authenticator` trait
  - [ ] Request/response logging at debug level
  - [ ] Timeout configuration
  - [ ] Auto-retry on 401 with token refresh
