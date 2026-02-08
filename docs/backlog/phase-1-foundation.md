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
- [x] Create the initial git commit with all scaffolding files

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

## Work Items — Implementation

- [x] Implement config module (connect stubs to real logic)
  - [x] TOML config file loading and saving (`load_from`/`save_to` in `profile.rs`)
  - [x] Default config creation on first run (`config init`)
  - [x] Config path resolution (`SNOW_CLI_CONFIG` env var, fallback to `~/.servicenow/config.toml`)
- [x] Implement credential storage (connect stubs to real logic)
  - [x] Keyring integration with `get_credential`/`store_credential`/`delete_credential`
  - [x] Fallback behavior for headless environments (`SNOW_CLI_PASSWORD`, `SNOW_CLI_API_TOKEN`, `SNOW_CLI_CLIENT_SECRET` env vars)
  - [x] Helper functions: `has_credential()`, `credential_type_for_auth()`
- [x] Implement `config` commands (replace `todo!()` in `src/cli/commands/config.rs`)
  - [x] `config init` — non-interactive setup with `--instance`, `--auth-method`, `--username`, `--name` flags
  - [x] `config set-profile <name>` — create/update profile with merge semantics
  - [x] `config list-profiles` — list all profiles (JSON/CSV)
  - [x] `config use-profile <name>` — set default profile
  - [x] `config show` — show current config (JSON/CSV)
- [x] Implement `auth` commands (replace `todo!()` in `src/cli/commands/auth.rs`)
  - [x] `auth login` — store credentials in keychain
  - [x] `auth logout` — delete credentials from keychain
  - [x] `auth status` — check credential availability
  - [x] `auth token` — output raw credential for piping
- [x] Set up tracing-based logging (basic setup exists in `main.rs`)
  - [x] Verify verbosity flag parsing works end-to-end (-v/-vv/-vvv)
  - [x] Ensure log output goes only to stderr
- [x] Build core HTTP client wrapper (`src/client/mod.rs`)
  - [x] Instance URL resolution from profile
  - [x] Authenticated request method using `Authenticator` trait
  - [x] Request/response logging at debug level
  - [x] Timeout configuration (`ClientConfig`)
  - [x] Auto-retry on 401 with token refresh
  - [x] HTTP methods: GET, POST, PUT, PATCH, DELETE
  - [x] JSON convenience methods: `get_json`, `get_json_with_params`, `post_json`
  - [x] Auto-paginated `get_table_records` for Table API
  - [x] `ApiError` mapping from HTTP status codes
- [x] Comprehensive test coverage
  - [x] 22 wiremock-based HTTP client tests (auth headers, 401 retry, pagination, error mapping, all HTTP methods)
  - [x] 12 config command unit tests (tempfile-based, no env var manipulation)
  - [x] 3 credential storage unit tests
  - [x] 12 config integration tests via assert_cmd (init, show, set-profile, use-profile, list-profiles)
  - [x] 8 CLI integration tests (help, version, completions, subcommand validation)
