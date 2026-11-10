# Changelog

All notable changes to `snow-cli` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows semantic versioning conventions while it is pre-1.0.

## [Unreleased]

### Added

- `domain table <table> --counts` counts records per domain from one Aggregate API request, with names and display paths, scoped to the session's visible domains. For admin or `query_no_domain_table_api` sessions it also reports `hidden` records from two Table API totals. The Aggregate API ignores `sysparm_query_no_domain` even with the role.
- `domain table --counts --via-script` counts across all domains with one read-only `GlideAggregate` background script. It is admin-only and denied in `snow-cli-ro`.
- `table list` and `table get --with-domain` add `sys_domain`, `sys_domain_path`, and a resolved `sys_domain_name` to records; lists add a `domains_in_page` summary.
- `domain status` reports `separation_in_use: true` when the user's home domain is not `global`, which works without access to `sys_properties`.

### Changed

- `domain table` reports `has_sys_domain: false` when the dictionary is readable and the table has no `sys_domain` field; `null` now only means the dictionary could not be read.
- `--query-no-domain` prints a warning when the session has neither `admin` nor `query_no_domain_table_api`, because ServiceNow silently ignores the option in that case.

## [0.10.0] - 2026-10-15

### Security

- **Breaking:** `--instance` no longer sends a profile's credentials to another instance. An override must match the profile's scheme, host, and port; otherwise the command fails with `INSTANCE_MISMATCH` (config class, exit 2) before any credential is read. Previously the stored password or token was sent to any URL that passed shape validation, so a prompt-injected `--instance` could leak it. To use another instance, add a profile for it and select it with `--profile`.
- **Breaking:** `auth token` prints only OAuth2 access tokens by default. For Basic profiles it printed `base64(username:password)`, which decodes to the password, and for API-key profiles the stored key. Those profiles now fail with `VALIDATION_ERROR` before the keychain is read unless `--reveal` is passed.

### Fixed

- `script run` output no longer shows `<redacted>` inside ordinary words. Redaction removed every cookie value as a substring, so short preference cookies (for example a language cookie `en`) turned "Incident" into `Incid<redacted>t`. Only session and token cookies of at least 8 characters are now redacted; the password, `Authorization` header, and `X-UserToken` are still always redacted.
- JSON, JSONL, and TOON output now lists record fields in a stable, sorted key order. Previously the order changed between identical runs.

### Changed

- **Breaking:** `scope list` output is bounded to 20 rows by default (pass `--all` for the previous behavior), with `--limit`, `--offset`, and `--all`. It adds `offset`, `returned`, `truncated`, and `next_offset` like `table list`; `counts` still cover every matching row. On a PDI the unbounded list was about 2,200 rows (418 KB).
- E2E artifacts keep the instance URL and username visible by default and no longer redact them inside other words (`sys_[REDACTED]istrative`). Credentials are still always redacted. Set `SNOW_E2E_REDACT_IDENTITY=1` to replace them with `<instance>` and `<user>` for published artifacts; the release E2E tester uses it.
- E2E `shell` steps are expanded by bash at run time instead of by the scenario parser, so `$SNOW_E2E_PASSWORD` no longer appears in the `bash -c` command line visible to `ps`.
- E2E runs now use a per-run file keychain by default, deleted with the run's scratch directory. Previously `profile sdk export` and `import` scenarios wrote aliases with the test instance's credentials into the user's real now-sdk alias store and left them there. Set `SNOW_E2E_REAL_KEYCHAIN=1` to test the OS keychain; the now-sdk export and import scenarios are then skipped.

## [0.9.0] - 2026-10-15

### Added

