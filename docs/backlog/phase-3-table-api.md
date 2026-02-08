# Phase 3 — Table API and Pagination

Implement core Table API operations with auto-pagination.

## Work Items

- [ ] Implement auto-pagination
  - [ ] Parse `X-Total-Count` and `Link` response headers
  - [ ] Async stream of records across pages
  - [ ] Configurable page size (default: 100)
  - [ ] `--limit` flag to cap total records
  - [ ] Tests for single page, multi-page, empty results, and limit
- [ ] Implement `table list <table_name>`
  - [ ] Query parameters: `--query`, `--fields`, `--limit`, `--order-by`
  - [ ] Auto-paginated output
  - [ ] JSON and CSV output formatting
- [ ] Implement `table get <table_name> <sys_id>`
  - [ ] Single record fetch
  - [ ] `--fields` to select specific fields
- [ ] Implement `table create <table_name>`
  - [ ] Accept `--data` flag (inline JSON) or stdin
  - [ ] Return created record
- [ ] Implement `table update <table_name> <sys_id>`
  - [ ] Accept `--data` flag (inline JSON) or stdin
  - [ ] PATCH semantics (partial update)
  - [ ] Return updated record
- [ ] Implement `table delete <table_name> <sys_id>`
  - [ ] Confirmation prompt (bypass with `--yes`)
  - [ ] Return success/failure
- [ ] Implement output formatters
  - [ ] JSON formatter (pretty-print with `--pretty`, compact by default)
  - [ ] CSV formatter with header row
  - [ ] `--output` flag to select format
- [ ] Write tests
  - [ ] Pagination with wiremock (multi-page responses)
  - [ ] Each CRUD operation
  - [ ] Output format tests
  - [ ] Error cases (404, 403, invalid table)
