# Plan — servicenow-cli-78: opt-in GraphQL query command

## 1. Problem & goal

Table API reads can require multiple requests and client-side joins to obtain a narrow, related-record projection. Add the explicitly optional `snow graphql` command so an agent can submit a Now GraphQL document to `POST /api/now/graphql` and receive the selected JSON payload.

The command is an addition, not a replacement for `table`: Now GraphQL must be enabled by the instance administrator and callers must supply schema-correct documents. It must preserve authenticated same-origin HTTPS transport, stdout-only structured successes, and structured stderr errors. In this first slice, `snow-cli-ro` and `snow-cli --read-only` will deny **all** GraphQL invocations because GraphQL POST may contain a mutation; this deliberately avoids claiming that an ad-hoc lexical check proves a document safe.

Example target workflow:

```sh
snow graphql --query-file incident.graphql \
  --variables '{"number":"INC0010001"}'
```

where the document selects the incident number, caller name, and assignment-group manager in one result.

## 2. Current state in code (cited files)

- The full command surface is `Commands` in `src/cli/args.rs:133`; raw REST is currently modeled separately by `ApiArgs` / `ApiCommands` at `src/cli/args.rs:1105-1111`. `ApiCommands::Post` shows the existing optional inline-body convention, but there is no GraphQL command or document-source argument type.
- `run_parsed_cli` checks the command policy before profile/config/auth resolution (`src/lib.rs:47-51`), then dispatches `Commands::Api` at `src/lib.rs:180-188`. `command_uses_connection` separately drives the profile hint and must include each remote command (`src/lib.rs:220-235`).
- `src/cli/commands/mod.rs:1` is the explicit handler-module registry. `src/cli/commands/api.rs:7-114` is the nearest transport/output precedent: it builds a configured client, reads an inline body or bounded stdin body, and renders an HTTP response. Its `read_data_from` at `src/cli/commands/api.rs:117-151` and `src/cli/io.rs:3-22` establish the 10 MiB bounded-stdin behavior; `script.rs` additionally establishes the file/inline/stdin precedence pattern.
- `SnowClient::post` in `src/client/mod.rs:963-965` enters the shared `request_inner` path (`src/client/mod.rs:1009-1111`). That path calls `ExecutionPolicy::ensure_request_allowed` before resolving the authenticated URL (`src/client/mod.rs:1017`), adds authentication headers, logs with redaction, enforces same-origin HTTPS through `authenticated_url`, and maps non-2xx statuses to `ApiError`. A GraphQL-specific client method can therefore reuse all transport safeguards without duplicating them.
- The current request backstop allows only GET in read-only mode (`src/policy.rs:117-133`), and command classification is exhaustive in `read_only_command_decision` (`src/policy.rs:181-338`). Thus even a command-level allowance for a GraphQL `query` would still fail at its POST request unless the lower-level policy were also redesigned.
- The reduced `snow-cli-ro` grammar is distinct (`src/cli/readonly_args.rs:50-89`) and maps only advertised commands back to full commands (`src/cli/readonly_args.rs:586-615`). Omitting GraphQL from that grammar is the correct expression of the chosen deny-all read-only policy.
- HTTP failures are centrally converted to `ApiError` in `src/client/error.rs:12-34` and then structured stderr JSON by `write_anyhow_error_and_exit_code` in `src/error.rs:55-100`. A successful GraphQL HTTP response can still carry an `errors` array, so it needs an equivalent typed error routed through this renderer rather than being printed as a successful raw body.
- `output::print_output` in `src/cli/output.rs:10-37` already formats arbitrary serializable JSON values consistently for JSON, CSV, JSONL, TOON, Text, and Auto. The command can pass the successful GraphQL JSON `data` value through this output module.
- Mock-backed command integration tests use an isolated API-key profile and `wiremock`; see `tests/test_api_script.rs:1-47` and POST/stdin coverage at `tests/test_api_script.rs:100-160`. No real instance is needed.
- The command-extension workflow requires args, handler registration, dispatch, policy classification, and tests (`docs/guides/adding-commands.md:5-55`). ADR-0002 requires a noun-verb surface, while this issue explicitly specifies the single-action `snow graphql` spelling; document this intentional exception/implicit `query` verb in help rather than creating an unrelated alternate command shape.

