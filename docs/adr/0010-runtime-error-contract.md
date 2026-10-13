# ADR-0010: Runtime Error Classification and Conservative Retry Guidance

## Status

Accepted

## Context

ADR-0005 established JSON stderr errors but did not specify a full taxonomy.
Typed API, GraphQL, policy, and command failures coexisted with untyped local
validation, configuration, credential, and HTTP transport failures. TOML and
HTTP library Display text could expose source excerpts or request URLs (including
queries). A transient failure alone does not establish that replaying a command
is safe: a timed-out create or script may have already changed the instance.

## Decision

### Runtime envelope and compatibility

Preserve `error.code`, `message`, and optional `status`, `detail`, and `instance`.
Add mandatory `class` and `retry` fields to runtime error objects. Consumers must
ignore unknown fields and use codes/classes, not message text, for branching.
The error's status is the HTTP status when known; the process exit code is a
separate contract. Codes are nonempty uppercase ASCII letters/digits/underscores,
at most 64 bytes; malformed external codes become `UNKNOWN_ERROR`. Runtime failures are JSON on stderr regardless of `--output`.
Logs may precede the final JSON error when verbosity is requested.

| Class | Representative codes | Exit |
|---|---|---|
| `validation` | `VALIDATION_ERROR`, `INVALID_JSON` | 2 |
| `config` | `CONFIG_NOT_FOUND`, `CONFIG_INVALID`, `PROFILE_NOT_FOUND`, `INVALID_INSTANCE` | 2 |
| `auth` | `AUTH_FAILED` (local credential acquisition); existing auth codes retained | 3 |
| `policy` | `POLICY_DENIED` and existing policy codes | 7 |
| `api` | `BAD_REQUEST`, `UNAUTHORIZED`, `FORBIDDEN`, `ACL_DENIED`, `INVALID_TABLE`, `RATE_LIMITED`, `SERVER_ERROR`, `UNKNOWN_ERROR` | 5 |
| `api` | HTTP 404: `NOT_FOUND`, `RECORD_NOT_FOUND` | 4 |
| `api` | `GRAPHQL_ERROR` | 5 |
| `transport` | `REQUEST_TIMEOUT`, `CONNECTION_FAILED`, `INVALID_RESPONSE`, `TRANSPORT_ERROR` | 5 |
| `io` | `IO_ERROR` | 6 |
| `command` | `IMPORT_SET_ROW_ERRORS`, `SCRIPT_EXECUTION_FAILED`, existing SN-Utils broker codes | 1 |
| `internal` | `INTERNAL_ERROR` for unclassified failures | 1 |

HTTP 401 remains API exit 5, not local auth exit 3. Config and auth typed sources
retain their classification through anyhow context; HTTP/GraphQL causes take
precedence over the generic auth context on OAuth requests. Transports remain
behind `src/client/`, including classification of HTTP library errors.

