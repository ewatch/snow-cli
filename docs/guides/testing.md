# Testing Guide

## Running Tests

```bash
# Run all tests
cargo test

# Run tests with output (see println! output)
cargo test -- --nocapture

# Run a specific test
cargo test test_name

# Run tests in a specific module
cargo test module_name::

# Run only unit tests (skip integration tests)
cargo test --lib

# Run only integration tests
cargo test --test '*'
```

## Test Structure

```
src/
├── module/
│   ├── mod.rs          # Contains #[cfg(test)] mod tests { ... }
│   └── ...
tests/
├── common/
│   └── mod.rs          # Shared test helpers, mock server setup
├── test_cli.rs         # End-to-end CLI invocation tests
├── test_auth.rs        # Auth mechanism integration tests
├── test_table.rs       # Table API integration tests
└── ...
```

### Unit Tests

Place unit tests in the same file as the code they test, inside a
`#[cfg(test)]` module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        // ...
    }
}
```

### Integration Tests

Integration tests go in the `tests/` directory. They test the CLI as a black
box, invoking the binary and asserting on its output.

```rust
use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_help_flag() {
    Command::cargo_bin("snow-cli")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}
```

### HTTP Mocking with wiremock

Use `wiremock` to mock ServiceNow API responses in tests:

```rust
use wiremock::{MockServer, Mock, ResponseTemplate};
use wiremock::matchers::{method, path};

#[tokio::test]
async fn test_table_list() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/now/table/incident"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({
                "result": [{"sys_id": "abc123", "number": "INC0010001"}]
            })))
        .mount(&mock_server)
        .await;

    // Use mock_server.uri() as the instance URL in tests
}
```

## Test Conventions

1. **Test names** describe what is being tested: `test_config_loads_valid_toml`,
   `test_auth_basic_returns_header`, `test_table_list_paginates`.
2. **One assertion per concept** — a test can have multiple asserts if they
   verify the same logical outcome.
3. **No network calls** — all HTTP interactions are mocked with wiremock.
4. **No filesystem side effects** — use `tempfile` for any config file tests.

## CLI Response Contract Matrix

`tests/test_response_contract.rs` is the cross-family black-box compatibility
matrix, with independent decoders in `tests/common/response_contract.rs`.
Run it with `cargo test --test test_response_contract`; it also runs under
ordinary `cargo test` and `cargo clippy --all-targets -- -D warnings`.

| Response class | Representative command | Formats | Binaries |
|---|---|---|---|
| List: empty, full, partial, oversized fields | `table list` | all six | both |
| Explicit preview and verbose diagnostic separation | `table list --preview -v` | all except CSV | both |
| Detail: lossless oversized fields | `table get` | all six | both |
| Aggregate: zero count | `table stats` | all six | both |
| Artifact and summary | `data export --out` | all six | both |
| Mutation | `table create` | all six | writable |
| Partial mutation failure / explicit acceptance | `import-set load` | all except CSV | writable |
| Unsupported format rejected before mutation/input | `import-set load` | CSV | writable |
| Raw non-JSON passthrough | `api get` | all six | both |
| API errors and rate limits | `table list` | all six | both |
| Timeout | `table list --timeout-secs 1` | JSON | both |
| Help, version, missing command, unknown flag, denial | discovery / create | JSON (output-independent) | both |
| Non-interactive confirmation refusal | `table delete` without `--yes` | all six | writable |

The six formats are JSON, JSONL, TOON, CSV, text, and auto. Tests decode
responses and assert semantic fields, not exact whitespace or complete
snapshots. Mock request counts guard against unexpected calls; each subprocess
has a ten-second deadline. Successful responses keep stderr empty, errors keep
stdout empty except for documented partial-mutation results, and credentials
and private HTTP error bodies must not appear in either stream.

Intentional differences and limits:

- `snow-cli-ro` omits mutation subcommands: clap rejects them with exit 2 and
  human-readable stderr. `snow-cli --read-only` returns structured policy
  errors. Help/version use stdout; missing commands and unknown flags use
  clap diagnostics on stderr with exit 2, not the runtime JSON error envelope.
- Empty table-list JSONL still contains metadata. Empty CSV has no rows or
  headers and carries no list metadata; nonempty CSV carries records only.
- Raw non-JSON API bodies remain raw in every format, including JSON.
- Export artifacts follow the requested format; auto intentionally exports
  JSON so it remains importable. Explicit TOON/CSV artifacts are not JSON.
- Nested import-set responses do not support CSV. `import-set load` rejects
  resolved CSV output before consuming input or sending a request, including
  CSV selected via environment or config. Use `--output json` to override it.
  The partial-failure matrix covers the five supported structured formats.

Add a representative case here when introducing a response class or changing
format, stream, metadata, or exit semantics. Keep specialized per-command
regressions in their existing test modules as well.

## Runtime Error Contract

`cargo test --test test_error_contract` covers the taxonomy defined in
[ADR-0010](../adr/0010-runtime-error-contract.md): error code, class, retry advice,
exit status, stderr-only diagnostics, and safe handling of config excerpts,
input payloads, request queries/URLs, response bodies, and private I/O paths.
It also verifies the explicit partial-mutation stdout exception. Unit tests in
`src/error.rs` check bounded Unicode diagnostics and classification through
anyhow contexts. The response matrix retains help/version and plain-text clap
usage diagnostics as a separate, no-network contract.

For new failure paths, prefer a typed validation/config/auth/client error over
an unclassified anyhow string. Keep its source for classification when adding
context, but do not render raw parser or transport Display text. `retry` advice
must never grant unconditional permission to replay a mutation.

## Optional Live Validation

Use live-instance checks sparingly after wiremock coverage is in place.

Known working import-set example from live validation on profile `sprint`:

```bash
cargo run -- --profile sprint import-set load imp_user --data '{"user_name":"snow-cli-live-test","first_name":"Snow","last_name":"CLI","email":"snow-cli-live-test@example.com"}'
```

Notes:

- `imp_user` is a valid staging table on the validated `sprint` instance.
- The payload needed `email` because the active transform map coalesces on that field.
- The live response showed that `POST /api/now/import/imp_user` performed the load and transform in one step.
- Transform-map row failures fail by default even when HTTP succeeds. Use
  `--allow-row-errors` only to explicitly accept partial failures; detailed
  row results remain on stdout. `--fail-on-error` remains a compatibility flag.
- Do not rely on arbitrary table names like `sys_user_import`; the import API requires a table that extends `sys_import_set_row`.