## 3. Proposed design in deep-module terms

### External seam: GraphQL execution

Create a `graphql` **module** whose external **interface** is one handler entry point accepting parsed `GraphqlArgs`, resolved profile/instance/timeout, and output format. Callers only need to know three document sources (positional document, `--query`, or `--query-file`; otherwise non-TTY stdin), optional JSON variables, and that a successful request emits the GraphQL `data` JSON to stdout. They do not learn request construction, authentication, response-envelope parsing, or GraphQL-error sanitization.

The module owns source resolution with this unambiguous precedence/validation contract:

1. Exactly one explicit source may be supplied: positional document, `--query <DOCUMENT>`, or `--query-file <PATH>`; Clap rejects combinations.
2. With no explicit source, read a non-empty document from bounded stdin; on a TTY or blank stdin, give an actionable source error.
3. Read a query file asynchronously (`tokio::fs::read_to_string`) and reject empty/whitespace-only content with the source named in the error.
4. Parse `--variables` at the CLI **interface** with `serde_json`; require a JSON object because GraphQL variables are a name-to-value map, defaulting an omitted value to `{}`. Reject malformed JSON or non-object JSON before any client is built or HTTP request is sent.

This is a deep module: the small caller interface exposes document + variables, while its implementation absorbs file/stdin limits, JSON validation, request shape, response decoding, and safe error extraction. It gains **depth** by giving callers a single operation that exercises related-record traversal without exposing the Table API's pagination/projection implementation. That depth provides **leverage** to every agent needing exact nested fields and **locality** by keeping GraphQL-specific semantics out of generic raw-API and Table modules.

### Transport seam: `SnowClient`

Add a focused `SnowClient` GraphQL method, e.g. `execute_graphql(&mut self, query: &str, variables: &serde_json::Map<String, Value>) -> anyhow::Result<Value>`. Its **interface** accepts already validated values and returns only GraphQL `data`; it must not expose a generic GraphQL response envelope. Its **implementation** serializes `{"query": query, "variables": variables}`, calls the existing authenticated POST path at the fixed `/api/now/graphql` path, parses JSON, and detects `errors` even on HTTP 200.

The shared authenticated-request **seam** remains `request_inner`; the new client method is an **adapter** over that seam, not a second HTTP implementation. This preserves existing auth refresh, request-body redaction, same-origin HTTPS enforcement, spinner behavior, and HTTP-status mapping. The method is deep because one focused interface contains the endpoint path, request envelope, response decoding, and response-level error handling. Do not create a generic GraphQL transport abstraction: there is one endpoint and one adapter today, so a broader seam would be hypothetical and shallow.

Define a typed `GraphqlError` beside `ApiError` in `src/client/error.rs`. On a valid JSON envelope with a non-empty `errors` array, it should retain only bounded `errors[*].message` strings (and a fixed `GRAPHQL_ERROR` code/message); never carry the raw body, `extensions`, paths, locations, query text, or partial `data`. The client returns this error rather than outputting partial `data`. Extend `src/error.rs` to downcast and serialize this error through the existing `ErrorOutput` path with nonzero exit status. This provides a small error **interface** to callers and centralizes safe rendering; no handler-local `eprintln!` or raw response dump is permitted.

### Policy seam

Add `Commands::Graphql(_)` to the full command policy matrix and deny it in `PolicyMode::ReadOnly` as `RemoteWrite`, with an explicit reason that GraphQL documents can contain mutations and the endpoint requires POST. Keep `ensure_request_allowed` GET-only and do not add GraphQL to `ReadOnlyCommands`; `snow-cli-ro graphql` will consequently be unavailable at parsing/help level, while `snow-cli --read-only graphql ...` reaches the central command-policy denial before config/auth/network work. This yields a conservative, real seam rather than a fragile query classifier plus a special POST bypass.

