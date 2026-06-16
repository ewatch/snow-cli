# `attachment`

Use `attachment` to work with files attached to ServiceNow records.

```bash
snow-cli attachment <verb> [options]
```

All `attachment` subcommands also accept the global flags from the [command overview](../commands.md).

## `attachment list <table> <sys_id>`

List attachments for a record.

```bash
snow-cli attachment list <table> <sys_id>
```

Arguments:

- `<table>`: table name, for example `incident`
- `<sys_id>`: record `sys_id`

Example:

```bash
snow-cli attachment list incident 46d44a4b2f13000044e0bfc8fb99b6fd
```

The result includes fields such as file name, content type, size, and download link.

## `attachment download <sys_id>`

Download one attachment.

```bash
snow-cli attachment download <attachment_sys_id> [options]
```

Important options:

- `-o, --out <path>`: write to a specific file path

Examples:

```bash
snow-cli attachment download 2d8b4c33db121010f4d224b5ca96198d
snow-cli attachment download 2d8b4c33db121010f4d224b5ca96198d --out incident-log.txt
```

If `--out` is omitted, the CLI tries to use the original attachment filename. If that is not safe or available, it falls back to `<sys_id>.bin`.

## `attachment upload <table> <sys_id> --file <path>`

Upload a local file as an attachment.

```bash
snow-cli attachment upload <table> <sys_id> --file ./report.txt
```

Important options:

- `-f, --file <path>`: local file to upload

Example:

```bash
snow-cli attachment upload incident 46d44a4b2f13000044e0bfc8fb99b6fd --file ./report.txt
```

Notes:

- The CLI reads the file locally and uploads it with the attachment API.
- `--file -` (stdin) is **not** supported — you must provide a real file path.
- Attachments larger than 100 MiB are rejected by the CLI before upload.

## Related pages

- [`table` command reference](./table.md)
