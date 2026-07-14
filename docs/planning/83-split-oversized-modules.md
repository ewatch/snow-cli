# Plan: Split oversized modules

## 1. Problem & goal

Several modules require reviewers to retain roughly 1.7k–2.5k lines of unrelated implementation context for a focused change: `src/client/mod.rs` is now 2,523 lines, `src/cli/args.rs` is 2,307, and `data`, `scope`, `config`, and `snu` handlers range from 1,751 to 1,933 lines. The goal is a mechanical reorganisation that improves locality and makes focused PRs reviewable without changing runtime behaviour, CLI syntax/help, policy decisions, output, errors, or the public interface.

The design must preserve the accepted noun-verb command model (`docs/adr/0002-noun-verb-commands.md:11-28`). It must also be sequenced after, or explicitly coordinated with, `servicenow-cli-81`, because both tasks move the identifier types used by CLI arguments and client methods.

## 2. Current state in code (cited files)

- `src/client/mod.rs` owns configuration construction (`build_client_with_timeout`, lines 27-48), debug redaction/logging and cookie parsing (lines 69-497), the `SnowClient` state and construction (lines 499-668), form-session handling (lines 692-938), authenticated transport and JSON helpers (lines 940-1175), Table API pagination (lines 1177-1292), and all 1,229 lines of client tests (lines 1294-2523). The only domain-specific `SnowClient` operation is table pagination; attachment and script implementations currently use the generic `http`, `authenticator`, `authenticated_url`, form-session, and logging interface directly (`src/cli/commands/attachment.rs:146-164,224-244`; `src/cli/commands/script.rs:112-245`). Therefore no existing attachment, import-set, or script client adapter can be moved without inventing behaviour.
- `src/cli/args.rs` keeps root parsing and shared formats (`Cli`, `Commands`, `OutputFormat`, lines 55-186) together with every noun's Clap types. Examples include profile/config plus SDK types (lines 227-596), table/data/seed/scope types (lines 672-1027), and the large SN-Utils hierarchy (lines 1212-1584). Its parser and help tests are also all colocated (lines 1621-2307). Most consumers import from the flat `crate::cli::args` interface, including the dispatcher (`src/lib.rs:15,54-72,116-216`), read-only conversion (`src/cli/readonly_args.rs:4-10,560-803`), policy classification (`src/policy.rs:6-9,181-357`), and command handlers.
- `data` has two naturally coherent implementations behind its command-handler interface: single-table artifacts and imports (`src/cli/commands/data.rs:348-418,593-880`) and multi-table package specification, dependency ordering, export, validation, and import (`419-531,882-1739`). The package model and helpers are currently interleaved with flat-artifact structures at lines 76-346.
- `scope` dispatches four verbs at lines 225-281. List presentation and classification live at lines 1089-1425; shared scope collection and inventory representation live at lines 758-1087 and 1451-1649; `move-file` and its generated-script implementation occupy lines 377-756. `move-file` deliberately crosses the existing `script::run_background_script` seam (`src/cli/commands/scope.rs:396-405`).
- `config` dispatches profile, SDK, and output operations in one handler (`src/cli/commands/config.rs:34-211`). Profile mutation/listing and selector logic occupy lines 267-758 and 1041-1304; now-sdk import/export and rollback operations occupy lines 759-1040. Existing tests begin at line 1306.
- `snu` has a large dispatcher (lines 17-379), bridge/session acquisition (381-426), record reads/writes and background-script mutation support (549-1254), and response/file utilities (1282-1426). It already has an explicit broker adapter at `src/snu/broker.rs`, which the handler uses through `BrokerBridge` (`src/cli/commands/snu.rs:14,381-426`).
- The repository already uses small model modules (`src/models/mod.rs:1-5`) and a small command-module index (`src/cli/commands/mod.rs:1-13`), demonstrating the established Rust module layout. `cargo test --lib` currently passes 405 tests (baseline run during planning).

## 3. Proposed design in deep-module terms

### Client module

Keep `client` as one external **module** with the same public **interface**: callers continue to use `crate::client::{build_client, build_client_with_timeout, ClientConfig, SnowClient}` and the existing `SnowClient` methods. `src/client/mod.rs` becomes the small external **seam**: declarations, shared state types, construction entry points, and private child-module declarations only. It re-exports nothing new and changes no visibility.

Split its **implementation** into private files with multiple `impl SnowClient` blocks:

