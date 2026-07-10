# E2E Task B: snow-cli `snu` (SN-Utils WebSocket bridge) against live dev instance

You are testing the `snu` command family of the `snow-cli` Rust CLI end-to-end. These commands talk to the SN-Utils browser extension over a local WebSocket broker (broker owns port 1978 for the extension, 1979 for IPC).

## Preconditions (already satisfied by the user — do NOT try to set these up)
- The user's browser has the instance `https://dev426739.service-now.com` open, logged in as admin.
- The SN-Utils ScriptSync helper tab is open and will connect to the broker's WebSocket.

## Rules
- Work step by step. After EACH step, append the command you ran and its result (PASS/FAIL + short output snippet) to the report file `.claude/e2e-results-b.md`.
- Do NOT modify any source code. You only build and run the CLI.
- If a command fails, record the failure verbatim and move on. Do not get stuck retrying more than twice.
- Wait for Task A's build if needed: if `cargo build --release` says the target directory is locked, wait and retry.
- If a command says to run `/token` in a ServiceNow tab, record that in the report and tell the user in your output — the user is watching and can do it — then retry the command after ~30 seconds.

## Steps

1. Build (or reuse) the CLI: `cargo build --release`. Binary: `target/release/snow-cli`.
2. Read the snu help: `target/release/snow-cli snu --help`.
3. Diagnostics first:
   a. `target/release/snow-cli snu check-connection`
   b. `target/release/snow-cli snu broker status`
   c. `target/release/snow-cli snu get-instance-info`
4. **Schema/browse:** `target/release/snow-cli snu list-tables` (record count only, not full output) and `target/release/snow-cli snu schema incident`.
5. **Query:** `target/release/snow-cli snu query incident --query 'active=true' --fields sys_id,number,short_description --limit 5`
6. **Record CRUD through the browser session:**
   a. CREATE: `target/release/snow-cli snu create-record incident --data '{"short_description":"E2E test incident snu B"}'` — record the sys_id.
   b. READ: `snu get-record incident <sys_id> --fields sys_id,number,short_description`
   c. UPDATE: `snu update-record incident <sys_id> --data '{"short_description":"E2E test incident snu B (updated)"}'`
   d. READ again, verify the update.
   e. DELETE: `snu delete-record incident <sys_id>`, then verify get-record now fails.
7. **Background script via browser helper:** `target/release/snow-cli snu execute-bg-script --code 'gs.info("hello from snu e2e")'`
8. **Broker resilience:** run `snu broker status`, then `snu broker stop`, then run `snu check-connection` again and verify the broker auto-restarts and reconnects.
9. Write a final summary at the top of `.claude/e2e-results-b.md`: total steps, passed, failed, any bugs found, and how the broker behaved after restart.

Start now with step 1.
