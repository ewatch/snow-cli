# Diagnostics, Logs, Jobs & Audit

Use when answering: "Why is this script failing or timing out?", "Did this scheduled job run?", "Who changed this field and when?", or "Was a record deleted?"

All commands below are read-only and use `snow-cli-ro`.

## 1. Key tables

| Table | What it stores | Key fields to project |
|---|---|---|
| `syslog` | Application & script logs (`gs.error`/`gs.warn`/`gs.info`) | `level,source,message,sys_created_on,sys_created_by` |
| `syslog_transaction` | HTTP/API/UI transaction timings | `url,type,response_time,sql_time,business_rule_time,http_response_code,sys_created_by,sys_created_on` |
| `sys_trigger` | Scheduled execution queue (active/stuck jobs) | `name,state,next_action,system_id,trigger_type,document,document_key,error_count` |
| `sysauto_script` | Scheduled Script Executions | `sys_id,name,active,run_type,run_dayofweek,run_time,conditional,sys_scope` |
| `sys_audit` | Field-level change history on audited tables | `tablename,documentkey,fieldname,oldvalue,newvalue,user,sys_created_on` |
| `sys_audit_delete` | Deleted record audit & recovery trail | `tablename,documentkey,display_value,sys_created_by,sys_created_on` |
| `sys_cluster_state` | Active cluster nodes & status | `node_id,system_id,status,most_recent_message` |

## 2. Inspect script errors and slow transactions

Always filter logs by **time window** and **narrow fields** — log tables are huge and rotated daily.

```bash
# 1. Recent script/platform errors (level=2 is Error, 1 is Warning, 0 is Info)
snow-cli-ro table list syslog \
  --query 'level=2^sys_created_on>=javascript:gs.hoursAgoStart(4)' \
  --fields sys_created_on,source,message,sys_created_by \
  --order-by -sys_created_on --limit 30

# Filter errors by a specific script or scope name
snow-cli-ro table list syslog \
  --query 'sourceLIKEChangeRequest^ORmessageLIKEChangeRequest' \
  --fields sys_created_on,level,source,message \
  --order-by -sys_created_on --limit 25

# 2. Slowest recent transactions (response_time in ms)
snow-cli-ro table list syslog_transaction \
  --query 'sys_created_on>=javascript:gs.hoursAgoStart(4)^response_time>3000' \
  --fields sys_created_on,url,type,response_time,sql_time,business_rule_time,http_response_code,sys_created_by \
  --order-by -response_time --limit 20
```

## 3. Check Scheduled Jobs (`sysauto_script` & `sys_trigger`)

```bash
# Find scheduled script definitions
snow-cli-ro table list sysauto_script \
  --query 'active=true^nameLIKEcleanup' \
  --fields sys_id,name,run_type,run_time,conditional,sys_scope.scope --limit 30

# Read a scheduled job's script body
snow-cli-ro table get sysauto_script <sys_id_or_name> --fields script --raw

# Check its runtime trigger state (0=Ready, 1=Running, 2=Error, 3=Queued)
snow-cli-ro table list sys_trigger \
  --query 'nameLIKEcleanup' \
  --fields sys_id,name,state,next_action,system_id,error_count --display-value both --limit 20

# Find stuck or erroring jobs across the instance
snow-cli-ro table list sys_trigger \
  --query 'state=2^ORstate=1' \
  --fields name,state,next_action,system_id,error_count \
  --order-by next_action --display-value both --limit 30
```

## 4. Trace field history & deletions (`sys_audit` / `sys_audit_delete`)

```bash
# 1. Confirm the table is audited (audit=true on its sys_dictionary collection row)
snow-cli-ro table list sys_dictionary \
  --query 'nameINincident,task^internal_type=collection' \
  --fields name,audit

# 2. Field change history for one record (always filter by BOTH tablename and documentkey!)
snow-cli-ro table list sys_audit \
  --query 'tablename=incident^documentkey=<record_sys_id>' \
  --fields sys_created_on,fieldname,oldvalue,newvalue,user \
  --order-by -sys_created_on --limit 50

# 3. Check if a record was deleted
snow-cli-ro table list sys_audit_delete \
  --query 'tablename=incident^display_value=INC0010001' \
  --fields tablename,documentkey,display_value,sys_created_by,sys_created_on \
  --order-by -sys_created_on --limit 10
```

## 5. Gotchas

- **Never query `sys_audit` or `syslog` unfiltered:** Both tables contain millions of rows and will time out without indexed filters (`tablename` + `documentkey` on `sys_audit`; `sys_created_on` on `syslog`/`syslog_transaction`). Increase `--timeout-secs 60` if needed.
- **`sys_audit` only covers audited tables:** If `audit=false` on the `sys_dictionary` collection row (or the field has `no_audit=true`), no `sys_audit` rows exist — and `gr.autoSysFields(false)` / `setWorkflow(false)` can also alter audit behavior.
- **Rotated log tables:** `syslog` and `syslog_transaction` use table rotation; queries older than the retention window return empty even if the event happened previously.
- **Transaction timing breakdown:** Compare `sql_time` vs `business_rule_time` vs `response_time` in `syslog_transaction` to immediately distinguish slow queries from slow Business Rules/scripts.
- **Event-driven async jobs also live in `sys_trigger`:** Async Business Rules and Flow executions enqueue rows in `sys_trigger` (`document`/`document_key` point back to the source record).
