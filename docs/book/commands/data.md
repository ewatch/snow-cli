# `data`

Use `data` for portable export, validation, and import workflows that sit above the raw Table API.

```bash
snow-cli data <verb> [options]
```

All `data` subcommands also accept the global flags from the [command overview](../commands.md).

## `data export <table>`

Export records from a single table into a portable artifact.

```bash
snow-cli data export <table> [options]
```

Important options:

- `--query <encoded-query>`: restrict exported rows
- `--fields <a,b,c>`: only export selected fields
- `--limit <n>`: maximum records to export
- `--order-by <field>`: sort the export
- `-o, --out <path>`: write to a file instead of stdout

Examples:

```bash
snow-cli data export incident --query 'active=true'
snow-cli data export incident --fields sys_id,number,short_description --limit 50
snow-cli --output csv data export sys_user --fields sys_id,user_name,email --out users.csv
```

Use this command when you want a flat, single-table artifact that can later be validated or re-imported.

## `data export-package --file <spec> --out-dir <dir>`

Export a multi-table dataset package from a manifest spec.

```bash
snow-cli data export-package --file dataset-spec.json --out-dir exported-dataset
```

Important options:

- `-f, --file <path>`: dataset export spec file
- `--out-dir <dir>`: destination directory for the manifest and exported table files

Examples:

```bash
snow-cli data export-package --file dataset-spec.json --out-dir exported-dataset
snow-cli data validate --file exported-dataset/manifest.json
snow-cli data import --file exported-dataset/manifest.json
```

Note: `data export-package` does not support `--output csv`.

## `data validate --file <file>`

Validate an export artifact or dataset package against the target instance.

```bash
snow-cli data validate --file export.json
```

Important options:

- `-f, --file <path>`: file to validate

The validation report checks schema compatibility and reports errors and warnings before you try an import.

Note: `data validate` does not support `--output csv`.

## `data import --file <file>`

Import a flat export artifact or dataset package.

```bash
snow-cli data import --file export.json [options]
```

Important options:

- `-f, --file <path>`: file to import
- `--dry-run`: preview the import plan without creating records
- `--import-set-table <table>`: use the Import Set API for a flat table-export artifact
- `--fail-on-error`: exit non-zero when Import Set responses contain row-level errors

Examples:

```bash
snow-cli data import --file users.json
snow-cli data import --file users.json --dry-run
snow-cli data import --file users.json --import-set-table imp_user
snow-cli data import --file users.json --import-set-table imp_user --fail-on-error
```

Notes:

- `--import-set-table` currently works only for flat table-export artifacts, not multi-table package imports.
- When `--import-set-table` is not used, the CLI falls back to direct create-only Table API writes.
- `--fail-on-error` is mainly useful together with `--import-set-table`.
- `data import` does not support `--output csv`.

### Real-world export → validate → import workflow

```bash
# 1. Export records to a file
snow-cli data export incident \
  --fields number,short_description,priority,state \
  --limit 10 \
  --out export-incidents.json

# 2. Validate the exported file against the target instance
snow-cli data validate --file export-incidents.json

# 3. Preview what an import would do (dry-run)
snow-cli data import --file export-incidents.json --dry-run

# 4. Import (creates new records via Table API)
snow-cli data import --file export-incidents.json
```

The validation step checks that the fields in the export exist on the target
table and reports any mismatches before you attempt the import.

## When to use `data` instead of `table`

Use `data` when you want:

- a portable export artifact,
- schema validation before import,
- a repeatable dataset package,
- a dry-run preview of an import,
- an Import Set-backed import path.

Use [`table`](./table.md) for direct record-by-record CRUD operations.

## Related pages

- [`table` command reference](./table.md)
- [`import-set` command reference](./import-set.md)