## 4. Step-by-step implementation

1. **Define the command grammar in `src/cli/args.rs`.** Add a `Graphql(GraphqlArgs)` top-level `Commands` variant with help text stating it is optional and requires Now GraphQL to be enabled. Add `GraphqlArgs` containing:
   - optional positional `document`,
   - `--query <DOCUMENT>`,
   - `--query-file <PATH>` (a `PathBuf`), and
   - `--variables <JSON>`.
   Put the three explicit document inputs in one Clap mutually exclusive group. Do not make that group required so piped stdin remains supported. Add concise after-help examples for inline, file, variables, and stdin use.
2. **Add the handler module and dispatch.** Register `pub mod graphql;` in `src/cli/commands/mod.rs`. In `src/lib.rs`, add a `Commands::Graphql` match arm passing profile, effective output format, instance override, and timeout to `graphql::handle`; add it to `command_uses_connection`.
3. **Implement CLI-side source/variables resolution in `src/cli/commands/graphql.rs`.** Keep helper functions private and unit-testable: resolve positional/flag/file/stdin to a non-empty string, using `DEFAULT_MAX_STDIN_BYTES` for stdin; parse variables to `serde_json::Map<String, Value>` before network construction; produce contextual `anyhow` errors for unreadable files, empty documents, malformed JSON, and non-object variables. Log only document length and whether variables were supplied—never the document or variable values.
4. **Implement the focused client method in `src/client/mod.rs`.** Serialize the request envelope with `serde_json` (not string interpolation), POST it through `self.post`, parse the successful response as JSON, and delegate envelope interpretation to a private helper or the typed error constructor. Require an object envelope: return `data` when no GraphQL errors exist; return typed `GraphqlError` when `errors` is non-empty; reject malformed/unexpected successful envelopes as a normal contextual error without echoing bodies. Preserve `data: null` as a valid successful GraphQL value when `errors` is absent.
5. **Centralize GraphQL errors.** Add `GraphqlError` and bounded typed message extraction in `src/client/error.rs`; extend `src/error.rs` so it produces the existing `{ "error": { ... } }` shape on stderr, code `GRAPHQL_ERROR`, a stable summary message, sanitized message detail, no raw body, and a nonzero general/API failure exit code. Ensure a server HTTP error remains the existing `ApiError` path.
6. **Finish the handler.** Build the client only after input validation; call the focused client method; send its returned JSON value through `output::print_output`. This keeps successful structured output on stdout across all configured formats.
7. **Classify policy and read-only grammar.** Import `GraphqlArgs`/the variant where needed, add the full-command deny decision and matrix test in `src/policy.rs`, and deliberately make no addition to `src/cli/readonly_args.rs`. Add a read-only help/parse test proving GraphQL is absent from `snow-cli-ro`, and a full-command test proving `--read-only` receives `POLICY_DENIED` before attempting a request.
8. **Document the opt-in prerequisite in `AGENTS.md`.** Add a short durable command note near the architectural/command guidance: `snow graphql` is optional, uses `/api/now/graphql`, requires an administrator to enable Now GraphQL on the target instance, and is excluded from read-only mode because GraphQL may mutate. Do not imply schema discovery or automatic enablement.
9. **Validate and format.** Run `cargo fmt`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, plus `cargo run --bin snow-cli -- graphql --help` and `cargo run --bin snow-cli-ro -- --help` to inspect the advertised surfaces.

## 5. Testing strategy

