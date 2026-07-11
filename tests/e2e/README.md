# E2E scenarios

Declarative, per-subcommand test scenarios for `snow-cli`, replacing prose
task prompts (`.claude/e2e-task-*.md`) with files a deterministic runner can
execute and a coverage gate can check.

- `tests/e2e/scenarios/**/*.toml` — one scenario file per leaf subcommand.
- `scripts/e2e-coverage` — CI gate: fails if any subcommand has no scenario file.
- `scripts/e2e-run` — deterministic runner: executes scenarios, checks exit
  code and deterministic assertions, writes sanitized results to
  `artifacts/e2e/<version>/`.

## File layout

A scenario file's path mirrors the command path: `snow-cli table list` lives
at `tests/e2e/scenarios/table/list.toml`; `snow-cli snu broker status` lives
at `tests/e2e/scenarios/snu/broker/status.toml`. This is what
`scripts/e2e-coverage` checks against `--help` output — get the path wrong
and the gate will report the subcommand as uncovered even if a scenario for
it exists somewhere else.

## Scenario format (TOML)

```toml
name = "table-create"
description = "Create an incident record (table CRUD: create leg)"

# Tags gating whether this scenario can run at all. Unmet requirements are
# SKIPPED and reported, never faked as a pass. See "requires tags" below.
requires = ["credentials"]

# Steps run before the command under test, in order. Optional.
[[setup]]
description = "Create the scenario profile"
args = ["profile", "add", "e2e-scenario", "--instance", "$SNOW_E2E_INSTANCE_URL", "--auth-method", "basic", "--username", "$SNOW_E2E_USERNAME"]
# allow_failure defaults to false for setup: a failing setup step aborts the
# scenario (marks it FAILED) and skips the command, but cleanup still runs.

[[setup]]
description = "Store credentials for the scenario profile"
# `shell` steps run through `bash -c` instead of invoking snow-cli directly.
# Use them for anything that isn't a single snow-cli invocation (piping a
# password via stdin, creating a local fixture file, etc).
shell = "printf '%s' \"$SNOW_E2E_PASSWORD\" | \"$SNOW_CLI_BIN\" auth login --profile e2e-scenario --password-stdin"

# The command under test. Exactly one per file.
[command]
args = ["table", "create", "incident", "--profile", "e2e-scenario", "--output", "json", "--data", "{\"short_description\":\"snow-cli e2e scenario: table-create\"}"]
# Pull values out of this step's stdout (parsed as JSON) for use by later
# steps via {{name}}. jq-style path; "raw" captures the trimmed stdout text
# as-is instead of parsing JSON (useful for shell steps that print a path).
capture = { sys_id = ".sys_id" }

# Deterministic assertions, checked automatically by scripts/e2e-run.
[expect]
exit_code = 0
stdout_contains = ["sys_id"]           # substrings that must appear in stdout
stderr_contains = []                   # substrings that must appear in stderr
json_field_present = [".sys_id"]       # jq paths that must resolve against stdout parsed as JSON

# Natural-language expectations for later LLM verification. Never executed
# or scored by scripts/e2e-run — recorded verbatim as "needs LLM verification"
# in the result JSON's `fuzzy_pending` array.
[[fuzzy]]
expectation = "The response's sys_id is a 32-character hex GUID and short_description matches what we sent."

# Steps run after the command, always — regardless of setup/command outcome.
# Failures here are recorded as warnings, never fail the scenario.
[[cleanup]]
description = "Delete the record this scenario created"
args = ["table", "delete", "incident", "{{sys_id}}", "--profile", "e2e-scenario", "--yes"]
allow_failure = true   # cleanup steps default to allow_failure = true
```

### `requires` tags

| Tag | Unlocked by |
| --- | --- |
| `none` | Always runs. |
| `credentials` | `SNOW_E2E_INSTANCE_URL`, `SNOW_E2E_USERNAME`, `SNOW_E2E_PASSWORD` all set and non-empty. |
| `sn-utils-bridge` | `SNOW_E2E_SN_UTILS=1`. |

An unknown tag is treated as unmet (the scenario is skipped, not silently
run). A scenario can list multiple tags; all must be satisfied.

### Placeholders

