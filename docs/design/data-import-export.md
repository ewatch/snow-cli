# Data Import, Export, and Seeding

## Overview

snow-cli already supports record-level CRUD through the Table API and generic API
access through `api` commands. What is missing is a first-class workflow for
portable data movement and fast test-data setup.

This design introduces two related command groups:

- `data` for export, validation, and import.
- `seed` for declarative multi-table test-data planning, creation, and cleanup.

The design favors deterministic file formats, JSON-first output, and the fastest
safe ingestion path available on the target instance.

## Goals

1. Export table data into portable, machine-readable artifacts.
2. Import flat datasets through a bulk-oriented default path.
3. Support multi-table datasets with dependency ordering and reference remapping.
4. Let users define repeatable test data declaratively.
5. Prefer the fastest supported load strategy while making the strategy visible.
6. Keep all commands non-interactive by default and safe for agents and CI.

## Non-Goals for v1

- Full environment cloning or bidirectional synchronization.
- Arbitrary transform-map authoring for Import Set API.
- General-purpose conflict resolution UI.
- Automatic attachment export/import in the MVP dataset format.
- Broad update/upsert semantics across multi-table imports.

## Command Model

```text
snow-cli data export <table>
snow-cli data validate --file <dataset>
snow-cli data import --file <dataset>

snow-cli seed plan --file <spec>
snow-cli seed apply --file <spec>
snow-cli seed cleanup <run-id>
```

### `data export`

Exports records from one table in v1.

Required behavior:

- Reuse existing table query semantics: `--query`, `--fields`, `--limit`,
  pagination controls, and ordering flags where available.
- Default to JSON output; allow CSV when records are flat enough to fit the
  current dynamic CSV formatter.
- Include export metadata such as instance, table, timestamp, query, selected
  fields, and record count.

### `data validate`

Validates a dataset against a target instance before import.

Required behavior:

- Check table existence.
- Check writable and required fields.
- Warn on unsupported or system-managed fields.
- Return a structured validation report without mutating data.

### `data import`

Imports flat datasets with explicit strategy reporting.

Required behavior:

- Prefer Import Set API when the dataset and instance support it.
- Fall back to direct Table API writes only when needed.
- Support create-only in v1.
- Return per-run counts for created, failed, and skipped records.

### `seed plan`

Validates a declarative seed specification and prints the execution plan.

Required behavior:

- Validate syntax and semantics.
- Compute dependency order.
- Estimate record counts.
- Report the selected execution strategy.

### `seed apply`

Creates test data from a declarative seed spec.

Required behavior:

- Preserve declared cross-table relationships.
- Support deterministic generation via a seed value.
- Create a run ID or tag for inspection and cleanup.
- Report the chosen strategy and resulting identifiers.

### `seed cleanup`

Deletes records produced by a prior seed run.

Required behavior:

- Use dependency-safe delete ordering.
- Support `--dry-run`.
- Refuse to delete records not associated with a tracked seed run unless forced.

## Architecture

### Command Layer

Likely files:

- `src/cli/args.rs`
- `src/main.rs`
- `src/cli/commands/import_set.rs`
- `src/cli/commands/table.rs`
- `src/cli/output.rs`

Implementation options:

- Add a new `src/cli/commands/data.rs` module for export, validate, and import,
  while keeping Import Set API internals in `src/cli/commands/import_set.rs`.
- Add a new `src/cli/commands/seed.rs` module for seed planning, apply, and
  cleanup.

This keeps user-facing command orchestration separate from lower-level import
transport details.

### Client Layer

Likely files:

- `src/client/mod.rs`
- `src/client/pagination.rs`

Required additions:

- Shared helpers for reading and validating dataset files.
- Bulk ingest helper functions for Import Set API requests.
- Possibly batch-oriented helpers so large exports/imports do not require one
  fully materialized `Vec<Record>` forever.

### Shared Utilities

Current stdin/body parsing is duplicated across command handlers. Before adding
new import/export entry points, extract shared helpers for:

- reading JSON from `--data` or stdin
- reading file-based JSON payloads
- producing consistent metadata wrappers for exported results

## Artifact Formats

### Flat Dataset Format

The flat dataset is the MVP import/export artifact.

Example:

```json
{
  "version": 1,
  "kind": "table-export",
  "instance": "https://dev12345.service-now.com",
  "table": "incident",
  "query": "active=true",
  "fields": ["number", "short_description", "priority"],
  "exported_at": "2026-03-06T00:00:00Z",
  "record_count": 2,
  "records": [
    {
      "number": "INC0010001",
      "short_description": "Email outage",
      "priority": "1"
    },
    {
      "number": "INC0010002",
      "short_description": "VPN issue",
      "priority": "2"
    }
  ]
}
```

