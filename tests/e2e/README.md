# E2E scenarios

Declarative, per-subcommand test scenarios for `snow-cli`: files a
deterministic runner can execute and a coverage gate can check. They replace
the prose task prompts formerly kept in `.claude/e2e-task-*.md` (now removed).

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

# Set only when this scenario clears cached sessions, stops the broker, or
# otherwise invalidates the SN-Utils harness's shared browser session. The
# runner stable-partitions these scenarios to the end of the selected run.
# session_destructive = true

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

`snu *` (`sn-utils-bridge`) scenarios additionally need the headless-browser
harness (requires `node`; see "Known gaps" below for what it does and does
not cover):

```bash
export SNOW_E2E_SN_UTILS=1
scripts/e2e-run tests/e2e/scenarios/snu/
```

Results land in `artifacts/e2e/<version>/`: one sanitized JSON file per
scenario, an aggregate `results.jsonl`, and a `summary.md`. `scripts/e2e-run`
exits non-zero if any scenario **failed**; skipped scenarios are reported but
do not fail the run, matching the release guide's rule that an unavailable
test never counts as a pass (`docs/guides/releasing.md`).

`SNOW_E2E_ARTIFACTS_DIR` can override the exact output directory. This is
primarily useful for runner tests and disposable local runs.

Capture placeholders are strict. If `{{name}}` refers to a missing or null
capture, the runner does not invoke that setup, command, or cleanup step.
Setup and command resolution errors fail the scenario; cleanup resolution
errors are recorded as cleanup warnings so remaining cleanup can continue.
Artifacts identify the unresolved placeholder by name without recording its
captured value.

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

SN-Utils scenarios share one browser-harness session for efficiency. A
scenario that clears sessions, stops the broker, or otherwise invalidates that
session must declare `session_destructive = true`. The runner preserves the
selected order within each group but always runs every shared-session scenario
before every destructive scenario. Cleanup still runs immediately after each
scenario, and the runner's exit trap terminates the browser harness and the
isolated broker. This ordering is intentional: auto-starting a stopped broker
does not recreate the `/token` captured at harness startup.

This does **not** sandbox the OS keychain: `auth login` still writes a real
keychain entry under the scenario's profile name. Cleanup best-effort-removes
it via `profile remove`, but there's no keychain sandbox — see "known gaps".

## Known gaps