- `$VAR` / `${VAR}` in `args` and `shell` values are substituted from the
  process environment at parse time (via Python's `os.path.expandvars`).
  Never put a real credential literally in a scenario file — use env var
  placeholders, matching `SNOW_E2E_INSTANCE_URL` / `SNOW_E2E_USERNAME` /
  `SNOW_E2E_PASSWORD`.
- `{{name}}` in `args` and `shell` values are substituted from values an
  earlier step in the *same scenario file* captured. Not available across
  scenario files.
- Passwords should go through `shell` steps piping via stdin (`auth login
  --password-stdin`), not as a literal CLI arg — the CLI's own `--help`
  recommends this to avoid shell-history/process-listing leaks, and
  `scripts/e2e-run` only redacts known env var values from recorded output,
  not arbitrary secrets.

## Running

```bash
scripts/e2e-coverage          # CI gate: fails if any subcommand lacks a scenario file
scripts/e2e-run                # run every scenario under tests/e2e/scenarios/
scripts/e2e-run tests/e2e/scenarios/table/  # run one subtree
scripts/e2e-run tests/e2e/scenarios/table/list.toml  # run one file
```

Credentialed scenarios need:

```bash
export SNOW_E2E_INSTANCE_URL=https://your-dev-instance.service-now.com
export SNOW_E2E_USERNAME=admin
export SNOW_E2E_PASSWORD='...'
scripts/e2e-run
```

Results land in `artifacts/e2e/<version>/`: one sanitized JSON file per
scenario, an aggregate `results.jsonl`, and a `summary.md`. `scripts/e2e-run`
exits non-zero if any scenario **failed**; skipped scenarios are reported but
do not fail the run, matching the release guide's rule that an unavailable
test never counts as a pass (`docs/guides/releasing.md`).

> **`artifacts/` is git-ignored on purpose.** Redaction is best-effort
> (see "known gaps"), so these files may still contain instance-specific
> values. Review any artifact before copying its content into committed
> docs or examples, and never force-add the `artifacts/` tree.

## Isolation

Every `scripts/e2e-run` invocation points `SNOW_CLI_CONFIG` at a fresh temp
file (removed on exit), so scenarios never read or write your real
`~/.config`-style profile store. Scenarios that need an authenticated
profile create one scoped to the run (conventionally named `e2e-scenario`)
and remove it in cleanup.

This does **not** sandbox the OS keychain: `auth login` still writes a real
keychain entry under the scenario's profile name. Cleanup best-effort-removes
it via `profile remove`, but there's no keychain sandbox — see "known gaps".

## Known gaps

- **Coverage is enumeration-only, not authored for every subcommand yet.**
  `scripts/e2e-coverage` currently reports ~60 subcommands without a scenario
  file (everything except the 6 seeded here: `completions`, `profile add`,
  `table list`/`create`/`delete`, `attachment upload`). That's the point of
  the gate — it's meant to be closed incrementally, not all at once.
- **Hidden commands are invisible to the coverage gate.** Subcommands marked
  `#[command(hide = true)]` in `src/cli/args.rs` (currently `skill install`)
  don't appear in `--help` output, so the gate can't require a scenario for
  them. Unhide + add a scenario together when a hidden command ships.
- **Redaction is literal-substring, not structural.** `scripts/e2e-run`
  redacts the known `SNOW_E2E_*` env var values (URL, username, password) and
  the derived HTTP Basic auth token (`base64(user:pass)`) wherever they appear
  verbatim in captured stdout/stderr/argv. It does **not** redact `g_ck`
  session tokens, cookies, `sys_id`s, or other generated values — these are
  left intact (sys_ids are often wanted in doc examples). Because artifacts
  are git-ignored, this is a review-before-publish gap, not a commit gap:
  `docs/guides/releasing.md` step 3 (turning successful E2E artifacts into doc
  examples) still needs a human/agent pass to strip anything sensitive before
  publishing.
- **`sn-utils-bridge` scenarios aren't seeded yet.** The tag and skip logic
  exist, but none of the 6 seed scenarios exercise the `snu` command family
  (see `.claude/e2e-task-b.md` for the prose version of that flow).
- **No parallelism.** Scenarios run sequentially in one isolated config; two
  concurrent `scripts/e2e-run` invocations would race on the same
  `e2e-scenario` profile name if pointed at the same real instance.
- **Argument values may not contain literal newlines.** The runner passes
  step stdout/args through newline-delimited plumbing (`jq -r '...[]'`
  piped into a `while read` loop); a `--data` payload or captured value
  containing an embedded newline will be silently truncated at the first one.
