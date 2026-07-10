# E2E Task A Results

## Final Summary

| Metric | Value |
|--------|-------|
| Total steps | 8 (Steps 1–8, with Step 5 having 5 sub-steps) |
| Passed | 8/8 |
| Failed | 0/8 |
| Bugs found | 0 |

**Notes:**
- `--output table` is not a valid value; the correct human-readable output flag is `--output text`. The `--help` output lists valid values as: `json`, `csv`, `jsonl`, `toon`, `text`, `auto`.
- Delete requires `--yes` flag for non-interactive use (expected, documented behavior).
- No actual bugs found — all commands function correctly against the live dev instance.

## Step 1: Build the CLI

**Command:** `cargo build --release`

**Result:** PASS — built successfully, binary at `target/release/snow-cli`.

## Step 2: Check help works

**Command:** `target/release/snow-cli --help`

**Result:** PASS — help output displayed with all commands listed.

## Step 3: Configure authentication

**Commands:**
```
target/release/snow-cli profile add e2e --instance 'https://dev426739.service-now.com' --auth-method basic --username admin
printf '%s' 'za3H^aSs!XO0' | target/release/snow-cli auth login --password-stdin --profile e2e
```

**Result:** PASS — profile `e2e` created and credentials stored in OS keychain.

## Step 4: Verify auth works

**Command:** `target/release/snow-cli table list incident --limit 1 --profile e2e`

**Result:** PASS — returned one incident record as JSON.

## Step 5: Table CRUD on incident

### 5a. CREATE
**Command:** `target/release/snow-cli table create incident --data '{"short_description":"E2E test incident snow-cli A"}' --profile e2e`
**Result:** PASS — created, sys_id = `c6f688b42f0e871084cde3807fa4e32a`

### 5b. READ
**Command:** `target/release/snow-cli table get incident c6f688b42f0e871084cde3807fa4e32a --profile e2e`
**Result:** PASS — returned record with `short_description: E2E test incident snow-cli A`

### 5c. UPDATE
**Command:** `target/release/snow-cli table update incident c6f688b42f0e871084cde3807fa4e32a --data '{"short_description":"E2E test incident snow-cli A (updated)"}' --profile e2e`
**Result:** PASS — returned record with `short_description: E2E test incident snow-cli A (updated)`

### 5d. READ again (verify update)
**Command:** `target/release/snow-cli table get incident c6f688b42f0e871084cde3807fa4e32a --profile e2e`
**Result:** PASS — confirmed `short_description: E2E test incident snow-cli A (updated)`

### 5e. DELETE + verify
**Commands:**
```
target/release/snow-cli table delete incident c6f688b42f0e871084cde3807fa4e32a --yes --profile e2e
target/release/snow-cli table get incident c6f688b42f0e871084cde3807fa4e32a --profile e2e
```
**Result:** PASS — delete succeeded, subsequent GET returns 404 NOT_FOUND

## Step 6: Output formats

**Commands:**
```
target/release/snow-cli table list incident --limit 1 --output json --profile e2e
target/release/snow-cli table list incident --limit 1 --output text --profile e2e
target/release/snow-cli table list incident --limit 1 --output auto --profile e2e
```

**Result:** PASS — all three produce distinct, valid output:
- `json`: compact JSON array (valid list of 1)
- `text`: pretty-printed JSON (valid list of 1, same data)
- `auto`: produced TOON format (flat dict, different structure from json/text)

Note: `--output table` is not a valid value (the valid values are `json`, `csv`, `jsonl`, `toon`, `text`, `auto`). Used `text` instead.

## Step 7: Data/query command

**Command:** `target/release/snow-cli data export sys_user --limit 3 --profile e2e`

**Result:** PASS — exported 3 sys_user records in portable JSON package format with metadata (version, kind, instance, timestamp, record_count).

## Step 8: Attachment test

**Commands:**
```
echo "Hello from E2E test attachment" > /tmp/e2e-attachment-test.txt
target/release/snow-cli table create incident --data '{"short_description":"E2E test incident for attachments"}' --profile e2e
target/release/snow-cli attachment upload -f /tmp/e2e-attachment-test.txt incident <sys_id> --profile e2e
target/release/snow-cli attachment list incident <sys_id> --profile e2e
target/release/snow-cli attachment download <attachment_sys_id> -o /tmp/e2e-attachment-downloaded.txt --profile e2e
target/release/snow-cli table delete incident <sys_id> --yes --profile e2e
```

**Result:** PASS — created incident, uploaded text file (31 bytes, text/plain), listed it with correct metadata, downloaded and verified content matches original, cleaned up incident.

