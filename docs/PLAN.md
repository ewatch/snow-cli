# snow-cli — Project Plan

## Overview

A cross-platform CLI written in **Rust** that serves as the primary gateway for LLMs,
coding agents, and humans to interact with ServiceNow instances. Compiles to a single
static binary with no runtime dependencies.

- **Binary:** `snow-cli`
- **Config:** `~/.servicenow/config.toml`
- **License:** MIT

## Technology Stack

| Component        | Choice                       | Crate(s)                          |
|------------------|------------------------------|-----------------------------------|
| Language         | Rust (latest stable)         | —                                 |
| CLI framework    | clap v4 + derive macros      | `clap`, `clap_complete`           |
| HTTP client      | reqwest (async, TLS)         | `reqwest`                         |
| Async runtime    | tokio (multi-thread)         | `tokio`                           |
| Serialization    | serde                        | `serde`, `serde_json`, `csv`      |
| Config file      | TOML                         | `toml`                            |
| Keychain         | OS-native credential store   | `keyring`                         |
| Logging          | tracing                      | `tracing`, `tracing-subscriber`   |
| TLS / mTLS       | rustls + client certs        | `reqwest` with `rustls-tls`       |
| Testing          | Built-in + HTTP mocking      | `wiremock`, `assert_cmd`, `predicates` |

## Command Structure (Noun-Verb)

```
snow-cli [GLOBAL FLAGS] <NOUN> <VERB> [OPTIONS]
```

### Global Flags

| Flag                   | Description                              |
|------------------------|------------------------------------------|
| `--profile <name>`    | Use named profile (otherwise use configured default profile) |
| `--instance <url>`    | Override instance URL                    |
| `--output <json|csv>` | Output format (default: json)            |
| `-v / -vv / -vvv`     | Verbosity level                          |
| `--version`           | Print version                            |
| `--help`              | Print help                               |

### Commands

```
snow-cli profile
  add <name>                      Create a new profile
  edit <name>                     Edit an existing profile
  remove <name>                   Remove a profile
  default <name>                  Set the default profile used when --profile is omitted
  current                         Show the currently selected profile
  show                            Show current active profile config
  list                            List all profiles
  find --instance <selector>      Find profiles by instance name, host, or URL
  sdk list                        List saved now-sdk authentication aliases
  sdk import [--alias|--all]      Import now-sdk aliases into snow-cli profiles
  sdk export <profile>            Export a profile into the now-sdk alias store

snow-cli auth
  login                           Authenticate and store credentials
  logout                          Clear stored credentials
  status                          Show current auth state
  token                           Print current access token (for piping)

snow-cli table
  list <table_name>               List records (auto-paginated)
  get <table_name> <sys_id>       Get a single record
  create <table_name>             Create a record (--data or stdin)
  update <table_name> <sys_id>    Update a record
  delete <table_name> <sys_id>    Delete a record
  schema <table_name>             Show table schema (columns, types, labels)

snow-cli data
  export <table_name>             Export table records as a portable dataset
  export-package --file <spec>    Export a multi-table dataset package from a spec
  validate --file <dataset>       Validate dataset compatibility before import
  import --file <dataset>         Import a dataset through the preferred load path
                                   (supports `--dry-run` preview)

snow-cli seed
  plan --file <spec>              Validate a seed spec and show execution plan
  apply --file <spec>             Create multi-table test data from a seed spec
  cleanup <run_id>                Remove records created by a prior seed run

snow-cli scope
  inspect <scope>                 Inspect scope metadata and artifact counts
  inventory <scope>               Export normalized scope artifacts

snow-cli codesearch
  search <query>                  Search code across the instance

snow-cli attachment
  list <table> <sys_id>           List attachments for a record
  download <sys_id>               Download an attachment
  upload <table> <sys_id>         Upload a file as attachment

snow-cli api
  get <path>                      GET a custom REST endpoint
  post <path>                     POST to a custom REST endpoint
  put <path>                      PUT to a custom REST endpoint
  delete <path>                   DELETE a custom REST endpoint

snow-cli script
  run                             Run a background script

snow-cli completions <shell>      Generate shell completions
```