- `auth status --verify` makes an authenticated identity request plus a best-effort build lookup and reports `verified`, the session user (`verified_user`, `verified_user_sys_id`), `latency_ms`, and a best-effort `build`. On failure it reports `verification_error` and exits non-zero. It is available in `snow-cli-ro`.
- Added ServiceNow SDK OAuth manual-code login with `auth login --sdk-oauth` and optional `--code-stdin`, without a localhost callback listener.
- Added instance-bound native cookie sessions with `session login`, `attach`, `show`, `options`, `switch`, and `forget`; credentials remain in the OS keychain. Read-only mode excludes login and context switching.
- Added domain-aware table reads and domain inspection commands, explicit `--query-no-domain`, and verified native domain/application/update-set context switching.
- Added explicit table-list paging metadata (`offset` and `next_offset`) and opt-in `--preview` metadata for oversized table and Code Search fields; `--full` remains a lossless escape hatch.
- Added cross-family response-contract tests for both binaries and supported output formats.
- Added `seed apply` and `seed cleanup`. Seed specs are validated offline and planned in dependency order; `seed apply` supports `--dry-run`, template expansion, round-robin references, and per-record run tracking, and `seed cleanup` deletes tracked records in reverse order after an instance check, with `--dry-run` and confirmation. Seeding uses the Table API.
- Added `data export --import-ready` to prepare a create-only JSON dataset from readable source dictionary metadata. It omits system-managed, read-only, unreadable, and unsupported field types and reports them in `preparation.omitted_fields`; references are not remapped, so validate on the destination.
- Import validation failures now include bounded, value-free itemized `error.issues` (`kind`, optional `field` and `record_index`, `message`) with `issue_count` and `issues_truncated`.

### Changed

- **Breaking:** `script run` against `/sys.scripts.do` now returns structured execution results for JSON, JSONL, TOON, and auto output (`output`, `duration_ms`, `scope`, `history_sys_id`, `errors`); text/CSV and custom endpoints retain their existing passthrough behavior.
- **Breaking:** `import-set load` now exits nonzero for row-level errors by default while preserving its detailed result on stdout. Use `--allow-row-errors` to explicitly accept partial failures; `--fail-on-error` remains a compatibility flag. `data import --import-set-table` retains its opt-in `--fail-on-error` behavior.
- **Breaking:** Known local validation, configuration, credential, transport, and I/O failures now use specific error codes and exit classifications instead of generic internal errors. Runtime JSON errors add mandatory `class` and conservative `retry` guidance; no automatic mutation retry is introduced. Clap usage diagnostics remain human-readable.
- **Breaking:** Table and Code Search field content is lossless by default. Previously values over 2000 characters were cut with inline `[truncated N of M chars]` markers and `fields_truncated` metadata. Shortening now requires `--preview`, with separate machine-readable metadata rather than inline markers; preview is rejected with CSV output.
- Refactored SN-Utils WebSocket action dispatch and updated Rust and Astro dependencies.
- Refreshed Astro documentation, base-path link processing, Homebrew upgrade instructions, piped installer environment examples, and read-only quick-start guidance.
- **Breaking:** `auth login` verifies stored passwords, API tokens, and OAuth client secrets (client-credentials and password grant) with one identity request. It reports `"status":"verified"` with `verified_user` and `verified_user_sys_id`; when the instance rejects the credentials it reports `"status":"stored"`, `verified: false`, and `verification_error`, keeps the secret, and exits non-zero. It previously printed `"status":"authenticated"` without contacting the instance, even for a wrong password. `--no-verify` stores without a request and reports `"status":"stored"`. Authorization-code logins now report `"status":"verified"`. All `auth` output except `auth token` now honours `--output`.
- **Breaking:** `auth status` no longer reports `authenticated`. That field only meant a credential was stored, yet read as "the instance accepted it". It is replaced by `credentials_present`; use `auth status --verify` and its `verified` field for a real server check. The output now honours `--output` and is compact JSON by default.
- `table schema` now returns the effective schema by default: columns inherited from parent tables are included (found by walking `sys_db_object.super_class`), each column is tagged with its defining `table`, and a child override replaces the parent definition. Use `--own-only` for columns defined on the table itself. `--include-inherited` is still accepted but hidden, since it is now the default; it previously returned only the table's own columns because `nameINSTANCEOF` does not work on `sys_dictionary.name`. `data validate` also uses the deduplicated effective schema.

### Fixed

