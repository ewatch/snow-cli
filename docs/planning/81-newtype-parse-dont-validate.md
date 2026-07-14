# Plan — servicenow-cli-81: Parse, don't validate: identifier newtypes

## 1. Problem & goal

The original problem was **shotgun parsing**: table names, `sys_id` values, path segments, and encoded-query literals were validated at individual call sites while still represented as `&str`/`String`. A caller could therefore reach a URL-building or query-building path without carrying any proof that its input was safe.

The goal is to make the identifier module deep: values become `TableName`, `SysId`, `PathSegment`, or `EncodedQueryValue` at their input seam and can only exist after parsing succeeds. Command and client callers then receive leverage from a small interface (`FromStr`/`TryFrom<String>`, `as_str`, and `Display`) while validation rules, error messages, and URL/query-safety details remain local to the module implementation.

**Current repository status:** this task's production refactor is already present in `HEAD`, introduced by commit `2fc799f` (`refactor: introduce TableName/SysId/PathSegment/EncodedQueryValue newtypes (servicenow-cli-81)`) and hardened by `054bead`. `src/cli/validation.rs` no longer exists. Thus the implementation work below is a reconciliation/verification plan for the already-landed change, not a proposal to duplicate it or reopen unrelated validation work.

## 2. Current state in code (cited files)

- `src/models/mod.rs:2` exposes the identifier module. `src/models/identifiers.rs:1-11` documents its parse-don't-validate invariant: no unchecked constructor is exposed.
- `src/models/identifiers.rs:21-75` defines the private-`String` newtypes and the shared public interface: `IdentifierError`, `as_str()`, `Display`, `AsRef<str>`, and `FromStr` delegating to `TryFrom<String>`.
- The implementation localizes the former rules: `TableName` accepts non-empty ASCII alphanumerics/underscores (`src/models/identifiers.rs:77-96`); `SysId` requires exactly 32 hex characters (`:141-156`); `PathSegment` rejects empty, traversal, separator, query/fragment, and control characters (`:115-168`); and `EncodedQueryValue` rejects empty values, encoded-query operators, and controls (`:170-189`).
- `TableName::from_static` (`src/models/identifiers.rs:98-113`) is the intentionally narrow construction path for trusted compile-time ServiceNow table literals, used for example by the schema lookup at `src/cli/commands/table.rs:380-389`.
- The primary clap seam is already typed: table CRUD uses `TableName`/`SysId` in `src/cli/args.rs:677-813`; data export/import uses `TableName` in `:824-890`; scope uses `EncodedQueryValue`, `TableName`, and `SysId` in `:941-997`; and attachment/import-set arguments use the same types in `:1034-1099`. The read-only binary preserves those types at its separate parser seam (`src/cli/readonly_args.rs:160-257`, `:318-381`) before forwarding them without reparsing (`:654-769`). Clap uses `FromStr` automatically for these field types; no explicit `value_parser!` attribute is needed.
- Command implementations retain the proof rather than converting identifiers back to raw owned values: table list passes `&table` to the client (`src/cli/commands/table.rs:41-49`), scope's request object carries `&TableName`/`&SysId` (`src/cli/commands/scope.rs:18-24`, `:259-279`), and data import options carry `Option<&TableName>` (`src/cli/commands/data.rs:159-164`). Formatting is only used at the sink where a path/query must be assembled (for example `table get` at `src/cli/commands/table.rs:62-83`).
- The client seam is currently intentionally narrow: the table pagination methods require `&TableName` (`src/client/mod.rs:1177-1204`), so their URL construction cannot accept an unvalidated table string. Generic raw-request methods remain `&str` (`src/client/mod.rs:949-991`) because the raw API command is designed to accept arbitrary paths and is governed by the authenticated URL/origin checks, not the table-identifier interface.
- Attachment list additionally parses its already-valid `SysId` as `EncodedQueryValue` before interpolating it into `sysparm_query` (`src/cli/commands/attachment.rs:48-69`), making the second query-context invariant explicit.
- Unit regressions cover accepted/rejected parsing, conversion/display, traversal/backslash protection, and 32-hex `SysId` enforcement in `src/models/identifiers.rs:191-313`. `src/client/mod.rs:1377-1379` supplies a parsed `TableName` test fixture. There are CLI parsing tests for valid typed values, e.g. scope move-file at `src/cli/args.rs:2159-2189`, and an integration regression for an invalid table name at `tests/test_table.rs:1128`.

## 3. Proposed design in deep-module terms (interfaces, seams, what depth is gained)

The **module** is `models::identifiers`. Its external **interface** is deliberately small:

- parse external or runtime strings with `FromStr` or `TryFrom<String>`;
- borrow a validated value with `as_str`/`AsRef<str>` or render it with `Display`;
- construct only audited internal table literals through `TableName::from_static`.

The private `String` is the **implementation**. Its invariant-specific checks and stable error text stay there; command handlers must not know the character rules or repeat a `validate_*` call. `IdentifierError` is part of the interface because clap must display parse failures, while the error storage and rule predicates remain implementation details.

There are two real **seams**:

1. The argv seam is the typed fields in `args.rs` and `readonly_args.rs`; clap invokes `FromStr`, so malformed user input cannot enter command dispatch.
2. Runtime/file/API input is parsed with `TryFrom<String>` at the point it crosses into an identifier-sensitive operation (for example, manifest-derived table names in `src/cli/commands/data.rs:456`, `:774`, and `:1080`).

