# Run a background script safely

When the read-only command surface cannot express a task, `script run` sends a
background script to the instance for server-side execution. This is the guarded
escape hatch: it can do anything server-side code can do, so treat it with care.

> **Do not use on production instances.** Background scripts execute arbitrary
> server-side code. The `script` command is excluded from `snow-cli-ro` and is
> not available under `--read-only`. Use a disposable
> [Personal Developer Instance](/pdi-testing/) and review every script before you
> run it.

## Run inline code

Pass the script with `--code`:

```bash
snow-cli script run --code 'gs.info("hello from snow-cli")'
```

A read-only aggregate is a good example of a safe, non-mutating script:

```bash
snow-cli script run --code 'var agg = new GlideAggregate("incident"); agg.addQuery("active", true); agg.addAggregate("COUNT"); agg.query(); var count = 0; if (agg.next()) { count = agg.getAggregate("COUNT"); } gs.info("Active incident count: " + count); count;'
```

## Run from a file

For anything longer than one line, keep the script in a file you can review and
version:

```bash
snow-cli script run --file ./cleanup.js
```

## Choose the scope

By default the script runs in `global`. Target a specific application scope with
`--scope`:

```bash
snow-cli script run --file ./app-fix.js --scope x_my_app
```

## What to look for

- The output is the **server's execution log**, so it reads the same under every
  `--output` format — unlike structured commands.
- Prefer [`table stats`](/guides/instance-analytics/) or a read-only
  [`table list`](/commands/table/) when a script is not strictly necessary; they
  stay inside the read-only surface.
- Reach for `script run` only when no bounded command can express the task, and
  only on an instance you can afford to break.

## Related pages

- [`script`](/commands/script/) — full `script run` reference
- [Testing with a PDI](/pdi-testing/)
- [Secure read-only usage](/secure-readonly-usage/)