| File | Implementation placed behind the unchanged interface |
|---|---|
| `src/client/core.rs` | `SnowClient`, `SessionState`, `FormSession`, `ClientConfig`, construction, URL safety/origin checks, and base accessors |
| `src/client/debug.rs` | HTTP-debug environment parsing, redaction, request/response logging, and their tests |
| `src/client/session.rs` | cookie extraction/upsert helpers and form-session bootstrap/login/cache implementation and tests |
| `src/client/transport.rs` | authenticated URL resolution, generic HTTP verbs, custom headers, retry/error/policy enforcement, JSON helpers, and transport tests |
| `src/client/table.rs` | `get_table_records*` pagination implementation and its wiremock tests |

This follows actual responsibilities rather than the issue's illustrative filenames: moving attachment, import-set, or script code into client would create a new **adapter** where there is currently only one implementation and no varying adapter. That would make a hypothetical **seam**, violating the “two adapters means a real seam” rule. The retained generic transport interface is the existing seam those command handlers use.

The split increases **depth** by leaving callers and tests with the same small interface while hiding the distinct transport, session, debug, and Table API implementations. It gains **leverage** because each concern is changed and verified once, and **locality** because a Table API or session change does not require reading unrelated debug and wiremock code. Private internal seams are acceptable only to organise this implementation; they are not new caller-facing interfaces.

### CLI argument module

Keep `crate::cli::args` as the external **module** and parsing **interface**. Retain `Cli`, `Commands`, `OutputFormat`, and the top-level help text in `src/cli/args.rs`; declare and `pub use` domain modules so every existing `crate::cli::args::TypeName` path continues to compile. Put each noun's complete Clap implementation, its help text, and its parser tests in:

- `src/cli/args/{profile,auth,table,data,seed,scope,attachment,import_set,api,script,codesearch,skill}.rs`
- `src/cli/args/snu.rs` for `SnuArgs`, all nested SN-Utils command types, `SnuSwitchType`, defaults, SN-Utils help text, and SN-Utils parser tests.

`profile.rs` owns `ConfigArgs`, `ConfigCommands`, `ProfileSdk*`, `CliAuthMethod`, and `CliOAuthGrantType`, keeping the profile noun coherent despite the historic `config` aliases. `skill.rs` owns `SkillTarget` as it is used only by `SkillCommands`. Each file imports its own Clap traits and identifier models. Keep parser tests beside the noun they exercise; retain only root parser/help tests in `args.rs`.

This is a deep module rather than a set of shallow forwarding modules: the root interface remains one stable import location and one top-level parser, while individual implementations own all command syntax, invariants encoded in Clap attributes, examples, and tests. The existing read-only and policy modules keep using that interface; their conversion/classification logic is not moved or redesigned.

### Large command modules

Perform these as separate mechanical commits (and, preferably, reviewable PRs) after client and argument moves. Preserve each existing `commands::<noun>::handle` interface at its current seam; change the file module to a directory module only when needed:

| Directory module | Private implementation modules |
|---|---|
| `src/cli/commands/data/mod.rs` | `flat.rs`, `package.rs`, `types.rs`, `tests.rs` |
| `src/cli/commands/scope/mod.rs` | `list.rs`, `inventory.rs`, `move_file.rs`, `types.rs`, `tests.rs` |
| `src/cli/commands/config/mod.rs` | `profiles.rs`, `now_sdk.rs`, `output.rs`, `tests.rs` |
| `src/cli/commands/snu/mod.rs` | `session.rs`, `records.rs`, `mutations.rs`, `browser.rs`, `response.rs`, `tests.rs` |

`mod.rs` in each directory is only the existing dispatch point and private declarations. `types.rs` holds existing private request/output/data structures shared by two or more sibling implementations; it is not a new public interface. Do not extract “common” helpers across nouns: `table`, `api`, `script`, and `import_set` currently have superficially similar stdin/HTTP code but only one implementation each, so a cross-noun adapter would reduce locality and add a speculative seam.

## 4. Step-by-step implementation

