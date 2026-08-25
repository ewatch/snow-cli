# `snu`

Use `snu` when you want to drive the SN-Utils browser helper tab directly.

```bash
snow-cli snu <verb> [options]
```

All `snu` subcommands also accept the global flags from the [command overview](/commands/).

> **Prerequisite:** most `snu` commands depend on the SN-Utils browser
> extension being connected to the local broker. Open a ServiceNow browser
> tab with SN-Utils installed and run `/token` there first — without a live
> connection, helper-dependent commands time out waiting for session
> metadata. The exceptions are the broker subcommands (`snu broker status`,
> `snu broker stop`, `snu broker clear`) and `snu check-connection`, which
> work standalone and don't require an active helper tab.

## What it is for

`snu` is the browser-session bridge for actions that need the live SN-Utils helper tab:

- check whether the helper bridge is connected
- inspect instance connection info
- wait for `/token` and print the live browser session metadata
- list tables and fetch records through the active ServiceNow session
- fetch table schema metadata
- update or delete records through the live browser helper session
- execute background scripts through the helper tab
- open slash commands
- activate tabs
- inspect or switch update set / application / domain context
- call ServiceNow REST endpoints through the browser session
- inspect and interact with live forms and pages
- fetch reference/parent options
- take screenshots
- upload attachments

Use [`script`](/commands/script/) for background scripts. That command runs directly against the ServiceNow instance and is not part of the `snu` helper flow.

## Common examples

```bash
snow-cli snu check-connection
snow-cli snu get-instance-info
snow-cli snu list-tables
snow-cli snu get-record incident <sys_id> --fields sys_id,number,short_description
snow-cli snu update-record incident <sys_id> --field state --content 2
snow-cli snu update-record sp_widget <sys_id> --data '{"script":"gs.info(\"hi\")"}'
snow-cli snu delete-record incident --sys-id <sys_id>
snow-cli snu wait-token
snow-cli snu query incident --query 'active=true' --fields sys_id,number --limit 10
snow-cli snu schema incident
snow-cli snu execute-bg-script --code 'gs.info("hello from SN-Utils")'
snow-cli snu slash /tn
snow-cli snu tab activate 'https://dev12345.service-now.com/incident.do*' --open-if-not-found
snow-cli snu context get
snow-cli snu context switch application x_my_app --tab-url 'https://dev12345.service-now.com/*'
snow-cli snu rest get /api/now/table/incident --query-param sysparm_limit=1
snow-cli snu records parent-options sys_db_object --fields sys_id,name --limit 10
snow-cli snu page form-state --fields number,state
snow-cli snu page set-field short_description 'Updated but not submitted'
snow-cli snu page navigate https://dev12345.service-now.com/incident_list.do
snow-cli snu screenshot --url 'https://dev12345.service-now.com/*' --out incident.png
snow-cli snu attachment-upload incident <sys_id> --file ./attachment.png
```

## `snu check-connection`

Check whether the bridge and helper are connected.

```bash
snow-cli snu check-connection
snow-cli snu check-connection --verify
```

`--verify` additionally probes ServiceNow with the cached session and reports
`token_valid`, so you can tell a dead `g_ck` from a healthy one before starting
work. Session freshness (`captured_at`, `last_verified_at`) is included per
instance. A `/token` prompt is only issued when a probe has confirmed the token
is dead; permission (ACL) denials on a valid session are reported as such, and
transient helper-tab errors are retried automatically.

## `snu get-instance-info`

Get instance connection info from the helper.

```bash
snow-cli snu get-instance-info
```

## `snu list-tables`

List table names available on the instance.

```bash
snow-cli snu list-tables
```

## `snu get-record <table> <sys_id>`

Fetch a single record by table and sys_id.

```bash
snow-cli snu get-record incident <sys_id> --fields sys_id,number,short_description
```

## `snu update-record <table> <sys_id>`

Update one or more fields on a record. Use `--data` with a JSON object for
multiple fields, or `--field`/`--content` for a single value (handy for large
contents where JSON escaping is awkward). The two forms are mutually exclusive.

```bash
snow-cli snu update-record incident <sys_id> --field state --content 2
snow-cli snu update-record sp_widget <sys_id> --data '{"script":"gs.info(\"hi\")","css":".c1 { color: red; }"}'
```

## `snu delete-record <table> [--sys-id <sys_id> | --query <encoded-query>]`

Delete a record or delete a limited query result set.

```bash
snow-cli snu delete-record incident --sys-id <sys_id>
snow-cli snu delete-record incident --query 'active=false' --limit 50 --confirm
```

`snu update-record` and `snu delete-record` run their mutation as a server-side
background script over the SN-Utils bridge (the same channel as
`snu execute-bg-script`) and parse a JSON result for success/affected count;
they do not make a direct REST call. `snu delete-record --dry-run` only
previews the matching record(s) and never deletes.

## `snu wait-token`

Wait for the SN-Utils helper tab to emit `/token` and print the browser session metadata.

```bash
snow-cli snu wait-token
```

The `g_ck` token is not stored in the OS keychain. Treat it as live browser-session metadata that is only useful together with the SN-Utils helper connection.

## `snu query <table>`

Query records through the active browser session.

```bash
snow-cli snu query incident --query 'active=true' --fields sys_id,number --limit 20
```

This is useful when you want the browser session and SN-Utils bridge to handle the request instead of the CLI's regular authenticated HTTP client.

## `snu schema <table>`

Fetch table metadata through the helper tab.

```bash
snow-cli snu schema incident
```

## `snu execute-bg-script`

