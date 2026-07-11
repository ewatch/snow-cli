# Changelog

All notable changes to `snow-cli` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows semantic versioning conventions while it is pre-1.0.

## [Unreleased]

### Added

- Bounded `table list` defaults for agent workflows: at most 20 records without `--limit`, a new `--all` flag for full fetches, and a compact table-aware field projection without `--fields` (pass `'*'` for every field). List responses carry `total`/`returned`/`truncated` result metadata in every format except CSV.
- Per-field content cap for `table list` and `table get`: field values longer than 2,000 characters are cut with an inline `… [truncated N of M chars; use --full]` size hint, list metadata reports `fields_truncated`, and the new `--full` flag disables the cap.

### Changed

- Condensed the `snu` after-help from ~4.3 KB to ~1.3 KB for token economy; the operational detail it carried (broker env vars, `check-connection --verify` semantics, mutation channel) moved to the book's `snu` page, which the help now links.

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
