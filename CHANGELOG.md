# Changelog

All notable changes to `snow-cli` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows semantic versioning conventions while it is pre-1.0.

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
