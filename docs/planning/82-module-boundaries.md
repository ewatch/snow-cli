# Plan: Enforce module boundaries mechanically

## 1. Problem & goal

The intended dependency direction is `models -> config -> auth -> client -> cli/commands`, with `policy` and `snu` cross-cutting. It is currently convention rather than a mechanically enforced interface. A command can import `reqwest`, construct a client, or use `SnowClient::http()` to bypass the request-policy, authentication, URL-validation, retry, debug-redaction, and error-mapping implementation in `src/client/mod.rs`.

The goal is to make the client module the single HTTP seam and make process termination and active-policy mutation entry-point-only operations. CI must reject regressions with an actionable rule message, while intentional URL parsing and protocol modelling remain possible without importing `reqwest` outside the client implementation.

## 2. Current state in code (cited files)

- `Cargo.toml:29` owns the sole `reqwest` dependency, but no lint configuration file exists. CI already runs `cargo clippy --all-targets -- -D warnings` and `cargo test` (`.github/workflows/ci.yml:41-45`), so both enforcement mechanisms will run without a workflow change.
- `SnowClient` is the right central module: it snapshots the active policy while building (`src/client/mod.rs:27-51`), stores it (`:525-544`), and calls `ensure_request_allowed` before resolving and sending every normal request (`:1007-1019`). Its authenticated URL validation rejects unsafe and cross-origin targets (`:593-629`).
- Its current interface leaks its implementation: `SnowClient::http()` returns `&reqwest::Client` (`src/client/mod.rs:674-676`), `authenticator()` exposes the authentication adapter (`:679-681`), and normal request methods return `reqwest::Response` (`:949-992`).
- That escape hatch is already used in command implementations. Attachment upload builds multipart HTTP directly and sends it through `.http()` (`src/cli/commands/attachment.rs:146-174`); attachment download does the same (`:224-240`). Script execution builds and executes form/JSON requests directly (`src/cli/commands/script.rs:189-200` and `:227-238`). These paths can evade the per-request policy check at the client seam.
- Other non-client modules mention `reqwest`: raw API command method/response types (`src/cli/commands/api.rs:27,46,70,92,156`); multipart types (`src/cli/commands/attachment.rs:4`); URL parsing/fetching in the skill installer (`src/cli/commands/skills.rs:93,222,240-242,450-457`); OAuth creates a `reqwest::Client` itself (`src/auth/oauth2.rs:504-523`); and config, script, and SN-Utils use `reqwest::Url` for parsing (`src/config/profile.rs:328`, `src/cli/commands/script.rs:325`, `src/snu/protocol.rs:75`).
- Policy is correctly checked before command dispatch in `src/lib.rs:48-50`, but some handlers re-read the process global (`src/cli/commands/attachment.rs:107` and `src/cli/commands/script.rs:102`). `set_active_policy` is called by the two CLI entry paths (`src/lib.rs:28,42`) and command-local tests (`src/cli/commands/attachment.rs:306,320`; `src/cli/commands/script.rs:892,913`).
- Process termination is currently confined to the binary roots (`src/main.rs:7`, `src/bin/snow-cli-ro.rs:7`), but this is not enforced.
- `src/cli/validation.rs`, named in the planning brief, does not exist in the current tree; argument validation remains in `src/cli/args.rs` and command-specific modules.

## 3. Proposed design in deep-module terms (interfaces, seams, what depth is gained)

### HTTP module and seam

Make `src/client/` the external HTTP seam. Its module interface should express ServiceNow and explicitly supported external-fetch operations in domain terms, not expose `reqwest` builders, clients, requests, multipart forms, or responses. Keep the `reqwest` implementation, request construction, auth-header application, policy enforcement, redirect/session handling, debug redaction, retries, and response decoding behind that module.

Replace the shallow `http()`/`authenticator()` escape-hatch interface with small operations needed by current callers, for example: an authenticated form-post operation for background scripts; authenticated attachment upload/download operations; an opaque response/body result used by the raw API command; and a narrowly named unauthenticated/external form or GET operation for OAuth token exchange and skill-manifest/file retrieval. The exact result types should be client-owned (status, final URL where required, headers only where a caller has a demonstrated need, and body/stream), rather than `reqwest::Response`.

