# E2E Task A: snow-cli core commands against live dev instance

You are testing the `snow-cli` Rust CLI end-to-end against a real ServiceNow developer instance.

## Instance credentials
- Instance URL: `https://dev426739.service-now.com`
- Username: `admin`
- Password: `za3H^aSs!XO0`

## Rules
- Work step by step. After EACH step, append the command you ran and its result (PASS/FAIL + short output snippet) to the report file `.claude/e2e-results-a.md`.
- Do NOT modify any source code. You only build and run the CLI.
- If a command fails, record the failure and move on to the next step. Do not get stuck.
- The password contains `^` and `!` — always single-quote it in shell commands.

## Steps

1. Build the CLI: `cargo build --release`. The binary will be at `target/release/snow-cli`.
2. Check help works: `target/release/snow-cli --help`
3. Configure authentication. First inspect `target/release/snow-cli auth --help` and `target/release/snow-cli profile --help` to learn the exact syntax, then set up a profile named `e2e` for the instance above using basic auth with the credentials given.
4. Verify auth works: run a simple read, e.g. `target/release/snow-cli table list incident --limit 1` (check `table --help` first for exact syntax).
5. **Table CRUD on `incident`:**
   a. CREATE: create an incident with short_description `E2E test incident snow-cli A` — record the returned sys_id.
   b. READ: get that record by sys_id.
   c. UPDATE: update its short_description to `E2E test incident snow-cli A (updated)`.
   d. READ again and verify the update took effect.
   e. DELETE: delete the record, then verify a GET now fails/returns nothing.
6. **Output formats:** run the same list query with `--output json`, `--output table`, and `--output auto` (check `--help` for the exact flag name) and record whether each produces distinct, valid output.
7. **Data/query command:** inspect `snow-cli data --help` and run one representative query against `sys_user` (e.g. list 3 users with a sysparm query).
8. **Attachment:** create a small text file, attach it to a new test incident, list attachments, download it back, verify content matches, then delete the test incident.
9. Write a final summary at the top of `.claude/e2e-results-a.md`: total steps, passed, failed, and any bugs found.

Start now with step 1.
