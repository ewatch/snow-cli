# `snu`

Use `snu` when you want to drive the SN-Utils browser helper tab directly.

```bash
snow-cli snu <verb> [options]
```

All `snu` subcommands also accept the global flags from the [command overview](../commands.md).

## What it is for

`snu` is the browser-session bridge for actions that need the SN-Utils helper tab and its cached `g_ck` token:

- check whether the helper bridge is connected
- inspect instance connection info
- wait for and cache `/token`
- list tables and fetch records through the active ServiceNow session
- fetch table schema metadata
- update or delete records through the direct ServiceNow API
- execute background scripts through the helper tab
- open slash commands
- activate tabs
- switch update set / application / domain context
- take screenshots
- upload attachments

Use [`script`](./script.md) for background scripts. That command runs directly against the ServiceNow instance and is not part of the `snu` helper flow.

## Common examples

```bash
snow-cli snu check-connection
snow-cli snu get-instance-info
snow-cli snu list-tables
snow-cli snu get-record incident <sys_id> --fields sys_id,number,short_description
snow-cli snu update-record incident <sys_id> --field state --content 2
snow-cli snu update-record-batch sp_widget <sys_id> --fields '{"script":"gs.info(\"hi\")"}'
snow-cli snu delete-record incident <sys_id>
snow-cli snu wait-token
snow-cli snu query incident --query 'active=true' --fields sys_id,number --limit 10
snow-cli snu schema incident
snow-cli snu execute-bg-script --code 'gs.info("hello from SN-Utils")'
snow-cli snu slash /tn
snow-cli snu tab activate 'https://dev12345.service-now.com/incident.do*' --open-if-not-found
snow-cli snu context switch application x_my_app --tab-url 'https://dev12345.service-now.com/*'
snow-cli snu screenshot --url 'https://dev12345.service-now.com/*' --out incident.png
snow-cli snu attachment upload incident <sys_id> --file ./attachment.png
```

## `snu check-connection`

Check whether the bridge and helper are connected.

```bash
snow-cli snu check-connection
```

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

Update one field on a record.

```bash
snow-cli snu update-record incident <sys_id> --field state --content 2
```

## `snu update-record-batch <table> <sys_id>`

Update multiple fields on the same record.

```bash
snow-cli snu update-record-batch sp_widget <sys_id> --fields '{"script":"gs.info(\"hi\")"}'
```

## `snu delete-record <table> [--sys-id <sys_id> | --query <encoded-query>]`

Delete a record or delete a limited query result set.

```bash
snow-cli snu delete-record incident <sys_id>
snow-cli snu delete-record incident --query 'active=false' --limit 50 --confirm
```

## `snu wait-token`

Wait for the SN-Utils helper tab to emit `/token` and print the browser session metadata.

```bash
snow-cli snu wait-token
```

The cached session is stored in the OS keychain so later `snu` commands can reuse it without prompting again.

## `snu query <table>`

Query records through the active browser session.

```bash
snow-cli snu query incident --query 'active=true' --fields sys_id,number --limit 20
```

This is useful when you want the browser session and SN-Utils bridge to handle the request instead of the CLI's direct HTTP client.

## `snu schema <table>`

Fetch table metadata through the helper tab.

```bash
snow-cli snu schema incident
```

## `snu execute-bg-script`

Run a server-side background script through the browser helper session. The command prints the helper's returned `data` payload.

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

## `snu context switch <type> <value>`

Switch update set, application, or domain context in the browser session.

```bash
snow-cli snu context switch application x_my_app --tab-url 'https://dev12345.service-now.com/*'
```

## `snu screenshot`

Capture a screenshot through the helper tab.

```bash
snow-cli snu screenshot --url 'https://dev12345.service-now.com/*' --out incident.png
```

## `snu attachment upload`

Upload a file as an attachment using the active browser session.

```bash
snow-cli snu attachment upload incident <sys_id> --file ./attachment.png
```

## Notes

- SN-Utils must be installed in the browser, and the helper tab must be reachable at `ws://127.0.0.1:1978`.
- The cached browser session is stored in a dedicated internal OS keychain entry alongside the instance URL and reused by later `snu` commands when present.
- If no cached session exists, `wait-token` is the first command you should run.
- Some helper actions are available in the SN-Utils extension but are not yet exposed as `snow-cli` commands, including `get_file_structure`, `query_records`, `get_form_state`, `set_field`, `run_ui_action`, `navigate`, `navigate_and_screenshot`, `rest_request`, `run_slash_command`, `switch_context`, `take_screenshot`, `upload_attachment`, `refresh_preview`, and `code_search`.

### Persistent bridge (single daemon)

The first `snu` command starts a WebSocket bridge daemon on `ws://127.0.0.1:1978`.
Subsequent `snu` commands will fail with **"Address already in use"** because
the port is already taken.

**Workaround:** Use a single shell session for all `snu` commands, or set
a custom port via the `SNU_BRIDGE_PORT` environment variable if you need
parallel sessions.

```bash
# Session 1 (port 1978)
snow-cli snu query incident --limit 10

# Session 2 (port 1979)
SNU_BRIDGE_PORT=1979 snow-cli snu query sys_user --limit 10
```

The bridge remains active until the `snow-cli` process exits. You do not
need to restart it between commands within the same invocation.

## Related pages

- [`script` command reference](./script.md)
- [`commands` overview](../commands.md)
