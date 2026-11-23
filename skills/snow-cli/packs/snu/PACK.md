# SN-Utils Browser Bridge Pack (`snow-cli-ro snu`)

Opt-in pack for working through a live SN-Utils browser helper tab (`ws://127.0.0.1:1978`) when you need active browser SSO/MFA session context, live form inspection, screenshots, or browser-driven REST/query calls.

Read-only `snu` verbs (`check-connection`, `wait-token`, `get-instance-info`, `app-meta`, `list-tables`, `get-record`, `query`, `schema`, `context get`, `rest get`, `records parent-options`, `page form-state`, `screenshot`) work in `snow-cli-ro`. (`snu broker *` commands are only available in the full `snow-cli` binary.)

## 1. Check bridge health & active instances first

```bash
# Verify live helper connection and probe token_valid against ServiceNow
snow-cli-ro snu check-connection --verify
snow-cli-ro snu get-instance-info

# Pin a command to one specific instance when multiple tabs are open
snow-cli-ro --instance https://dev12345.service-now.com snu context get

# Full snow-cli binary only: inspect broker-cached sessions across multiple tabs/instances
snow-cli snu broker status
```

If no session is cached yet, ask the user to run `/token` in their ServiceNow browser tab (or wait via `snow-cli-ro snu wait-token`).

## 2. Read-only inspection through the browser session

Useful when MFA/SSO prevents Basic/OAuth profiles, or when verifying exactly what the live browser user sees:

```bash
# Query tables, inspect a record, or fetch table schema via the browser session
snow-cli-ro snu query incident --query 'active=true' --fields sys_id,number,short_description --limit 20
snow-cli-ro snu get-record incident <sys_id> --fields sys_id,number,state,short_description
snow-cli-ro snu schema incident

# Inspect scoped app metadata or call any safe GET REST endpoint
snow-cli-ro snu app-meta x_my_app
snow-cli-ro snu rest get /api/now/table/incident --query-param sysparm_limit=5

# Read live unsaved form state on an open ServiceNow tab & capture a screenshot
snow-cli-ro snu page form-state --fields number,state,short_description --url 'https://dev12345.service-now.com/*'
snow-cli-ro snu screenshot --url 'https://dev12345.service-now.com/*' --out incident.png
```

## 3. Guarded write & browser-action commands (only when explicitly asked)

Never run mutating `snu` commands without explicit user intent; SN-Utils **Agent Access** review gates (`approve` / `off`) may also hold or reject them.

```bash
# Switch browser session context (application / updateset / domain)
snow-cli snu context switch application x_my_app --tab-url 'https://dev12345.service-now.com/*'

# Preview before deleting (dry-run never mutates)
snow-cli snu delete-record incident --query 'active=false' --limit 10 --dry-run

# Single-field update (handy for scripts where JSON escaping is noisy)
snow-cli snu update-record incident <sys_id> --field state --content 2
```

## 4. Broker cleanup & troubleshooting

```bash
# Drop stale cached g_ck tokens after logging out (does not log out the browser)
snow-cli snu broker clear --instance https://dev12345.service-now.com
snow-cli snu broker stop
```

## 5. Gotchas

- **Mutually exclusive with `sn-scriptsync`:** VS Code's `sn-scriptsync` extension uses the same `127.0.0.1:1978` SN-Utils port; stop it before running `snow-cli-ro snu`.
- **`SNU_APPROVED_NO_REPLY`:** If an approved mutating command loses its reply, it **may already have executed** on the instance — inspect the record before retrying.
- **`snu update-record` / `snu delete-record` run as background scripts:** They execute over the SN-Utils bridge script channel, not the Table REST API.
- **`snu page run-ui-action` requires one exact instance URL:** Pass `--url 'https://dev12345.service-now.com/incident.do*'` (wildcard hosts like `https://*.service-now.com/*` and `--tab-id` are refused).
- **Prefer native `snow-cli` when Basic/OAuth or `session` works:** Direct Table/Aggregate/Include/Flow commands are faster and avoid browser tab dependencies; use `snu` specifically when you need the live browser tab/SSO session.
