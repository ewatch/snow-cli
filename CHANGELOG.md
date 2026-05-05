# Changelog

All notable changes to `snow-cli` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows semantic versioning conventions while it is pre-1.0.

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
