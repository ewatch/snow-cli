# Phase 3 — Table API and Pagination

Implement core Table API operations with auto-pagination.

## Work Items

- [x] Implement auto-pagination
  - [x] Parse `X-Total-Count` and `Link` response headers
  - [x] Async stream of records across pages
  - [x] Configurable page size (default: 100)
  - [x] `--limit` flag to cap total records
  - [x] Tests for single page, multi-page, empty results, and limit
- [x] Implement `table list <table_name>`
  - [x] Query parameters: `--query`, `--fields`, `--limit`, `--order-by`
  - [x] Auto-paginated output
  - [x] JSON and CSV output formatting
- [x] Implement `table get <table_name> <sys_id>`
  - [x] Single record fetch
  - [x] `--fields` to select specific fields
- [x] Implement `table create <table_name>`
  - [x] Accept `--data` flag (inline JSON) or stdin
  - [x] Return created record
- [x] Implement `table update <table_name> <sys_id>`
  - [x] Accept `--data` flag (inline JSON) or stdin
  - [x] PATCH semantics (partial update)
  - [x] Return updated record
- [x] Implement `table delete <table_name> <sys_id>`
  - [x] Confirmation prompt (bypass with `--yes`)
  - [x] Return success/failure
- [x] Implement output formatters
  - [x] JSON formatter (pretty-print with `--pretty`, compact by default)
  - [x] CSV formatter with header row
  - [x] `--output` flag to select format
- [x] Write tests
  - [x] Pagination with wiremock (multi-page responses)
  - [x] Each CRUD operation
  - [x] Output format tests
  - [x] Error cases (404, 403, invalid table)
