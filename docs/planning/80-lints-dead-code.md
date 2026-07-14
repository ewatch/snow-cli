# Plan: enable lints and remove crate-wide dead-code suppression

## 1. Problem & goal

`dead_code` is a compiler signal that makes safe refactoring possible: when a path becomes unreachable, the compiler should report it rather than silently accepting it. The goal is to make that signal part of the crate's normal lint interface, remove any crate-wide suppression, and retain only narrow, documented exceptions where code is intentionally present but conditionally used.

The current checkout already contains the core change described by `servicenow-cli-80` (commit `4d2837e`): `src/lib.rs` no longer has `#![allow(dead_code)]`, and the manifest has lint configuration. This work should therefore be treated as a verification and cleanup pass: preserve that policy, prove all targets are clean, and reduce any remaining overly broad local suppression rather than reintroducing the original crate-wide allow.

## 2. Current state in code (cited files)

- `Cargo.toml:89-105` already defines the lint interface: Rust's `unused` group is `warn`; Clippy's `unwrap_used`, `expect_used`, and `panic` are `warn`; and `dbg_macro`, `todo`, and `unimplemented` are denied. This is stronger than the issue's suggested baseline while retaining warnings that CI upgrades to errors.
- `src/lib.rs:1-3` contains only a test-only Clippy exception for panicking test helpers. The crate-wide `dead_code` allow is absent. Its public module interface is declared at `src/lib.rs:5-12` and the command dispatch implementation begins at `src/lib.rs:24`.
- The CLI command interface is centralised in `src/cli/args.rs:156-201` (`Commands`) and dispatched from `src/lib.rs:109-217`. This is the main reachability path to use when judging whether command-facing code is dead.
- `src/client/mod.rs:18-39` exposes client construction, while `src/client/mod.rs:327-421` defines the `SnowClient` interface; command handlers use this module to reach the remote instance. Its functions should be retained or deleted based on real callers, not on a speculative future command.
- There is no `src/cli/validation.rs` in this checkout. Validation is deliberately local to the relevant implementation, for example identifier validation in `src/models/identifiers.rs:115-190`, command validation in `src/cli/commands/data.rs:1351-1659`, and policy validation in `src/policy.rs:89-251`.
- Three existing local `dead_code` suppressions remain:
  - `src/cli/mod.rs:4-5` suppresses the entire output module, although all public output entry points are currently called by command implementations (for example `src/cli/commands/table.rs:58-219`, `src/cli/commands/data.rs:407-1150`, and `src/cli/commands/snu.rs:43-946`).
  - `tests/common/mod.rs:11-50` individually suppresses helper warnings because each integration-test crate imports a shared helper module but does not necessarily use every helper.
  - `src/models/secret.rs:102` narrowly suppresses field warnings in a test-only derived-`Debug` fixture; the adjacent comment explains that the fields are exercised through the derived implementation.
- The intended enforcement command is already wired into CI at `.github/workflows/ci.yml:39-45`, including `cargo clippy --all-targets -- -D warnings`.
- Baseline exploration completed cleanly: `cargo check --all-targets` and `cargo clippy --all-targets -- -D warnings` both exit successfully. That means no currently unsuppressed dead code surfaced under the manifest's `unused = "warn"` policy.

## 3. Proposed design in deep-module terms (interfaces, seams, what depth is gained)

The workspace lint configuration is a small **module** whose **interface** is the set of compiler and Clippy signals contributors can rely on: unused code warns locally and becomes an error in the CI invocation; production `unwrap` and `expect` warn; tests have explicit exceptions. Its **implementation** is the `[lints.rust]` and `[lints.clippy]` manifest tables plus only the narrowly scoped attributes needed by the affected code.

The crate root is the external **seam** for this module: a crate-wide `#![allow(dead_code)]` placed there disables the signal for every caller and every internal implementation. Do not put an exception at that seam. Put a targeted `#[allow(dead_code)]` on the smallest adapter that genuinely varies by integration-test crate or is exercised indirectly, and document the reason at that site.

The command dispatch, policy, validation, client, and models remain their existing modules. This task must not add a new validation module or an abstraction merely to satisfy linting: those would be shallow modules with a larger interface but no additional behaviour. Instead, let lint reachability validate the existing interfaces: `Commands` is the command-interface seam, `ExecutionPolicy` is the policy-interface seam, and `SnowClient` is the authenticated-request-interface seam.

