# Errors and retry guidance

Runtime errors are JSON on **stderr**, independently of `--output`:

```json
{"error":{"code":"RATE_LIMITED","message":"Too many requests","class":"api","retry":"safe_reads_only","status":429}}
```

`status` is the HTTP status, not the process exit status. Optional `detail` and
`instance` fields provide bounded, sanitized context; `instance` is an origin
only. Ignore unknown fields and branch on codes/classes, not message wording.
Verbose logs may precede the final JSON error on stderr.

## Exit statuses

| Exit | Meaning |
|---|---|
| 0 | Success (including help/version) |
| 1 | Command failure or unclassified internal failure |
| 2 | Input/format validation, configuration, or clap usage failure |
| 3 | Local credential acquisition failure |
| 4 | HTTP 404 |
| 5 | Other API, GraphQL, or HTTP transport failure |
| 6 | Local I/O failure |
| 7 | Read-only/policy denial |

Runtime classes are `validation`, `config`, `auth`, `policy`, `api`, `transport`,
`io`, `command`, and `internal`. Common codes include `VALIDATION_ERROR`,
`INVALID_JSON`, `CONFIG_INVALID`, `PROFILE_NOT_FOUND`, `INVALID_INSTANCE`,
`AUTH_FAILED`, `REQUEST_TIMEOUT`, `CONNECTION_FAILED`, and `INVALID_RESPONSE`.
HTTP codes such as `INVALID_TABLE`, `RECORD_NOT_FOUND`, `ACL_DENIED`, and
`RATE_LIMITED` retain their existing meanings. HTTP 401 exits 5; local credential
failures exit 3.

Known local validation failures that previously fell back to `INTERNAL_ERROR`
and exit 1 now use validation/config exit 2. Scripts that execute and fail and
imports with row errors still exit 1. A `session switch` whose request, read-back,
or verification fails, whose read-back does not match, or whose context is
incomplete after the request reports `SESSION_SWITCH_UNCERTAIN` (class
`command`, exit 1, `manual_review`), with any classified cause in `detail`; run
`session show` before doing anything else. If the context is already incomplete
before switching, no switch request is sent and the command fails with `VALIDATION_ERROR`
(exit 2). Unclassified command failures retain
`INTERNAL_ERROR`, class `internal`, and unknown retry guidance.

## Retry guidance is not permission to replay

| `retry` | What to do |
|---|---|
| `never` | Fix the input/config/credential/permission issue first. Repeating identical input will not resolve it. |
| `safe_reads_only` | Back off and retry only an operation independently known to be side-effect-free. |
| `manual_review` | Inspect effects first: some records may already have changed. |
| `unknown` | No reliable retry decision is available. Do not blindly replay. |

Timeouts, connection failures, and HTTP 408/429/500/502/503/504 use
`safe_reads_only`. This is **not** a claim that a failed create/update/delete,
import, script, or GraphQL mutation is idempotent or unchanged. Generic GET
endpoints can also have instance-defined side effects. No new automatic retry
loop is introduced; the existing single 401 credential-refresh attempt remains.

## Streams and parser exceptions

Most failures leave stdout empty. Import-set row errors intentionally keep
detailed results on stdout, with `IMPORT_SET_ROW_ERRORS` on stderr. Inspect
those results before retrying. `--allow-row-errors` explicitly accepts partial
failure with a warning and exit 0. Output failures can also happen after data
has been written; nonzero exit never promises that nothing changed.

Clap parsing failures remain human-readable usage diagnostics on stderr with
exit 2, **not JSON**. Help/version remain human output on stdout, with no network
calls. Read-only commands omitted from `snow-cli-ro` are rejected by clap;
`snow-cli --read-only` emits a structured policy error. Invalid argv tokens may
be reflected by clap, so never place credentials in invalid options/values.

Runtime input/config/transport errors withhold payloads, TOML excerpts, URLs,
queries, and private paths. Messages/details are bounded and credential-scrubbed.
Raw HTTP bodies are not normal error diagnostics. Explicit sensitive HTTP debug
is a separate opt-in diagnostic channel and should not be enabled in automation
that records secrets.