Known local input rejection now uses exit 2 instead of the former generic exit 1:
invalid/missing table or import payloads, CSV/preview incompatibility,
non-interactive delete confirmation, API header/body validation, GraphQL
variables/document validation, script input, stdin limits, and dataset input
syntax/shape or flat-import validation. This intentional reclassification does
not change execution-error exits (including row failures and script evaluation).
Dataset-specific itemized validation details remain a separate issue (#8.8).

Other command-specific untyped failures conservatively retain `INTERNAL_ERROR`,
class `internal`, exit 1, and unknown retry guidance. Do not infer a programming
bug or transient failure from that fallback. New known validation failures must
use `models::validation::ValidationError`, not string matching in the renderer.

### Retry guidance

`retry` is an enum, NOT a `retryable: true` permission or an automatic retry:

- `never`: identical input cannot resolve the rejection. Fix input, config,
  credentials, or permissions before attempting a new command.
- `safe_reads_only`: back off and retry only an operation the caller independently
  knows is side-effect-free. Used for timeouts, connection failures, and HTTP
  408/429/500/502/503/504. It does **not** declare the failed operation idempotent,
  guarantee no server-side effects, or authorize replay of a mutation.
- `manual_review`: inspect effects before attempting anything further. Used for
  script execution, row-level import errors, and GraphQL errors (which may follow
  mutations with partial effects). No rollback or convergence is implied.
- `unknown`: there is insufficient evidence for a retry decision. Used for other
  transport/response/I/O/fallback/SN-Utils failures and unspecified HTTP statuses.

There is no new retry loop, delay schedule, or Retry-After promise. The existing
single 401 credential-refresh attempt is unchanged. In particular, safe read
semantics are not inferred solely from an endpoint using HTTP GET, or from the
binary name. Generic API requests can have instance-defined behavior.

### Sensitive diagnostic boundary

Local JSON and TOML failures discard source excerpts and parser Display text.
Config URL errors never echo the supplied URL. HTTP errors discard transport
Display text, URLs, request parameters, scripts, and payloads. Response decode
failures are distinguished from invalid local JSON. I/O failures expose only the
OS error kind, not file paths or their contents.

Standard ServiceNow/GraphQL diagnostic envelopes retain the existing bounded,
scrubbed message policy; unknown response bodies stay redacted. All rendered
messages/details (including legacy fallback and broker text) are scrubbed for
credential-like key/value patterns, stripped of control characters, and bounded
to 500 Unicode scalar values plus a truncation suffix. Diagnostics with explicit
query/script markers are withheld entirely rather than partially scrubbing
multi-token payloads. API `instance` is origin only. This remains in effect when sensitive HTTP debug is enabled; that explicit
opt-in debug channel is not part of the normal diagnostic contract.

Scrubbing is defense-in-depth, not proof arbitrary text is safe. New failure
sites must not insert source bodies, scripts, queries, credential values, or
untrusted local parse messages into authored diagnostics. Registered error types
retain safe guidance or classifications rather than raw source data.

### Parser and discovery exception

Explicitly retain clap's existing human-readable stderr usage diagnostics and
exit 2 for parsing failures, including missing commands, invalid flags/values,
conflicts, and mutation commands omitted by `snow-cli-ro`. These are **not** JSON
runtime errors and have no `class`/`retry` fields. Help/version remain successful
human output on stdout. Neither parsing nor discovery loads config, authenticates,
or contacts the instance. The parser can reflect invalid argv tokens: callers
must not put credentials into invalid options or values. Runtime redaction does
not claim to sanitize clap output. Actionable/structured parse-error changes are
tracked in #120.5, not silently bundled into this runtime contract change.

### Stdout exceptions

Errors normally leave stdout empty. An import-set load with row errors retains
its detailed result on stdout and emits the runtime error on stderr; some rows
may have succeeded. Other output/write failures can occur after a command has
already produced data. Nonzero exit means consumers must inspect outcomes; it
never promises that stdout is empty or the instance is unchanged.

## Alternatives Considered

- A retryable boolean: rejected because it encourages unsafe mutation replay.
- Classify errors by matching message strings: rejected as unstable and liable
  to misclassify private text. Use typed sources and preserve source chains.
- Replace clap diagnostics in this slice: deferred to #120.5 to avoid changing
  human help/usage compatibility while defining the runtime contract.
- Expose TOML excerpts and request URLs after scrubbing: rejected; withholding
  source text is safer than trying to recognize every possible secret.

## Consequences

Agents gain stable classes, exits, and conservative retry guidance without
changing successful result schemas. Some previously generic failures now exit
2/3/5/6 and carry specific codes. Legacy unclassified command failures remain
conservative and can be migrated incrementally. Tests in
`tests/test_error_contract.rs`, `tests/test_response_contract.rs`, and
`src/error.rs` enforce classifications, source preservation, stream separation,
redaction, bounded diagnostics, and partial-mutation behavior.