1. **Coordinate first.** Confirm `servicenow-cli-81` has landed or agree on commit order and ownership. Start this work from its final identifier paths/signatures; do not duplicate or partially revert its moves. Record the baseline `git status`, `wc -l`, `cargo test`, and representative `--help` output before editing.
2. **Create the client implementation files by pure move.** Add the private `mod` declarations in `src/client/mod.rs`; move whole functions, types, and their directly related `#[cfg(test)]` tests into `core.rs`, `debug.rs`, `session.rs`, `transport.rs`, and `table.rs`. Use `super::{...}` for private shared state and retain existing `pub`/`pub(crate)` visibility exactly. Split the existing `impl SnowClient` into blocks without changing method signatures, bodies, order-sensitive request construction, constants, or strings. Keep the external module declarations for `error` and `pagination` unchanged.
3. **Compile the client move before proceeding.** Fix only compiler-required imports/module paths. In particular, retain the `pub(crate)` logging and cookie helpers used by `auth/oauth2.rs`, `attachment.rs`, and `script.rs`; retain `authenticated_url`, `http`, and `authenticator` because those handlers presently cross the client interface.
4. **Turn `args.rs` into the facade.** Add `src/cli/args/` domain files, move each noun's constants/types/associated impls and relevant tests intact, add private module declarations and `pub use` statements to the facade, and keep root `Cli`/`Commands` resolving the re-exported types. Do not rename `Config*`, command variants, aliases, flags, defaults, Clap groups, docs, or paths used by `readonly_args.rs` and `policy.rs`.
5. **Verify the parser contract.** Retain `Cli::command().debug_assert()` and all current parsing tests; run top-level and nested help for every moved noun, including `profile sdk`, `data export-package`, `scope move-file`, and all `snu` nested nouns. Compare their stdout byte-for-byte against the pre-move captures where practical.
6. **Split oversized command implementations one noun at a time.** Convert only `data`, then `scope`, then `config`, then `snu` to directory modules. Move existing declarations and functions without semantic edits according to the table above. Keep private sibling imports narrow, move each unit test with the function/data it verifies, and leave dispatch signatures and all `src/lib.rs` routing unchanged. Build and test after each noun so a misplaced private item is isolated.
7. **Review movement and finish.** Run formatter and lint checks; inspect rename/move detection with `git diff -M --summary`, `git diff -M --color-moved=dimmed-zebra`, `git diff --check`, and `git diff --stat`. The intended diff is moves plus Rust module/import wiring only. If any behavioural change is discovered, stop and create/handle it separately rather than hiding it in this reorganisation.

## 5. Testing strategy

- Preserve all existing unit tests and move them with their implementation; this includes client wiremock coverage for retry, error, form-session, URL, redaction, and pagination behaviours (`src/client/mod.rs:1385-2522`), parser/help coverage (`src/cli/args.rs:1621-2307`), and each moved command module's tests.
- Run `cargo test --lib` after each mechanical slice and `cargo test` after the complete reorganisation. Run the existing integration suite with `cargo test --test '*'`; the testing guide identifies these as black-box CLI contract tests (`docs/guides/testing.md:11-33`).
- Run `cargo fmt -- --check` and `cargo clippy --all-targets -- -D warnings`.
- Add no new behaviour tests solely to justify a move. If current tests do not directly cover a parser/help contract being moved, use a pre/post generated-help comparison rather than changing the contract. Any newly discovered regression gets a focused regression test in the implementation module that owns it, in a separate behavioural change.
- Treat a clean compilation of `readonly_args.rs`, `policy.rs`, all handlers, and `auth/oauth2.rs` as an interface compatibility check, since these are the important existing consumers of flat args and client internals.

## 6. Prototype?

**No.** This is a mechanical implementation-location question, not an uncertain state/logic or visual question. A throwaway LOGIC prototype would not validate Rust privacy, Clap-derived parsing, or module-path compatibility better than the compiler plus the existing 405-test baseline. The migration slices, compiler, help comparisons, and test suite are the appropriate de-risking loop.

## 7. Risks & open questions

- **Newtype churn:** `servicenow-cli-83` is blocked by `servicenow-cli-81` in beads. Landing both independently will create broad conflicts around `args.rs`, client Table API signatures, and identifier imports. Resolve the dependency before moving files.
- **Accidental interface narrowing:** Client internals have real same-crate consumers: `attachment` and `script` use raw client accessors, while OAuth uses logging/cookie helpers. Do not make these private merely because they move to a child module; preserve their current `pub(crate)` interface until a separately designed seam has two adapters.
- **Clap drift:** Moving constants or attributes can silently change generated help, aliases, default values, groups, or read-only conversions. The parser debug assertion, existing parser tests, generated-help captures, and full `snow-cli-ro` tests are required guardrails.
- **Over-splitting:** The goal is depth and locality, not a file per function. Keep functions and private types together when they change together; do not introduce cross-noun shared adapters based only on similar syntax.
- **Command-module scope:** The issue labels command splits optional. If the client/args move already makes a review too broad, land the required client and args reorganisation first, then take `data`, `scope`, `config`, and `snu` as independently reviewable mechanical slices. Do not leave a mixed half-directory/half-file layout.
- **Line-count target:** There is no fixed line-count threshold that proves depth. Review each proposed file against the deletion test: deleting it should force its hidden complexity back into multiple callers; otherwise merge it with its owning implementation rather than preserving a shallow module.
