# Phase 4 — Domain Commands and APIs

Higher-level commands and remaining API integrations.

## Work Items

- ~~Implement `incident` commands~~ (removed — achievable via `table` commands)
- [x] Implement `attachment` commands
  - [x] `attachment list <table> <sys_id>` — list attachments for a record
  - [x] `attachment download <sys_id>` — download to file (streaming)
  - [x] `attachment upload <table> <sys_id> --file <path>` — upload file
  - [x] Progress indicator for large files
  - [x] Tests with wiremock (upload, binary download)
- ~~Implement `import-set` commands~~ (delayed for later)
- [x] Implement `api` raw commands
  - [x] `api get <path>` — GET arbitrary endpoint
  - [x] `api post <path> --data <json>` — POST with body
  - [x] `api put <path> --data <json>` — PUT with body
  - [x] `api delete <path>` — DELETE endpoint
  - [x] Support `--header` flag for custom headers
  - [x] Tests for each HTTP method
- [x] Implement `table schema` command
  - [x] `table schema <table>` — show columns, types, labels (compact mode)
  - [x] `--extended` flag for additional field metadata
  - [x] `--include-inherited` flag for parent table fields
  - [x] JSON and CSV output support
  - [x] Tests for compact, extended, inherited, CSV, and empty result
- [x] Implement `codesearch` command
  - [x] `codesearch search <query>` — search code via Code Search API
  - [x] `--source-table` (alias `--table`), `--limit`, `--current-scope`, `--search-group` options
  - [x] Flexible response parsing (TableResponse, generic JSON, raw text)
  - [x] Tests for basic, table filter, custom limit, CSV, server error, non-standard response
- [x] Implement `script run` command (WIP)
  - [x] Basic command structure with --code, --file, --scope options
  - [x] Runtime warning: no default REST API for background scripts on ServiceNow

## UX/Config Improvements (Post-Review)

- [x] Resolve active profile from config default when `--profile` is omitted
- [x] Add profile execution hint on stderr for interactive terminals (`SNOW_CLI_PROFILE_HINT=0` to disable)
- [x] Add `config delete-profile <name>` with default-profile safety checks
- [x] Remove `incident` subcommand wiring and stale placeholder handler
