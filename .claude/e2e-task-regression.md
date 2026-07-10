# Regression Test: `snow-cli snu` after bug fixes

You are running a REGRESSION test of the `snu` command family of `snow-cli`. Two bugs were just fixed; your job is to verify the fixes work against a live instance and that nothing that previously worked has broken.

## Context: what was fixed
- **Fix 1:** `snu check-connection` and `snu get-instance-info` used to HANG until timeout. They were rebuilt on broker state + a live helper-tab probe. They must now return quickly (a few seconds) with useful JSON.
- **Fix 2:** `snu update-record` and `snu delete-record` used to FAIL with token-rejection timeouts (broken direct REST path). They now run the mutation as a server-side background script over the SN-Utils bridge. They must now actually persist/delete.

## Preconditions (already satisfied — do NOT set these up)
- Binary is already built at `target/release/snow-cli` (do NOT rebuild).
- The user's browser has `https://dev426739.service-now.com` open, logged in as admin, with the SN-Utils ScriptSync helper tab open.
- A leftover incident from the previous test exists: sys_id `833984f82f0e871084cde3807fa4e3ae` (INC0010004, short_description "E2E test incident snu B").

## Rules
- After EACH step, append the command and result (PASS/FAIL/REGRESSION + short output snippet) to `.claude/e2e-results-regression.md`.
- Mark a step **REGRESSION** if something that passed in the previous run now fails.
- Do NOT modify source code. Do NOT run cargo build.
- If a command prints that you should run `/token` in a ServiceNow tab, say so clearly in your chat output (the user is watching and will do it), wait ~30 seconds, then retry once.
- If a command hangs more than 90 seconds, Ctrl-C it, record FAIL (hang), and continue.

## Steps

### Part 1 — Fixed diagnostics (must respond fast now)
1. `target/release/snow-cli snu broker status` — broker should auto-start.
2. `target/release/snow-cli snu check-connection` — MUST return within seconds with JSON including `connected` and `browser_connected` fields. Record elapsed time.
3. `target/release/snow-cli snu get-instance-info` — MUST return instance metadata (url, has_g_ck, ...) or a clear error telling you to run /token. NOT a hang. Record elapsed time.

### Part 2 — Previously working commands (must still work)
4. `target/release/snow-cli snu list-tables` — record only the table count.
5. `target/release/snow-cli snu schema incident` — record PASS/FAIL only.
6. `target/release/snow-cli snu query incident --query 'active=true' --fields sys_id,number,short_description --limit 3`
7. `target/release/snow-cli snu execute-bg-script --code 'gs.info("regression test bg script")'`

### Part 3 — Fixed mutations (the core regression target)
8. UPDATE the leftover incident: `target/release/snow-cli snu update-record incident 833984f82f0e871084cde3807fa4e3ae --data '{"short_description":"E2E regression: update works now"}'`
9. VERIFY: `target/release/snow-cli snu get-record incident 833984f82f0e871084cde3807fa4e3ae --fields sys_id,number,short_description` — short_description MUST be "E2E regression: update works now".
10. Test update with tricky characters: `target/release/snow-cli snu update-record incident 833984f82f0e871084cde3807fa4e3ae --data '{"description":"quotes \" and backslash \\ and\nnewline test"}'` then get-record with `--fields description` and verify it round-trips.
11. DRY-RUN delete first: `target/release/snow-cli snu delete-record incident --sys-id 833984f82f0e871084cde3807fa4e3ae --dry-run` — must preview only, record must still exist after.
12. REAL delete: `target/release/snow-cli snu delete-record incident --sys-id 833984f82f0e871084cde3807fa4e3ae` (add a confirm flag if `--help` says one is required non-interactively).
13. VERIFY deletion: get-record for the same sys_id MUST now fail / return nothing.
14. Full create→update→delete cycle on a fresh record: create incident with short_description "E2E regression cycle", update it to "E2E regression cycle (updated)", verify, delete it, verify gone.

### Part 4 — Summary
15. Write a final summary at the TOP of `.claude/e2e-results-regression.md`: steps passed/failed, elapsed times for steps 2-3, whether both fixes are confirmed working live, and any REGRESSIONS found.

Check the exact flag syntax with `--help` on any subcommand before using it. Start now with step 1.