Run a server-side background script through the correlated Agent API. The command prints the helper's returned script output.

```bash
snow-cli snu execute-bg-script --code 'gs.info("hello from SN-Utils")'
snow-cli snu execute-bg-script --file ./cleanup.js
```

If you omit both `--code` and `--file`, `snow-cli` reads the script from stdin.

## `snu slash <command>`

Run a slash command inside a browser tab.

```bash
snow-cli snu slash /tn
snow-cli snu slash tn --no-auto-run
```

## `snu tab activate <url>`

Activate or open a matching tab.

```bash
snow-cli snu tab activate 'https://dev12345.service-now.com/*' --open-if-not-found
```

## `snu context get|switch`

Read the current application/update-set context, or switch update set,
application, or domain context in the browser session.

```bash
snow-cli snu context get
snow-cli snu context switch application x_my_app --tab-url 'https://dev12345.service-now.com/*'
```

## `snu rest <method> <endpoint>`

Use the helper's correlated Agent REST API with the active browser session.
Endpoints must be safe instance-relative paths. Repeat `--query-param` for
multiple unique keys; duplicate keys are rejected rather than silently
collapsed. GET is available in `snow-cli-ro`; write methods are blocked there.

```bash
snow-cli snu rest get /api/now/table/incident --query-param sysparm_limit=1
snow-cli snu rest patch /api/now/table/incident/<sys_id> --data '{"state":"2"}'
```

## `snu records parent-options`

Fetch reference/parent choices with a bounded Table API query.

```bash
snow-cli snu records parent-options sys_db_object --fields sys_id,name --limit 10
```

## `snu page`

Drive the community Agent API for a live ServiceNow tab. `form-state` is a
read; `set-field` changes the unsaved form, while `run-ui-action`, `click`, and
`navigate` can cause browser or instance side effects.

```bash
snow-cli snu page form-state --fields number,state --url 'https://dev12345.service-now.com/*'
snow-cli snu page set-field short_description 'Updated but not submitted'
snow-cli snu page run-ui-action save
snow-cli snu page click '#sysverb_update'
snow-cli snu page navigate https://dev12345.service-now.com/incident_list.do
```

SN-Utils Pro-only Agent actions (`agentCodeSearch` and all `agentCdp*` browser
debugger calls) are intentionally not exposed by `snow-cli snu`.

## `snu screenshot`

Capture a screenshot through the helper tab. The selected instance must have
an active `/token` session so the CLI can preflight its `browserDebugger` gate,
even when targeting the tab by ID or URL pattern.

```bash
snow-cli snu screenshot --url 'https://dev12345.service-now.com/*' --out incident.png
```

## `snu attachment-upload`

Upload a file as an attachment using the active browser session.

```bash
snow-cli snu attachment-upload incident <sys_id> --file ./attachment.png
```

## Notes

- SN-Utils must be installed in the browser, and the helper tab must be reachable at `ws://127.0.0.1:1978`.
- `snow-cli snu` commands auto-start a local broker that owns the SN-Utils WebSocket port and idles out when unused.
- The `g_ck` token is not stored as a reusable credential. The broker keeps live browser-session metadata in memory per instance while it is running.
- If a command waits for session metadata, run `/token` in a ServiceNow tab.
- Community Agent API calls use correlated responses. Pro-only code search and CDP debugger actions remain intentionally unsupported.

### Targeting a specific instance

The SN-Utils tab can be a portal to several ServiceNow instances at once, each
with its own `g_ck`. Every `/token` push is self-describing — it carries the
instance URL alongside the token — so the broker stores one session per
instance, keyed by origin (`scheme://host:port`).

By default a command uses the **most recently active** instance. To pin a
command to a specific instance, pass the global `--instance` flag with a URL or
bare host:

```bash
snow-cli --instance https://dev12345.service-now.com snu query incident --query 'active=true'
```

When the requested instance has no cached token yet, the command prompts you to
run `/token` in a tab for that instance and ignores tokens pushed from other
tabs. `snow-cli snu broker status` lists every instance the broker currently
holds a live `g_ck` for, with the active one flagged.

### Broker lifecycle

The broker starts automatically on the first `snu` command that needs it. It
owns the port hard-coded by SN-Utils (`127.0.0.1:1978`) and accepts foreground
CLI requests on a local broker IPC port. When no clients or requests are active,
it exits after the idle timeout (default 1800 seconds, override with
`SNOW_CLI_SNU_BROKER_IDLE_SECS`).

The `g_ck` token is treated as live browser-session metadata only. The broker
keeps it in memory per instance and, by default, also caches it in a `0600`
file under `~/.servicenow/` so a single `/token` survives across commands and
broker restarts (set `SNOW_CLI_SNU_BROKER_PERSIST=0` to keep it memory-only).
It is never stored in the OS keychain or used as a standalone reusable
credential.

Usually you do not need to manage it. For debugging:

```bash
snow-cli snu broker status
snow-cli snu broker stop
```

To drop cached browser sessions without stopping the broker — for example after
logging out of an instance, or to force a fresh `/token` — use `broker clear`:

```bash
snow-cli snu broker clear                                          # all instances
snow-cli snu broker clear --instance https://dev12345.service-now.com  # just one
```

The next command for a cleared instance re-prompts for `/token`. Clearing is
broker-memory only; it does not affect the ServiceNow session in the browser.

The `sn-scriptsync` VS Code extension and `snow-cli snu` are still mutually
exclusive because both need the same SN-Utils browser port. Stop `sn-scriptsync`
before using `snu`.

## Related pages

- [`script` command reference](/commands/script/)
- [`commands` overview](/commands/)
