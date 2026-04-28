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
- Use `--fail-on-error` when transform-map row failures should fail CI or agent automation even if the HTTP request itself succeeded.
- Do not rely on arbitrary table names like `sys_user_import`; the import API requires a table that extends `sys_import_set_row`.
