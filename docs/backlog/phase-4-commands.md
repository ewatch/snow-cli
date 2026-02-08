# Phase 4 — Domain Commands and APIs

Higher-level commands and remaining API integrations.

## Work Items

- [ ] Implement `incident` commands
  - [ ] `incident list` — wraps table API for `incident` table
  - [ ] `incident get <number>` — lookup by incident number (not sys_id)
  - [ ] `incident create` — create with common fields (short_description, etc.)
  - [ ] `incident update <number>` — update by incident number
  - [ ] `incident resolve <number>` — set state to resolved
  - [ ] Tests for each operation
- [ ] Implement `attachment` commands
  - [ ] `attachment list <table> <sys_id>` — list attachments for a record
  - [ ] `attachment download <sys_id>` — download to file (streaming)
  - [ ] `attachment upload <table> <sys_id> --file <path>` — upload file
  - [ ] Progress indicator for large files
  - [ ] Tests with wiremock (multipart upload, binary download)
- [ ] Implement `import-set` commands
  - [ ] `import-set load <table> --data <json>` — load data into staging
  - [ ] `import-set transform <sys_id>` — trigger transform map
  - [ ] Tests for load and transform
- [ ] Implement `api` raw commands
  - [ ] `api get <path>` — GET arbitrary endpoint
  - [ ] `api post <path> --data <json>` — POST with body
  - [ ] `api put <path> --data <json>` — PUT with body
  - [ ] `api delete <path>` — DELETE endpoint
  - [ ] Support `--header` flag for custom headers
  - [ ] Tests for each HTTP method