- `script run` now rejects recognized ServiceNow execution diagnostics with `SCRIPT_EXECUTION_FAILED`, nonzero exit, and no success output; scripts may already have had side effects.
- `import-set load --output csv` is rejected before any mutation.
- Code Search now resolves scoped searches correctly and bounds grouped hits.
- Native cookie sessions now honor server expiry attributes. Context switching rejects missing or malformed picker selections before mutation and during read-back, rather than claiming verified success with unknown context.
- Native bootstrap HTTP failures and context-switch failures preserve their status and typed runtime classifications. Uncertain context switches, including incomplete context after the request, report `SESSION_SWITCH_UNCERTAIN` with `manual_review` guidance. Malformed successful OAuth token responses are classified as remote transport response errors, not local JSON validation errors.
- Invalid session input and zero domain limits report validation errors before network requests. Added all ten domain/session E2E scenario files with explicit native-UI and domain-separation capability gates, backed by a mock execution of the complete scenario matrix.
- Piped tracing output no longer includes ANSI colors.
- OAuth authorization-code callbacks now read the complete request headers, so a callback split across several TCP reads no longer fails.
- API errors now report the ServiceNow cause. The standard `{"error":{"message","detail"}}` envelope is surfaced in the structured JSON error after secret scrubbing and length bounding, with specific codes `INVALID_TABLE`, `RECORD_NOT_FOUND`, and `ACL_DENIED` where the message is unambiguous. The default-level `API request failed` log line that printed the raw response body on stderr is now a body-free `debug` event.
- `table list --order-by` and `data export --order-by` now sort. They previously sent `sysparm_orderby`, which the Table API ignores, so results came back in default order. Sort keys are now appended to `sysparm_query` as `ORDERBY`/`ORDERBYDESC` clauses; `-field` and `field:desc` sort descending, several keys can be comma-separated, and invalid specs are rejected before any request. Dataset package `order_by` entries use the same syntax.

### Security

- Updated rustls to 0.23.46 to address RUSTSEC-2026-0285, RUSTSEC-2026-0346, RUSTSEC-2026-0347, RUSTSEC-2026-0348, and RUSTSEC-2026-0349, and the docs-site http-cache-semantics dependency to 4.3.0 (GHSA-ch52-4w7c-c8xp).
- Hardened HTTP, SN-Utils, script, configuration, and runtime failure diagnostics: secret-like values are scrubbed, messages are bounded, and sensitive bodies, URLs, source excerpts, and stack traces are withheld from normal error output.

## [0.8.0] - 2026-08-25

### Added

- Added SN-Utils helper capability negotiation with redacted build, license-tier, and per-instance security-gate status in `snu check-connection`.
- Added community Agent API commands for context reads, browser-session REST requests, parent/reference options, live form state, field updates, UI actions, element clicks, and navigation. Pro-only code search and CDP debugger actions remain intentionally excluded.
- Added read-only mirrors for safe SN-Utils context, REST GET, form-state, and records-metadata operations while keeping page and instance mutations unavailable in `snow-cli-ro`.

### Changed

- Background-script commands now use the correlated `agentRunBackgroundScript` API with an atomic `executeBackgroundScript` fallback for legacy helpers.
- The SN-Utils E2E harness now supports current and legacy authorization interfaces, extension metadata capture, disposable gate configuration, and deterministic page-tab targeting.
- Screenshots now resolve the selected instance session before dispatch so the `browserDebugger` gate can be preflighted for URL-pattern and tab-id targets.

### Security

- Gated actions are classified and preflighted before reaching the browser socket. Blocked, unauthorized, unavailable, paused, and approval-timeout states return structured, secret-free errors while the browser extension remains the final enforcement and approval authority.
- Gate decisions are bound atomically to the helper WebSocket generation and negotiated support state; stale revisions, reconnect races, omitted instance authorization, and contradictory capability downgrades fail closed.
- Hardened Agent REST endpoint validation, response token redaction, session-probe handling, and serialized legacy mutation dispatch.

### Validation

- Passed the complete local test, Clippy, formatting, documentation-build, and 78-command E2E coverage gates.
- Passed the reviewed live SN-Utils matrix: 32 passed, 0 failed, 0 skipped with current helper authorization and all disposable security gates enabled in automatic mode.

## [0.7.0] - 2026-07-21

### Added

- `scope inspect` and `scope inventory` (and their read-only mirrors) now accept `--limit N` to bound artifact enumeration, threading a per-table cap through enumeration and counting via the `X-Total-Count` header instead of paginating every matching row. Enumerating the platform `global` scope unbounded is refused (pass `--limit`), and capped tables emit truncation warnings.
- The `snu` broker and bridge listen addresses can be overridden via the `SNOW_CLI_SNU_WS_ADDR` and `SNOW_CLI_SNU_BROKER_ADDR` environment variables (defaults `127.0.0.1:1978` / `127.0.0.1:1979`), so an automated second SN-Utils connection no longer evicts an already-connected ScriptSync browser tab.