This gives maintainers **locality**: an intentionally retained exception is explained where it lives, and a newly unreachable path warns near its implementation. It gives callers **leverage** and **depth** without expanding their interfaces: callers keep using the same command, policy, and client interfaces while the compiler protects their shared implementations from invisible accumulation.

## 4. Step-by-step implementation

1. Start from a clean source diff and inspect the existing `Cargo.toml:89-105` and `src/lib.rs:1-12` policy. Confirm the obsolete `#![allow(dead_code)]` has not been restored; do not change the test-only Clippy configuration unless the lint baseline changes.
2. Run `cargo check --all-targets` after temporarily removing any suspect local `dead_code` exception one at a time. Record every reported item and classify it from concrete call sites: delete truly unreachable implementation; retain an adapter only when its conditional use or indirect exercise is demonstrable.
3. Remove `#[allow(dead_code)]` from `src/cli/mod.rs:4` and rerun the all-target check. The output module's interface is exercised by current command implementations, so the module-wide allow should not conceal future unused private implementation. If a specific item surfaces, decide it at that item rather than restoring a module-level allow.
4. Keep the fixture-level exception in `src/models/secret.rs:102` only if the compiler continues to require it; retain its explanation because derived `Debug` is the test interface that exercises the fields.
5. For each helper in `tests/common/mod.rs:11-50` that is unused in one or more integration-test crates but used by others, keep a function-level `#[allow(dead_code)]` and make the reason explicit: it is a shared test adapter compiled independently by multiple integration test crates. Delete an unused helper instead of preserving it if repository-wide reference checks find no test caller.
6. Recheck the production Clippy exceptions: retain `src/models/identifiers.rs:105-110` because `TableName::from_static` deliberately accepts only trusted compile-time constants, and retain the guarded take in `src/snu/bridge.rs:501-505` only with its local proof comment. Do not use these Clippy exceptions as a precedent for `dead_code` suppression.
7. Keep the manifest lint interface as configured. If it is missing on the implementation branch, restore `[lints.rust] unused = { level = "warn", priority = -1 }` and the documented `[lints.clippy]` rules at the end of `Cargo.toml`; do not add crate-root lint attributes as a substitute.
8. Format the final source. Do not change command semantics, policy decisions, output formats, or validation placement as part of this lint-only slice.

## 5. Testing strategy

1. Run `cargo fmt -- --check` to verify manifest/source formatting.
2. Run `cargo check --all-targets` to expose compiler `unused`/`dead_code` warnings across the library, both binaries, unit tests, and integration tests.
3. Run `cargo clippy --all-targets -- -D warnings`. This is the acceptance gate: the manifest's warning-level signals must fail under CI's strict invocation, while only documented test or provably safe local exceptions remain.
4. Run `cargo test` to ensure removing a suppression or deleting a dead helper does not break conditional test compilation and to cover the existing command, policy, client, validation, and model tests.
5. Verify the CI command remains identical to the local acceptance gate at `.github/workflows/ci.yml:42`; no additional behavioural test is needed because this change alters compiler feedback, not runtime behaviour.

## 6. Prototype? (yes/no + what it validates)

**No.** This is neither an uncertain state/logic model nor a visual question. The compiler's all-target check and the strict Clippy invocation directly validate the proposed lint module's interface. A throwaway prototype would add no leverage or locality and would be less reliable than compiling the real adapters at their actual seams.

## 7. Risks & open questions

- The issue description is stale relative to the checkout: its requested manifest and crate-root changes are already present and clean. Confirm whether this plan is intended as a follow-up audit or whether the issue should be closed after the scoped-allow review; do not duplicate an already landed change.
- Removing `src/cli/mod.rs:4` may surface a genuinely unused private output helper that is presently hidden by the module-level allow. Delete it if no caller needs it; otherwise move the exception to that item with a reason. Do not preserve the broad module exception merely for convenience.
- `tests/common/mod.rs` is compiled separately for several integration-test crates, so a helper can be useful repository-wide yet dead in a particular test crate. The function-level exceptions are appropriate only for that compilation-model constraint; they should not cover unused production code.
- `[lints.rust] unused` is warning-level by design, while CI's `-D warnings` supplies enforcement. Contributors who run only `cargo check` will see warnings rather than failures; this maintains an informative local interface while CI provides the hard gate.
- `servicenow-cli-80` is blocked by CI issue `servicenow-cli-79` according to beads. The local lint proof can be completed independently, but merge-time enforcement depends on the workflow remaining enabled.