Every operation that can reach the ServiceNow instance must enter the common policy-checking request implementation before network I/O. The external OAuth and skill-fetch operations need explicitly named client operations so their different trust/policy semantics are reviewable rather than hidden in a generic escape hatch. Preserve URL parsing outside this seam through the direct `url` crate (or small client-owned parsing helpers), and use the existing direct `http` dependency for `Method`; neither requires importing `reqwest`.

This deepens the client module: command modules learn a small task-level interface and gain leverage from one implementation of credentials, policy, origin safety, logging, retry/error translation, and HTTP configuration. It improves locality: a security fix belongs in the client implementation rather than being replicated across attachment and script handlers. The deletion test is decisive here: removing the client must cause the authentication/policy/request complexity to reappear in every command, proving it is not a pass-through.

### Policy module and seam

`policy` remains the module whose interface decides command and request permission. `set_active_policy` is process initialization, not a command interface: only the `src/lib.rs` CLI launch paths may call it; `src/policy.rs` owns its implementation. Command implementations receive enforcement through the already-configured `SnowClient` and command dispatch rather than reading the global themselves. Move any test-only global-policy setup behind a `#[cfg(test)]` helper owned by `policy`, so test callers no longer invoke the setter directly and reset state safely.

This places the active-policy mutation seam at initialization, eliminates duplicated command checks, and makes client-held `ExecutionPolicy` the single policy adapter for outbound instance calls.

### Mechanical guards

Use two complementary mechanisms:

1. `clippy.toml` makes constructing/using `reqwest::Client` and calling direct convenience HTTP functions disallowed by default, with a reason directing authors to the client module. Add narrowly justified `#[allow(clippy::disallowed_types, reason = "...")]` only within the client implementation for its deliberate adapter role. Disallow `std::process::exit` outside binary roots via the structural guard (and configure the Clippy method rule where it can provide an early diagnostic).
2. `tests/structure.rs` walks tracked Rust source under `src/` and applies path-aware lexical rules Clippy cannot express: no `reqwest` token outside `src/client/`; no `SnowClient::http`/`.http()` escape hatch; no `set_active_policy(` call outside `src/lib.rs` and `src/policy.rs`; and no `std::process::exit` outside `src/main.rs` and `src/bin/snow-cli-ro.rs`. Failures must report the relative file, line, forbidden form, allowed locations, and the intended module interface.

The structural test is intentionally a guard on the seam, not a parser or a second compiler. It should inspect only Rust source, skip generated/build directories, tolerate comments/strings only if a simple token rule proves too noisy, and include its own fixture-independent assertions against the real tree. Keep the allowed-path list explicit and small so a new exception is a reviewed design decision, not an accidental convention.

## 4. Step-by-step implementation

1. Add a short **Module boundaries** subsection to `AGENTS.md` after Architectural Invariants. State the client HTTP seam, the dependency direction, that commands must use the client interface rather than `reqwest` or `SnowClient::http()`, that policy initialization belongs in `lib.rs`, and that new exceptions require updating the structural test and rationale.
2. Inventory every production `reqwest` reference with `rg -n "reqwest" src --glob '*.rs'`; classify it as HTTP transport, HTTP method/response leakage, or URL parsing. Establish the target allow-list as `src/client/**` only. Do not blanket-suppress findings.
3. Deepen `src/client/mod.rs` (and split private helpers into client submodules only if that makes the implementation clearer): remove public `http()` and the public authenticator escape hatch; introduce client-owned request/result types and focused operations for raw custom-header requests, form-script execution, attachment upload/download, OAuth form exchange, and external skill fetches. Route ServiceNow operations through the existing policy/origin/auth/logging implementation. Maintain current stdout/stderr behaviour in command handlers and preserve streaming for attachment download so large files are not buffered unnecessarily.
4. Refactor `src/cli/commands/attachment.rs` and `src/cli/commands/script.rs` to call those focused client operations. Delete their direct global-policy reads; command dispatch plus the configured client policy is the enforcement path. Retain command-specific validation, file-size limits, progress display, and response presentation at the CLI interface.
5. Refactor `src/cli/commands/api.rs` to use `http::Method` and client-owned response data, so its output formatter does not name `reqwest::Response`. Refactor OAuth and remote skill loading to call the dedicated client operations. Replace the remaining `reqwest::Url` parsing in config, script, and SN-Utils with `url::Url` (adding a direct `url` dependency if necessary) or a purpose-built client-free parser. Verify no module outside `src/client/` names `reqwest` afterward.
6. Consolidate active-policy mutation: retain the two production calls in `src/lib.rs:28,42`; move test setup/reset behind a policy-owned `#[cfg(test)]` helper or redesign those tests to inject `ExecutionPolicy`, then remove command-local setter calls. Confirm `set_active_policy` has no other source callers.
7. Create `clippy.toml` with documented `disallowed-types` and `disallowed-methods` entries for direct `reqwest::Client` construction/use and direct request convenience calls. Give each rule a remediation message naming the client interface. Apply the smallest possible lint allow only inside `src/client/`, never to `cli`, `auth`, `config`, `policy`, or `snu` modules.
8. Add `tests/structure.rs` using only standard-library traversal and robust relative-path/line-number reporting. Encode the four rules and explicit allow-lists described above. Write its helpers so adding a future source subtree is automatically checked.
9. Run `cargo fmt`, `cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`. Review structural-test failure text by temporarily checking a known forbidden token only in a local scratch change, then remove that scratch change before committing.