### Fixed

- `script run` against the background-script form endpoint now resolves a human-readable `--scope` name to its `sys_scope` sys_id before submitting, instead of sending the name verbatim; the `global` scope and raw sys_ids are passed through unchanged.
- `snu create-record` now fails when SN-Utils returns an error or a response without a 32-hex `sys_id`, instead of reporting a false success; it also reads the `newRecord` response key.

### Changed

- Updated dependencies: `indicatif` 0.18.5 → 0.18.6, `clap_complete` 4.6.5 → 4.6.7, `rand` 0.10.1 → 0.10.2, `open` 5.3.6 → 5.4.0, and `uuid` 1.23.4 → 1.23.5.

## [0.6.0] - 2026-07-17

### Added

- Bounded `table list` defaults for agent workflows: at most 20 records without `--limit`, a new `--all` flag for full fetches, and a compact table-aware field projection without `--fields` (pass `'*'` for every field). List responses carry `total`/`returned`/`truncated` result metadata in every format except CSV.
- Per-field content cap for `table list` and `table get`: field values longer than 2,000 characters are cut with an inline `… [truncated N of M chars; use --full]` size hint, list metadata reports `fields_truncated`, and the new `--full` flag disables the cap.
- Added `table stats` backed by the Aggregate API (`GET /api/now/stats/{table}`): returns record counts by default and supports `--group-by`, `--avg`, `--min`, `--max`, `--sum`, and `--having` for grouped aggregates. Available in `snow-cli-ro` as a read-only command.
- Added an opt-in `graphql query` command for the ServiceNow GraphQL API.
- Added a token-efficient `auto` output format and made the chosen default output format persistent.
- Added a `skill install` command for installing agent skills.
- Added a declarative e2e scenario harness (`scripts/e2e-run`, `tests/e2e/scenarios/**/*.toml`) with a coverage gate, validated against a real PDI.
- Added a CI workflow running `cargo fmt --check`, `cargo clippy -D warnings`, and `cargo test` on every PR and push to main.

### Changed

- Condensed the `snu` after-help from ~4.3 KB to ~1.3 KB for token economy; the operational detail it carried (broker env vars, `check-connection --verify` semantics, mutation channel) moved to the book's `snu` page, which the help now links.
- Reworked the `snu` integration around a broker-owned WebSocket connection manager with probe-before-prompt reauthentication and resilient mutation parsing; update/delete mutations now go through the background-script bridge instead of cookie-less REST.

### Fixed

- Fixed `snu check-connection` and `snu get-instance-info` hanging against the legacy bridge protocol.
- Mapped empty background-script output to an expired-session hint instead of a confusing parse error.
- Fixed e2e scenario temp-file templates that BSD `mktemp` never randomized, which caused silent setup-step failures and stale-file collisions between runs; scenario setup steps now fail fast.

### Security

- Wrapped OAuth tokens and `now-sdk` credentials in a `Secret<T>` type so they are redacted from `Debug` output and logs.
- Identifiers are now strictly validated: path-traversal sequences are rejected and `sys_id` values must be 32-hex, backed by `TableName`/`SysId` newtypes used throughout the request path.

## [0.5.1] - 2026-06-24

### Added

- Added `snu create-record` and `snu app-meta` commands.
- Routed `g_ck` per instance and added session cache controls.

### Changed

- Migrated the `snu` integration to a persistent broker that stays alive across commands, hardening auth reliability and eliminating repeated `/token` round-trips.

### Fixed

- Fixed the release pipeline so assets are uploaded to a draft GitHub release and the release is published as a final step, which is required now that immutable releases are enabled. Dropped the duplicate `release: published` trigger that raced the tag-push run, and made the workflow build from the dispatched commit so `workflow_dispatch` runs no longer fail when the tag does not yet exist.
- Fixed `snu create-record` to send the request body under the `payload` key.
- Capped the helper-tab connection wait so a missing bridge fails fast.

### Security

- Bumped `quinn-proto` to 0.11.15 to address RUSTSEC-2026-0185, plus routine dependency updates (`rand`, `toml`, `sha2`, `toon-format`).

## [0.4.2] - 2026-06-16