`TableName`, `SysId`, `PathSegment`, and `EncodedQueryValue` are concrete types rather than interchangeable **adapters**: they encode distinct invariants for distinct interpolation contexts. Do not add a generic identifier adapter or a new seam merely to avoid a local parse; one adapter is only a hypothetical seam. The generic raw HTTP client stays outside this module's interface because it deliberately accepts raw paths, while table-specific client methods form the type-safe seam.

This raises **depth** because a caller learns one construction/borrowing protocol yet exercises all centralized format and traversal protections. It creates **leverage** because every typed clap field and typed client call gets the proof without a fresh validation branch. It improves **locality** because changing a ServiceNow identifier rule, its error text, and its tests happens in `identifiers.rs`, rather than across command handlers. The interface is also the test surface: parser and command/client tests should create these types through parsing, not reach into validator internals.

## 4. Step-by-step implementation

1. **Confirm the landed scope before modifying code.** Verify `2fc799f` and `054bead` are ancestors, `src/cli/validation.rs` is absent, and `src/cli/io.rs` owns the unrelated `read_to_string_limited` helper. Do not recreate `validation.rs` or move `io` code back.
2. **Audit every identifier ingress.** Search both `args.rs` and `readonly_args.rs`, plus runtime inputs from manifests/artifacts, for table names, `sys_id`s, generic URL path segments, and encoded-query value interpolation. Keep CLI-origin values typed at the argv seam; use `.parse::<Type>()?`/`TryFrom<String>` exactly once for non-CLI input. Preserve `String` where a value is not an identifier or intentionally supports a broader grammar (full encoded queries, field lists, raw API paths, and SNU bridge payloads).
3. **Audit the table-client interface and call sites.** Keep table pagination methods accepting `&TableName`; follow compiler errors rather than adding `as_str()` merely to satisfy a raw `&str` signature. For a future table/sys-id-specific `SnowClient` method, make the corresponding typed identifier part of that method's interface. Do not incorrectly type generic raw `get`/`post`/`request_with_headers` methods, because that would collapse their intentionally different seam.
4. **Preserve construction discipline.** Keep the inner values private, retain `FromStr`, `TryFrom<String>`, `as_str`, and `Display`, and reserve `TableName::from_static` for audited string literals compiled into the binary. Do not add `From<String>`, `Deref<Target = str>`, or an unchecked public constructor, as each would let callers bypass the module's interface.
5. **Reconcile and close only if verification finds no gap.** If the audit finds a raw identifier crossing into a typed URL/query context, make the smallest compiler-driven conversion at that ingress and add its regression test. If it finds no gap, record that servicenow-cli-81 is already implemented and avoid a no-op source change.

## 5. Testing strategy

- Run identifier unit tests in `src/models/identifiers.rs` and retain a table-driven set for valid and invalid values: empty input, table punctuation, query operators/control characters, slash/backslash/`.`/`..` path traversal, and short/long/non-hex `SysId` values. Assert relevant error wording where CLI compatibility requires it.
- Add/retain parser-level tests using `Cli::try_parse_from` and `ReadOnlyCli::try_parse_from` for invalid table, `sys_id`, and scope/query-literal input. Assert parsing fails before dispatch; retain valid parsing assertions that inspect `as_str()` rather than relying on raw strings.
- Keep command/client tests typed: construct test tables through parsing and ensure table pagination still requests `/api/now/table/incident`. The existing client fixture at `src/client/mod.rs:1377-1379` is the pattern.
- Run the existing black-box invalid-table integration test in `tests/test_table.rs:1128` and add analogous black-box coverage only for a newly discovered CLI ingress. No real instance is needed; use existing mocks/`assert_cmd` conventions.
- Validate the bounded refactor with `cargo fmt -- --check`, `cargo test`, and `cargo clippy --all-targets -- -D warnings`.

## 6. Prototype? (yes/no + what it validates)

**No.** This is not a state-model or visual-design question. Rust's compiler plus clap's parser tests directly validate the relevant interface and seam. A throwaway prototype would duplicate the newtype constructors without increasing confidence; the existing module tests and compiler-driven call-site audit provide better leverage and locality.

## 7. Risks & open questions

- **Compatibility:** `SysId` is deliberately stricter than generic path safety (32 hexadecimal characters). Confirm every affected ServiceNow command truly expects a canonical `sys_id`, not a permissive alternate identifier; do not silently relax the invariant merely to preserve an undocumented input form.
- **Context matters:** a `SysId` safe for a path is separately checked before use in an encoded query (`attachment.rs:50-58`). Future interpolation contexts must use the type whose interface proves that context's rule rather than assuming one identifier type is universally safe.
- **Typed argv coverage is not universal:** SNU command fields remain raw strings (for example `src/cli/args.rs` SNU arguments after line 1200) because they are sent through the SN-Utils bridge rather than the `SnowClient` table seam. This issue should not expand into a behavior-changing SNU redesign without a separate requirement and tests.
- **Raw API behavior must remain raw:** converting `SnowClient`'s generic path methods to `PathSegment`/`TableName` would reject valid custom endpoints and create a shallow, over-broad interface. Keep URL safety enforcement in the raw-client implementation.
- **Repository state:** the planning directory is currently untracked and contains the supplied briefs. Only this plan file should be added for this request; do not alter the pre-existing `.beads/.migration-hint-ts` working-tree change.
