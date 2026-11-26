# Attachments, Portable Exports & Tracked Seeding

Use when working with record attachments (`attachment`), extracting complete datasets (`data export` / `data export-package`), validating imports (`data validate`), or managing reversible test fixtures (`seed`).

## 1. Attachments (`attachment` — list/download are read-only)

`attachment list` and `attachment download` work in `snow-cli-ro`. `attachment upload` writes to the instance and is blocked in read-only mode.

```bash
# List attachments on a record (bounded to 20 by default; --all fetches all)
snow-cli-ro attachment list incident <record_sys_id>
snow-cli-ro attachment list incident <record_sys_id> --all

# Download an attachment by its attachment sys_id
snow-cli-ro attachment download <attachment_sys_id> --out ./downloaded-log.txt

# Search attachment metadata across records via Table API
snow-cli-ro table list sys_attachment \
  --query 'table_name=incident^file_nameLIKElog' \
  --fields sys_id,table_name,table_sys_id,file_name,content_type,size_bytes,sys_created_on \
  --order-by -sys_created_on --limit 20

# Upload a local file (WRITE OP — only when explicitly asked; max 100 MiB; no stdin `-`)
snow-cli attachment upload incident <record_sys_id> --file ./report.txt
```

## 2. Full dataset exports & portable validation (`data`)

Use `data export` (not `table list`) whenever you need a complete, portable dataset artifact:

```bash
# Archival export (lossless, preserves all returned fields)
snow-cli-ro data export incident --query 'active=true' --fields sys_id,number,short_description,state --order-by -sys_created_on --out incidents.json
snow-cli-ro --output csv data export sys_user --fields sys_id,user_name,email --out users.csv

# Create-only import-ready dataset (strips system-managed, read-only, journal, script, password fields)
# Filter to records that can be created as-is: closed/final task records are usually rejected on create (403)
snow-cli-ro data export incident --import-ready --query 'active=true' --limit 20 --out incidents-import-ready.json

# Multi-table dataset package from a manifest spec
snow-cli-ro data export-package --file dataset-spec.json --out-dir exported-dataset

# Validate an export or package against a destination instance (READ-ONLY)
snow-cli-ro --profile destination data validate --file incidents-import-ready.json
snow-cli-ro --profile destination data validate --file exported-dataset/manifest.json
```

### Guarded import (WRITE OP — always dry-run first, only when explicitly asked)

```bash
# Preview without writing
snow-cli --profile destination data import --file incidents-import-ready.json --dry-run

# Direct create-only Table API import, or route through an Import Set staging table
snow-cli --profile destination data import --file incidents-import-ready.json
# --file must be a `data export --import-ready` artifact (kind: table-export) or a package manifest, not raw JSON rows
snow-cli --profile destination data import --file users-import-ready.json --import-set-table imp_user --fail-on-error
```

## 3. Reversible test fixtures (`seed` — `plan` is read-only; `apply`/`cleanup` are WRITE OPS)

Use `seed` instead of ad-hoc `table create` calls when generating QA/test fixtures so every created record is tracked under a `run-id` and can be cleanly removed:

```bash
# 1. Validate spec and inspect plan (no writes)
snow-cli-ro seed plan --file qa-fixture.json

# 2. Preview apply, then execute when explicitly asked
snow-cli seed apply --file qa-fixture.json --dry-run
snow-cli seed apply --file qa-fixture.json --yes

# 3. Preview cleanup, then remove tracked records
snow-cli seed cleanup <run-id> --dry-run
snow-cli seed cleanup <run-id> --yes
```

If `seed apply` fails part-way, the error keeps the original code (e.g. `SERVER_ERROR`) and its `detail` says how many records were created and gives the exact `seed cleanup` command. If nothing was created, no run is tracked.

## 4. Gotchas

- **`data import` 403 on every record (`IMPORT_RECORD_ERRORS`):** ServiceNow refused the creates (ACL, data policy, or business rule). The usual cause is exporting closed records (`active=false`, final `state`); re-export with `--query 'active=true'`, drop lifecycle fields, or load via `--import-set-table` so a transform map sets the state. The report's `hint` field repeats this.
- **Attachment API ignores `sysparm_fields`:** `snow-cli-ro attachment list <table> <sys_id>` automatically projects the 7 useful fields (`sys_id`, `file_name`, `content_type`, `size_bytes`, `table_name`, `table_sys_id`, `download_link`).
- **`--import-ready` does NOT remap references:** Reference `sys_id`s stay unchanged from the source instance (`reference_policy: source_values_unchanged`). Always run `data validate` on the destination and use `data export-package` when cross-instance reference remapping is needed.
- **Row errors fail the command:** `data import` exits non-zero when any record fails (`IMPORT_RECORD_ERRORS`; `IMPORT_SET_ROW_ERRORS` with `--import-set-table --fail-on-error`), and `import-set load` fails by default (`IMPORT_SET_ROW_ERRORS`). The per-record report is still printed to stdout, so read it before retrying: some records may already exist.
- **Unsupported CSV outputs:** `data export-package`, `data validate`, and `data import` reject `--output csv`; pass `--output json` if CSV is configured as default.
- **`attachment upload` size & stdin limits:** Files over 100 MiB are rejected locally; `--file -` is not supported.
