# Query instance analytics

Use `table stats` to answer counting and aggregation questions in a single call,
backed by the ServiceNow Aggregate API. It reads data only, so it is available
in `snow-cli-ro` and under `--read-only`.

## 1. Count a population

Count the records that match an encoded query:

```bash
snow-cli --read-only table stats incident --query 'active=true'
```

## 2. Group and aggregate

Group the same population by a field and compute per-group aggregates. Each group
returns as one row, including its count:

```bash
snow-cli --read-only table stats incident \
  --query 'active=true' \
  --group-by priority \
  --avg reassignment_count
```

`--group-by` accepts a comma-separated list, and you can combine aggregates:

```bash
snow-cli --read-only table stats incident \
  --group-by assignment_group,priority \
  --avg business_duration --max reassignment_count
```

The available aggregates are `--avg`, `--min`, `--max`, and `--sum`, each taking
a comma-separated field list.

## What to look for

- With **no `--group-by`**, the result is a single total for the query.
- With **`--group-by`**, you get one row per group plus its count — useful for a
  breakdown by priority, state, or assignment group.
- Because `table stats` only reads, it is a safe first analytics step before you
  reach for a background script.

## Related pages

- [`table`](/commands/table/) — `table stats` reference and other subcommands
- [Secure read-only usage](/secure-readonly-usage/)