## Authentication Methods

All auth methods implement a common `Authenticator` trait:

| Method          | Flow                                                   |
|-----------------|--------------------------------------------------------|
| Basic Auth      | Username/password, stored in OS keychain               |
| OAuth 2.0       | Client credentials or authorization code flow          |
| API Key / Token | Bearer token stored in keychain                        |
| mTLS            | Client certificate + key file paths in config          |
| SSO / SAML      | Browser-based SAML flow, captures token via callback   |

## ServiceNow APIs

| API             | Scope                                                  |
|-----------------|--------------------------------------------------------|
| Table API       | CRUD on any ServiceNow table                           |
| Scripted REST   | Custom REST endpoints defined in ServiceNow            |
| Import Set API  | Bulk data import via staging tables                    |
| Attachment API  | Upload/download file attachments                       |

## Output

- **stdout:** Structured data (JSON or CSV)
- **stderr:** Structured JSON errors + log output

### Error Format

```json
{
  "error": {
    "code": "AUTH_TOKEN_EXPIRED",
    "message": "OAuth token expired and refresh failed",
    "status": 401,
    "detail": "Token refresh returned 403: insufficient scope",
    "instance": "https://mycompany.service-now.com"
  }
}
```

## Configuration Example

`~/.servicenow/config.toml`:

```toml
default_profile = "dev"

[profiles.dev]
instance = "https://dev-company.service-now.com"
auth_method = "oauth2"
client_id = "abc123"

[profiles.prod]
instance = "https://company.service-now.com"
auth_method = "basic"
username = "admin"

[profiles.staging]
instance = "https://staging-company.service-now.com"
auth_method = "mtls"
cert_path = "/path/to/client.pem"
key_path = "/path/to/client-key.pem"
```

Secrets (passwords, client secrets, tokens) are stored in the OS keychain,
never in the config file.

## Implementation Phases

### Phase 1 — Foundation ✓

- [x] Initialize Rust project with Cargo
- [x] Set up clap CLI structure with global flags
- [x] Implement config module (TOML loading/saving, profile management)
- [x] Implement credential storage with `keyring` crate
- [x] Build core HTTP client wrapper with reqwest
- [x] Implement error types with structured JSON output
- [x] Set up tracing-based logging with verbosity flags
- [x] Write tests for config and error handling

### Phase 2 — Authentication ✓

- [x] Define `Authenticator` trait
- [x] Implement Basic Auth
- [x] Implement OAuth 2.0 (client credentials flow)
- [x] Implement API Key/Token auth
- [x] Implement `auth` commands (login, logout, status)
- [x] Write tests with wiremock for each auth method

### Phase 3 — Table API + Pagination ✓

- [x] Implement auto-pagination module
- [x] Implement `table` commands (list, get, create, update, delete)
- [x] Implement JSON and CSV output formatters
- [x] Write tests for pagination edge cases

### Phase 4 — Domain Commands and APIs (in progress)

- [x] Implement `api` raw endpoint commands (get, post, put, delete with --header)
- [x] Implement `table schema` command (compact, extended, include-inherited)
- [x] Implement `codesearch` command (search via Code Search API)
- [x] Implement `script run` command
- [ ] Implement `attachment` commands (upload/download with streaming)
- [x] Write tests for each command group
- ~~Implement `incident` shortcut commands~~ (removed — achievable via `table` commands)
- ~~Implement `import-set` commands~~ (delayed for later)

### Phase 5 — Polish and Distribution

- [x] Add shell completions generation
- [x] Implement `config init` first-time bootstrap
- [ ] Implement `data export` MVP and command model for `data` / `seed`
- [ ] Implement dataset packages, reference remapping, and seed workflows
- [ ] Set up CI/CD (GitHub Actions) for cross-compilation
- [ ] Create Homebrew formula
- [ ] Add mTLS and SSO/SAML auth