## 5. Testing strategy

- Preserve and run client unit tests, especially the read-only client test demonstrating policy travels with the client (`src/client/mod.rs` around 1442) and the request-policy test in `src/policy.rs:658-666`.
- Add focused client tests for each newly internalized operation: attachment upload/download and form script posts must reject a read-only `ClientConfig` before network I/O; OAuth/skill external fetch must retain status/error handling; raw API custom headers must still reject method override in read-only mode.
- Move the existing attachment and script read-only tests away from process-global setup. Assert their command-facing behaviour using injected/configured policy or test-only policy support, including cleanup so tests remain parallel-safe.
- Add unit tests for structural-test helper functions using temporary directory fixtures: each rule accepts its allowed path and rejects an equivalent prohibited source path; diagnostics include the path, line, rule, and remediation. The integration structural test then validates the actual `src/` tree.
- Run the full existing integration suite because it exercises command output and mocked HTTP behaviour (`tests/test_attachment.rs`, `tests/test_api_script.rs`, and the other `tests/test_*.rs` targets).
- Validation gate: `cargo clippy --all-targets -- -D warnings` verifies configured lint enforcement; `cargo test` runs `tests/structure.rs`; `cargo fmt -- --check` confirms the new configuration/test code is formatted. CI already executes all three (`.github/workflows/ci.yml:38-45`).

## 6. Prototype? (yes/no + what it validates)

**No.** This is not an uncertain state model or visual question. The design can be de-risked directly with a small vertical refactor of the two known escape-hatch callers and compile/test feedback. A throwaway prototype would duplicate real HTTP/policy seams without answering more than the planned client tests and structural test will.

## 7. Risks & open questions

- **Scope of the client interface:** Do not replace `http()` with a generic `request_builder` or `execute` interface; that recreates the shallow seam. Review each focused operation against the deletion test and add a generic operation only if two real adapters/callers require the same variation.
- **Policy semantics for non-instance HTTP:** OAuth token exchange and remote skill download are legitimate network operations with different authority than ServiceNow record calls. Their client operations need explicit documented behaviour; they must not silently inherit same-origin rules intended for authenticated instance calls, nor become an unaudited generic fetch escape hatch.
- **Streaming and headers:** Attachment download currently streams chunks (`src/cli/commands/attachment.rs:260-280`), and form/script flows require cookies and content types. Client-owned result types must preserve only these demonstrated requirements without exposing `reqwest` types.
- **Global test state:** The current command tests mutate a global atomic. Moving this behind policy test support must ensure restoration even on assertion failure and avoid parallel-test leakage; prefer policy/config injection where feasible.
- **Lexical structural checks:** A simple `reqwest` scan can flag comments or documentation. Keep messages clear, begin with the strict production rule, and only add narrowly tested comment/string handling if real false positives arise. Do not weaken scanning with broad ignored directories under `src/`.
- **URL dependency decision:** Adding direct `url` is the cleanest way to retain URL value parsing outside the HTTP implementation, but it expands the manifest. Confirm whether the project prefers this explicit dependency or client-owned parsing helpers before implementation.
