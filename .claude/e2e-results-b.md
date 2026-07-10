# E2E Task B Results

## Final Summary

| Metric | Value |
|--------|-------|
| **Total steps** | 8 (with sub-steps) |
| **Sub-operations** | ~18 |
| **Passed** | 12 |
| **Failed** | 5 |
| **Bugs found** | 2 |
| **Broker after restart** | Auto-recovered successfully — new broker spawned, reconnected to browser helper tab, session re-established. |

### Bugs Found

1. **`check-connection` and `get-instance-info` always hang** — These commands send a WebSocket payload to the helper tab and wait for a response, but the helper tab never responds. They hang until timeout regardless of whether a session exists. Unclear if these commands are unimplemented in SN-Utils or if the response message format doesn't match.

2. **`update-record` and `delete-record` direct HTTP path broken** — These use the g_ck token to make direct REST API calls (PUT/DELETE). The debug log shows: *"ServiceNow rejected the cached SN-Utils token; refreshing it via the helper tab"* — the token refresh via the helper tab also times out. The g_ck token from `/token` works for WebSocket bridge operations but is rejected by the direct REST API. This affects both `update-record` and `delete-record`.

---

## Step 1: Build release binary

**Command:** `cargo build --release`
**Result: PASS** — Build completed successfully in 31s.

## Step 2: snu --help

**Command:** `target/release/snow-cli snu --help`
**Result: PASS** — Help displayed successfully, shows all subcommands.

## Step 3: Diagnostics

### 3a. `snu check-connection`
**Result: TIMEOUT** — timed out after 60s. Broker is running and browser connected but no session (needs /token).

### 3b. `snu broker status`
**Result: PASS** — Broker v0.5.1 running, browser connected, session_count=0.

### 3c. `snu get-instance-info`
**Result: TIMEOUT** — timed out after 30s. Same issue — no session.

**Action:** User was asked to run /token in ServiceNow tab. User confirmed /token was pasted.

**/token was requested from user and confirmed executed. Broker status then showed session_count=1 with dev426739 instance active.**

### 3a. `snu check-connection` (retry after /token)
**Result: TIMEOUT** — timed out with no output, even with -v verbose. Bug: SN-Utils helper tab never responds to check_connection payload.

### 3b. `snu broker status` (retry after /token)
**Result: PASS** — Broker v0.5.1, browser_connected=true, session_count=1, instance: https://dev426739.service-now.com (has_g_ck=true).

### 3c. `snu get-instance-info` (retry after /token)
**Result: TIMEOUT** — timed out with no output. Same bug as check-connection.

## Step 4: Schema/browse

### 4a. `snu list-tables`
**Result: PASS** — Listed 6334 tables.

### 4b. `snu schema incident`
**Result: PASS** — Schema returned with full column metadata (parent, made_sla, watch_list, upon_reject, etc.).

## Step 5: Query incident

**Command:** `snu query incident --query active=true --fields sys_id,number,short_description --limit 5`
**Result: PASS** — Returned 5 records including INC0008001, INC0000015, INC0000016, INC0000017, INC0000018.

## Step 6: CRUD operations

### 6a. CREATE
**Command:** `snu create-record incident --data {"short_description":"E2E test incident snu B"}`
**Result: PASS** — Created with sys_id: 833984f82f0e871084cde3807fa4e3ae, number: INC0010004.

### 6b. READ
**Command:** `snu get-record incident <sys_id> --fields sys_id,number,short_description`
**Result: PASS** — Fetched record: INC0010004, short_description: "E2E test incident snu B".

### 6c. UPDATE
**Command:** `snu update-record incident <sys_id> --data '{"short_description":"E2E test incident snu B (updated)"}'`
**Result: FAIL (TIMEOUT)** — timed out after 60s. Also tried --field short_description --content '...' variant — same timeout. Root cause: g_ck token rejected by direct REST API.

### 6d. READ (verify)
**Command:** `snu get-record incident <sys_id> --fields sys_id,number,short_description`
**Result: PASS** — After session re-established, READ showed original data (INC0010004, short_description unchanged — update did not persist).

### 6e. DELETE
**Command:** `snu delete-record incident --sys-id <sys_id>`
**Result: FAIL (TIMEOUT)** — dry-run succeeded (uses bridge), but actual delete timed out after 120s. Same root cause as update-record: g_ck token rejected by direct REST API.

**Note:** Session expired between steps multiple times; user re-ran /token to restore it.

## Step 7: Background script

**Command:** `snu execute-bg-script --code 'gs.info("hello from snu e2e")'`
**Result: PASS** — Script executed successfully: "Script: hello from snu e2e" printed.

## Step 8: Broker resilience

### 8a. `snu broker status` (before stop)
**Result: PASS** — Broker v0.5.1 running, browser_connected=true.

### 8b. `snu broker stop`
**Result: PASS** — Broker stopped successfully ({"stopped":true}). Ports 1978/1979 freed.

### 8c. `snu check-connection` (after stop)
**Result: PARTIAL PASS** — The broker auto-restarted and reconnected (confirmed via broker status: browser_connected=true, session_count=1, instance active). However, check-connection itself still hangs with no output (same bug as step 3).