- **Args/help unit coverage (`src/cli/args.rs` or existing CLI tests):** parse positional, `--query`, and `--query-file`; reject multiple explicit sources; verify help names every source and the enablement prerequisite. Verify the flat, issue-required `snow graphql` spelling.
- **Handler unit coverage (`src/cli/commands/graphql.rs`):** exercise pure source helpers with `Cursor`/temporary files: each source, stdin fallback, TTY/no-input, blank input, file I/O failure, and stdin size limit. Exercise variables omission (`{}`), valid nested object, malformed JSON, and valid-but-non-object JSON. Assert validation fails before the client invocation boundary.
- **Client unit tests (`src/client/mod.rs` and `src/client/error.rs`):** with `wiremock`, assert POST path `/api/now/graphql`, `Accept`/JSON content type/auth inherited from the shared request path, and exact serialized `{query,variables}` envelope. Assert same-origin/auth behavior remains inherited rather than reimplemented. Cover HTTP non-2xx mapping to `ApiError`, valid `{ "data": ... }`, valid `{ "data": null }`, malformed success JSON/envelope, and a 200 response with `errors` becoming `GraphqlError` without retaining raw `extensions` or response text.
- **Error-rendering unit tests (`src/error.rs`):** downcast a `GraphqlError` through `write_anyhow_error_and_exit_code`/its testable rendering extraction and assert `GRAPHQL_ERROR`, typed message detail, no raw envelope/extension/query material, and nonzero exit status.
- **Policy unit tests (`src/policy.rs`):** full access allows `Commands::Graphql`; read-only returns a deny with `RemoteWrite`; retain the GET-only request backstop test. This is the required policy matrix, and explicitly demonstrates that no mutation can reach the POST endpoint in read-only mode.
- **Integration tests (new `tests/test_graphql.rs`, following `tests/test_api_script.rs`):** use the API-key temp config and wiremock to cover inline query + variables, `--query-file`, and stdin, asserting stdout contains only returned `data`. Assert malformed variables and absent/empty source send no request. For a 200 GraphQL `errors` response, assert failure, structured stderr `GRAPHQL_ERROR`, no raw errors envelope, and empty stdout. Add `snow-cli --read-only graphql ...` denial and `snow-cli-ro graphql` unknown-subcommand coverage.

## 6. Prototype?

**No.** This is neither a state-model question nor a visual question. The uncertain behavior is bounded request/response and policy semantics with established mock-based seams, so focused unit and wiremock integration tests answer it more directly than throwaway logic or UI code. A live instance is not a prototype requirement; the documented Now GraphQL enablement/schema prerequisite remains an operational integration concern.

## 7. Risks & open questions

- **Read-only usability vs. safety:** denying all GraphQL requests is conservative but prevents legitimate queries in `snow-cli-ro`. A future change should introduce a real GraphQL parser and a deliberately designed privileged POST policy seam before allowing query operations; a trim/regex check cannot prove safety across comments, fragments, multiple operations, or operation selection.
- **CLI-shape exception:** `snow graphql` has an implicit query action, whereas ADR-0002 and the command reference describe noun-verb commands. The issue explicitly requires that spelling; confirm maintainers accept the intentional exception rather than prefer `snow graphql query`. Do not support both without a product decision, because aliases broaden the interface and documentation burden.
- **Response contract:** this plan treats any non-empty `errors[]` as failure and suppresses partial `data`. Confirm whether callers instead need partial GraphQL data with an explicit warning/envelope; returning partial output with exit success would violate the stated centralized-error goal and make automation ambiguous.
- **Variables contract:** this plan requires a JSON object and sends `{}` when omitted. Confirm the Now endpoint accepts that conventional GraphQL representation (rather than omitting `variables`) if instance-specific behavior differs.
- **Error disclosure:** GraphQL error messages may themselves expose schema or ACL information. The implementation must bound and type-filter messages, never dump raw bodies, and should retain existing debug-body redaction defaults.
- **Instance availability:** endpoint availability, feature/plugin naming, schema content, ACLs, and response extensions vary by ServiceNow release and configuration. The command must diagnose HTTP 404/403 through the existing centralized error path and documentation must clearly say it does not enable GraphQL or discover schemas.
