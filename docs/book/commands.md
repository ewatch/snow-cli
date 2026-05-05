# Command overview

Use `snow-cli --help` and `snow-cli <noun> --help` for the authoritative command reference.

## Profile

Manage ServiceNow connection profiles.

```bash
snow-cli profile add dev --instance https://dev.service-now.com --auth-method basic --username admin
snow-cli profile edit dev --username new-admin
snow-cli profile default dev
snow-cli profile current
snow-cli profile list
snow-cli profile show
snow-cli profile remove old-dev
```

`config` is available as an alias for `profile`.

## Auth

Store and remove credentials, check login status, and retrieve tokens where supported.

```bash
snow-cli auth login
snow-cli auth login --password '<password>'
snow-cli auth status
snow-cli auth token
snow-cli auth logout
```

## Table API

Perform CRUD operations against any ServiceNow table.

```bash
snow-cli table list incident --query 'active=true' --limit 20
snow-cli table get incident <sys_id>
snow-cli table create incident --data '{"short_description":"Disk alert"}'
snow-cli table update incident <sys_id> --data '{"state":"2"}'
snow-cli table delete incident <sys_id> --yes
snow-cli table schema incident --extended
```

## Raw API

Call arbitrary REST endpoints.

```bash
snow-cli api get /api/now/table/incident?sysparm_limit=1
snow-cli api post /api/x_myapp/action --data '{"dry_run":true}'
snow-cli api get /api/x_myapp/status -H 'X-Trace-Id:abc123'
```

## Script

Run a ServiceNow background script.

```bash
snow-cli script run --script 'gs.info("hello from snow-cli")'
```

## Code search

Search code and metadata on the instance.

```bash
snow-cli codesearch search "GlideRecord"
```

## Attachments

Work with attachments.

```bash
snow-cli attachment --help
```

## Import sets

Load rows into import set tables.

```bash
snow-cli import-set load imp_user --data '{"user_name":"snow-cli-user","email":"snow-cli-user@example.com"}'
echo '{"user_name":"stdin-user"}' | snow-cli import-set load imp_user
```

## Data workflows

Export, validate, and import portable datasets.

```bash
snow-cli data export incident --query 'active=true'
snow-cli data export sys_user --fields sys_id,user_name,email --out users.json
snow-cli data validate --file users.json
snow-cli data import --file users.json
```

## Seed workflows

Plan, apply, and clean up declarative test data.

```bash
snow-cli seed plan --file qa-fixture.json
snow-cli seed apply --file qa-fixture.json
snow-cli seed cleanup <run-id> --dry-run
```

## Scope workflows

Inspect ServiceNow application scope metadata.

```bash
snow-cli scope list
snow-cli scope inspect x_my_app
```

## Shell completions

Generate completions for supported shells.

```bash
snow-cli completions bash
snow-cli completions zsh
snow-cli completions fish
snow-cli completions powershell
snow-cli completions elvish
```
