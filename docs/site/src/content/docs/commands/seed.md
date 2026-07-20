# `seed`

> **Planned, not yet implemented.** The entire `seed` command group is a
> planned interface. `seed plan`, `seed apply`, and `seed cleanup` are all
> visible in `--help`, but every one of them currently exits 1 with
> `planned but not implemented yet` when run. The command blocks and examples
> below describe the intended interface only — treat them as a design
> preview, not as runnable commands, until this page is updated.

`seed` is reserved for declarative test-data workflows.

```bash
snow-cli seed <verb> [options]
```

All `seed` subcommands also accept the global flags from the [command overview](/commands/).

## Current status

The command surface exists, but the implementation is **not finished yet**. At the moment, these commands return a "planned but not implemented yet" error.

## `seed plan --file <file>` (planned)

Intended purpose: validate a seed spec and show the execution plan.

```bash
# Illustrates the planned interface — not runnable yet
snow-cli seed plan --file qa-fixture.json
```

Important options:

- `-f, --file <path>`: seed specification file

## `seed apply --file <file>` (planned)

Intended purpose: create test data from a seed spec.

```bash
# Illustrates the planned interface — not runnable yet
snow-cli seed apply --file qa-fixture.json
```

Important options:

- `-f, --file <path>`: seed specification file

## `seed cleanup <run_id>` (planned)

Intended purpose: remove data created by a prior seed run.

```bash
snow-cli seed cleanup <run_id> [options]
```

Important options:

- `--dry-run`: preview what would be deleted
- `--yes`: skip confirmation when deletion becomes available

Example:

```bash
# Illustrates the planned interface — not runnable yet
snow-cli seed cleanup run-123 --dry-run
```

## What to use today

Until `seed` is implemented, use:

- [`data`](/commands/data/) for export and import workflows
- [`table`](/commands/table/) for direct create/update/delete operations
- [`import-set`](/commands/import-set/) for staging-table loads