- **Every leaf subcommand now has a scenario file (coverage gate passes), but
  many credentialed assertions are exit-code-first and need tightening from a
  real PDI run.** The scenarios fall into three tiers:
  - **Network-free** (`requires = ["none"]`): run in any CI without credentials —
    `completions`, the local-config `profile *` and `auth status`/`logout` leaves,
    and the `seed *` stubs (which assert the current "planned but not implemented
    yet" contract). These have real deterministic assertions.
  - **Credentialed** (`requires = ["credentials"]`): the REST leaves (`api`,
    `table`, `attachment`, `auth login`/`token`, `data`, `import-set`, `scope`,
    `script`, `codesearch`, `graphql`, `profile sdk`). They run against the PDI
    named by `SNOW_E2E_*`. **Assertion philosophy:** `exit_code` is the primary
    deterministic gate (it proves auth + network + parse + format all worked),
    `json_field_present`/`stdout_contains` are used only where the output shape
    is already proven (e.g. `api` returns a `.result` envelope; the `table`
    family), and richer value-level checks live in `[[fuzzy]]`. After the first
    successful PDI run, promote the fuzzy expectations to deterministic
    assertions using the recorded artifacts (see `docs/guides/releasing.md`).
  - **SN-Utils bridge** (`requires = ["sn-utils-bridge", ...]`): the `snu *`
    leaves, which drive the browser extension bridge rather than REST. Needs
    `SNOW_E2E_SN_UTILS=1` — see the harness details in "Known gaps" below.
- **`import-set load` is self-provisioning and validates the transform too.**
  Its setup steps create an Import Set staging table (`u_e2e_import`, extending
  `sys_import_set_row`) and an active transform map (`u_e2e_import -> incident`)
  via `snow-cli script run` against `tests/e2e/fixtures/import-set/`. Because
  `POST /api/now/import/{table}` runs the transform on load, the single load call
  inserts an incident through the map (asserted deterministically), then cleanup
  drops the fixture and deletes the incident. Runs on any credentialed admin-like
  account with no manual setup; provisioning uses the `/sys.scripts.do`
  background-script endpoint, so the account needs background-script rights (as
  the mutating SNU scenarios already assume). Verified end to end against a PDI.
  `import-set transform` is a hidden, unimplemented CLI placeholder (the load
  already transforms), so `import-set/transform.toml` is tagged
  `requires = ["unimplemented"]` and SKIPPED by default.
- **Some credentialed scenarios still need instance-specific prerequisites the
  suite cannot self-provision, and are gated so they SKIP rather than FAIL.**
  `scope move-file` (a custom scope + movable application file — uses `--dry-run`
  and placeholder ids) is tagged `requires = ["instance-fixtures"]` and skips
  unless `SNOW_E2E_INSTANCE_FIXTURES=1`. Others carry a NOTE comment and will
  FAIL until adjusted: `graphql` (Now GraphQL enabled + a schema-matching
  document) and the `profile sdk` leaves (need the now-sdk toolchain installed —
  they can BLOCK if it is absent). `snu context switch` provisions its own
  disposable update set and restores the current user's previous update set.
- **`json_field_present` means "present AND truthy".** The runner checks each
  path with `jq -e`, which exits non-zero when the resolved value is `false` or
  `null`. So a field that is legitimately `false`/`null` (e.g. `auth status`'s
  `.authenticated` for a profile with no stored credential) cannot be asserted
  via `json_field_present` — assert it through `stdout_contains` (e.g.
  `'"authenticated": false'`) and reserve `json_field_present` for
  always-truthy fields.
- **Hidden commands are invisible to the coverage gate.** Subcommands marked
  `#[command(hide = true)]` in `src/cli/args.rs` (currently `skill install`)
  don't appear in `--help` output, so the gate can't require a scenario for
  them. Unhide + add a scenario together when a hidden command ships.
- **Artifact redaction is structural and conservative, but not a publication
  guarantee.** `scripts/e2e-run` redacts the known `SNOW_E2E_*` env var values
  (URL, username, password), the derived HTTP Basic auth token
  (`base64(user:pass)`), and values under token/session/cookie keys such as
  `g_ck`, `session_token`, `X-UserToken`, and `JSESSIONID`. JSON embedded in
  stdout or stderr is parsed and sanitized structurally; discovered values are
  then removed from every string in the complete result document, including
  argv, descriptions, fuzzy expectations, and `results.jsonl`. Non-JSON text
  gets a conservative key/value and header-pattern fallback, including complete
  `Authorization`, `Proxy-Authorization`, `Cookie`, and `Set-Cookie` values.
  Known and discovered values are replaced longest-first so overlapping
  credentials cannot expose a secret suffix. Safe response
  shape and values such as `sys_id` remain available for debugging. Novel
  unlabeled secret formats can still evade pattern-based redaction, so
  `docs/guides/releasing.md` step 3 still requires review before publishing an
  artifact. Never force-add the `artifacts/` tree.
- **`sn-utils-bridge` scenarios now have a real harness, confirmed against a
  live PDI on 2026-07-17.** `SNOW_E2E_SN_UTILS=1` starts
  `scripts/e2e-snu-harness` (Node + Playwright, npm-installed on demand —
  untouched otherwise), which downloads the real SN-Utils extension from the
  Chrome Web Store, loads it in headless Chromium, logs into
  `SNOW_E2E_INSTANCE_URL` with `SNOW_E2E_USERNAME`/`PASSWORD` (standard
  ServiceNow basic-auth login form; SSO instances unsupported), opens its
  ScriptSync helper tab, triggers `/token` via the page's own
  `window.snuSlashCommandShow('/token', true)`, and approves the resulting
  one-time per-instance connection prompt in the ScriptSync tab
  (`#instanceallow`). `snow-cli snu query incident` against the resulting
  session returned real records end to end. Known limits:
  - The harness runs against a **dedicated isolated broker**
    (`127.0.0.1:19178`/`19179`, via the `SNOW_CLI_SNU_WS_ADDR`/
    `SNOW_CLI_SNU_BROKER_ADDR` env overrides), never the real default
    `1978`/`1979` — so it can't evict a real daily-driver browser session.
    This means only **one** `SNOW_E2E_SN_UTILS=1` run at a time; see "No
    parallelism" below.
  - Getting an isolated port past the extension required patching a local,
    unpacked copy of it (CSP `connect-src` + the hardcoded WS port in
    `scriptsync.js`) — never redistributed, applied fresh each run. The
    patch matches on exact literal strings; if SN-Utils ships an update that
    changes them, the harness fails loudly (not silently unpatched) and the
    patch in `scripts/e2e-snu-harness/harness.js` needs updating. The same
    local copy also has `<all_urls>` added to `host_permissions` so the
    `snu screenshot` scenario's `chrome.tabs.captureVisibleTab` call succeeds
    without the interactive "click the SN Utils icon" `activeTab` gesture (see
    the screenshot note below).
  - The connection-approval prompt is per-instance but the harness always
    starts from a fresh browser profile, so it reappears (and gets
    auto-approved) every run — this is expected, not a bug.
  - `token_capture` in the harness's ready signal is `"attempted+approved"`
    on the confirmed-working path; `"attempted (no approval prompt seen)"`
    or `"failed"` indicate `snuSlashCommandShow` wasn't found in time
    (`window.snusettings`/`window.snuSlashCommandShow` load asynchronously
    after login — the harness waits up to 15s) or SN-Utils' own internals
    changed since 2026-07-17.
  - The behavioral SNU scenarios require an administrator-like PDI account:
    they create and delete incidents, run background scripts for mutations,
    upload an attachment, and create/read/switch/delete `sys_update_set`
    context. The context scenario also needs read access to the current user's
    `sys_user_preference` row so it can restore the prior update set.
  - `snu wait-token` starts its fresh-session waiter first and then targets the
    harness ServiceNow tab with `snu slash /token`; the token captured during
    harness startup is intentionally not accepted as evidence for this leaf.
  - `snu screenshot` targets that same tab and fails unless the saved file is a
    non-empty PNG. `chrome.tabs.captureVisibleTab` normally needs a manual
    `activeTab` gesture (clicking the SN Utils extension icon on the tab), which
    can't be synthesized in headless Chromium; the harness sidesteps this by
    granting the local unpacked copy the `<all_urls>` host permission at load
    time (see `patchExtensionForScreenshot`), which an unpacked extension
    receives without a prompt, so the capture runs unattended. If a future
    Chrome/extension change makes even that insufficient, it surfaces as a real
    scenario failure rather than an exit-code-only pass.
- **No parallelism.** Scenarios run sequentially in one isolated config; two
  concurrent `scripts/e2e-run` invocations would race on the same
  `e2e-scenario` profile name if pointed at the same real instance, and (with
  `SNOW_E2E_SN_UTILS=1`) on the SN-Utils harness's fixed isolated ports too.
- **Argument values may not contain literal newlines.** The runner passes
  step stdout/args through newline-delimited plumbing (`jq -r '...[]'`
  piped into a `while read` loop); a `--data` payload or captured value
  containing an embedded newline will be silently truncated at the first one.
