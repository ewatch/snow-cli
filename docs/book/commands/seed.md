# `seed`

`seed` is reserved for declarative test-data workflows.

```bash
snow-cli seed <verb> [options]
```

All `seed` subcommands also accept the global flags from the [command overview](../commands.md).

## Current status

The command surface exists, but the implementation is **not finished yet**. At the moment, these commands return a "planned but not implemented yet" error.

## `seed plan --file <file>`

Intended purpose: validate a seed spec and show the execution plan.

```bash
snow-cli seed plan --file qa-fixture.json
```

Important options:

- `-f, --file <path>`: seed specification file

## `seed apply --file <file>`

Intended purpose: create test data from a seed spec.

```bash
snow-cli seed apply --file qa-fixture.json
```

Important options:

- `-f, --file <path>`: seed specification file

## `seed cleanup <run_id>`

Intended purpose: remove data created by a prior seed run.

```bash
snow-cli seed cleanup <run_id> [options]
```

Important options:

- `--dry-run`: preview what would be deleted
- `--yes`: skip confirmation when deletion becomes available

Example:

```bash
snow-cli seed cleanup run-123 --dry-run
```

## What to use today

Until `seed` is implemented, use:

- [`data`](./data.md) for export and import workflows
- [`table`](./table.md) for direct create/update/delete operations
- [`import-set`](./import-set.md) for staging-table loads