### Fixed

- Fixed the release pipeline, which had never produced working artifacts. Replaced the misconfigured cargo-dist packaging with direct `tar.xz`/`zip` archiving, so macOS, Linux, and Windows archives are built and named `snow-cli-<version>-<target>.<ext>` as the install scripts and Homebrew tap expect.
- Fixed Linux release builds by compiling the `x86_64` and `aarch64` targets through `cross` with `libdbus-1-dev` provided for the target architecture (see `Cross.toml`), resolving the missing `libdbus`/cross-compiler errors.
- Added a release guard that fails fast with a clear message when the git tag does not match the version in `Cargo.toml`.

## [0.4.0] - 2026-05-17

### Added

- Added read-only policy engine and `snow-cli-ro` executable. Commands that mutate ServiceNow data are blocked when the policy is active.
- Added cross-platform install script (`scripts/install.sh`) for easy binary installation on macOS and Linux.
- Added PDI testing guide (`docs/book/pdi-testing.md`) with step-by-step OAuth setup instructions for Personal Developer Instances.
- Added ServiceNow-side OAuth application setup instructions to the OAuth authorization-code + PKCE guide.

### Changed

- Removed `mtls` and `saml` from documented supported auth methods; `browser-session` (cookie-based) is the current SSO workaround.
- Updated `docs/PLAN.md` to reflect that `attachment` and `data` commands are fully implemented.
- Updated installation docs to cover the install script, pre-built binaries, and build-from-source paths.

### Validation

- `cargo fmt`
- `cargo clippy -- -D warnings`
- `cargo test`

## [0.3.1] - 2026-05-11

### Security

- Restricted authenticated requests to the configured ServiceNow origin and reject off-origin absolute URLs before credentials are sent.
- Changed the ServiceNow form-login fallback from GET query parameters to POST form bodies so passwords no longer appear in URLs.
- Redacted sensitive HTTP debug data and API error bodies by default; sensitive output now requires explicit opt-in.
- Updated `snow-cli auth token` for OAuth2 profiles to print a short-lived access token instead of stored client secrets.
- Added stricter validation for instance URLs, table names, path segments, encoded-query literals, attachment filenames, and dataset package file names.
- Limited OAuth authorization-code redirect listeners to loopback hosts and hardened callback state/PKCE handling.
- Added size limits for stdin request bodies and attachment uploads to reduce oversized-input risk.
- Neutralized formula-like CSV cell values to protect spreadsheet users from CSV injection.
- Gated the plaintext test keychain store behind explicit unsafe opt-in and tightened Unix file permissions.
- Added weekly dependency/security monitoring with Dependabot and `cargo audit`; release workflow permissions now follow least-privilege defaults.

### Added

- Added stdin-based secret input flags for auth flows, including `--password-stdin`, `--token-stdin`, `--client-secret-stdin`, and `--session-cookie-stdin`.
- Added changelog-driven GitHub release notes.
- Expanded the mdBook command reference and added an OAuth authorization-code + PKCE guide.
- Polished help output and long-running interactive waits with a stderr-only snowflake spinner.

### Changed

- GitHub releases now publish only platform archives plus a consolidated `SHA256SUMS` file; installer scripts are intentionally not uploaded.
- Some previously tolerated but unsafe inputs are now rejected earlier with clearer validation errors.

### Validation

- `cargo fmt`
- `cargo clippy -- -D warnings`
- `cargo test`

## [0.3.0] - 2026-05-05

### Added

- Added JSON Lines output via `--output jsonl` / `--format jsonl`.
  - Array outputs are emitted as one compact JSON value per line.
  - Object and scalar outputs are emitted as a single compact JSON line.
- Added TOON output via `--output toon` / `--format toon` for LLM-friendly, token-efficient structured output.
  - Uses the official `toon-format` Rust crate.
  - Supports general JSON-shaped output, including nested and irregular API responses.
  - Best suited for arrays of similarly shaped ServiceNow records.
- Added `--format` as an alias for the existing global `--output` flag.

### Changed

- Bumped crate version from `0.2.0` to `0.3.0`.
- Extended output handling across table, schema, profile/config, raw API, script, scope, and data workflows so the new formats are available consistently where structured output is produced.

### Validation

- `cargo test`
- `cargo clippy -- -D warnings`