Design notes:

- `version` keeps the format evolvable.
- `kind` distinguishes flat export from future multi-table packages.
- Record field ordering should be deterministic where JSON serialization allows.

### Multi-Table Dataset Manifest

This is a later-phase extension for portable related data.

Example:

```json
{
  "version": 1,
  "kind": "dataset",
  "exported_at": "2026-03-06T00:00:00Z",
  "tables": [
    {
      "name": "cmn_location",
      "file": "cmn_location.json",
      "depends_on": []
    },
    {
      "name": "incident",
      "file": "incident.json",
      "depends_on": ["cmn_location"],
      "references": [
        {
          "field": "location",
          "target_table": "cmn_location",
          "match_key": "name"
        }
      ]
    }
  ]
}
```

Design notes:

- The manifest defines import order and remapping rules.
- Each table file contains records plus source keys used for remapping.
- The format must be stable enough for agents to generate and consume.

### Seed Specification Format

The seed spec describes what to create rather than storing raw exported rows.

Example:

```json
{
  "version": 1,
  "kind": "seed-spec",
  "seed": 42,
  "tables": [
    {
      "name": "cmn_location",
      "count": 2,
      "records": [
        { "name": "Berlin Lab" },
        { "name": "Boston Lab" }
      ]
    },
    {
      "name": "incident",
      "count": 10,
      "template": {
        "short_description": "Generated incident {{index}}",
        "priority": "3"
      },
      "references": {
        "location": {
          "table": "cmn_location",
          "strategy": "round_robin"
        }
      }
    }
  ]
}
```

Design notes:

- The `seed` field makes generation deterministic.
- The spec can mix explicit records and generated templates.
- Relationship definitions are resolved into concrete inserts during planning or
  apply execution.

## Strategy Selection

`data import` and `seed apply` should make strategy selection explicit.

Recommended order:

1. Server-side fast path when a trusted supported mechanism exists.
2. Import Set API for broadly supported bulk ingestion.
3. Direct Table API fallback.

Every successful run should return a JSON field like:

```json
{
  "strategy": "import_set"
}
```

If a faster strategy is unavailable, the result should explain why the command
fell back.

## Output Model

All commands should keep the current CLI convention:

- structured data to stdout
- structured errors to stderr

Suggested top-level success fields:

- `kind`
- `version`
- `command`
- `strategy` when relevant
- `instance`
- `table` or `tables`
- `record_count` or per-table counts
- `run_id` for seed/import runs
- `warnings`
- `result`

## Rollout Plan

### Phase A - MVP data movement

1. Define command surface and file formats.
2. Implement `data export` for a single table.
3. Implement `data validate`.
4. Implement flat `data import`.

### Phase B - Related dataset portability

1. Add manifest-based multi-table export.
2. Add dependency-aware import ordering.
3. Add reference remapping and preview mode.

### Phase C - Test-data seeding

1. Add `seed plan`.
2. Add `seed apply` with strategy selection.
3. Add run tracking and `seed cleanup`.

### Phase D - Reliability and scale

1. Add resumability and status inspection for long-running imports.
2. Improve batching or streaming behavior for larger datasets.
3. Extend support for more complex field types as needed.

## Testing Strategy

Primary files:

- `tests/test_table.rs`
- `tests/test_api_script.rs`
- `tests/test_attachment.rs`
- new `tests/test_data.rs` if coverage outgrows existing suites

Coverage should include:

- single-table export JSON and CSV output
- validation success and blocking validation failures
- import strategy reporting and fallback behavior
- partial import failures with machine-readable summaries
- multi-table ordering and reference remapping
- deterministic seed planning and apply output
- cleanup dry-run and safe deletion behavior

## Open Questions

1. Which server-side fast path is acceptable in scope for v1 seeding, if any?
2. Should attachment export/import be added to the dataset format before v1?
3. Is create-only enough for the first import release, or is limited upsert
   required immediately?
4. Should multi-table artifacts be stored as a directory layout, a single JSON
   bundle, or an archive format?

## Recommended Next Step

Start with the command and artifact design in this document, then implement the
MVP in this order:

1. `data export`
2. `data validate`
3. `data import`
4. `seed plan`
5. `seed apply`
6. `seed cleanup`
