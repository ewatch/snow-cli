# Read-only mode and `snow-cli-ro`

## Summary

snow-cli provides a read-only execution policy for users who want to expose
ServiceNow data access to agents without allowing those agents to mutate
ServiceNow through snow-cli.

There are two user-facing entry points:

- `snow-cli --read-only ...` — full CLI parser with read-only policy enforcement.
- `snow-cli-ro ...` — reduced read-only command surface with a locked read-only
  policy.

The recommended agent-facing executable is `snow-cli-ro`.

## Goals

- Prevent ServiceNow mutations through snow-cli when read-only policy is active.
- Provide a smaller, less confusing CLI surface for agent harnesses.
- Fail closed for commands and HTTP methods that can write or export reusable
  credentials.
- Keep policy decisions centralized and testable.
- Allow `api get` for read-oriented custom APIs, relying on normal HTTP
  semantics that GET should not mutate server state.

## Non-goals

Read-only mode is not a complete sandbox for a shell-capable agent. It does not
prevent mutation through tools outside snow-cli, such as `curl`, browsers, SDKs,
or direct network access. It also cannot prove that every custom GET endpoint is
side-effect-free.

For stronger protection, combine `snow-cli-ro` with deployment controls:

1. Expose only `snow-cli-ro` to the agent harness.
2. Do not expose the full `snow-cli` binary to the same agent environment.
3. Use a ServiceNow account, OAuth client, or API token with read-only roles.
4. Avoid giving agents raw reusable credentials where possible.

## Policy model

The policy model lives in `src/policy.rs`.

Read-only policy has two enforcement layers:

1. **Command policy** — rejects unsafe subcommands before handlers run.
2. **Request policy** — rejects non-GET authenticated HTTP requests before
   credentials are attached and the request is sent.

Policy denials are rendered as structured JSON errors with code
`POLICY_DENIED`.

Example denial:

```json
{
  "error": {
    "code": "POLICY_DENIED",
    "message": "Policy denied table write: read-only policy does not allow table mutations",
    "detail": "mode=ReadOnly; capability=remote_write"
  }
}
```

## Allowed command surface

The following commands are intended to be allowed in read-only mode and exposed
by `snow-cli-ro`:

- `profile list`
- `profile find --instance ...`
- `profile current`
- `profile show`
- `profile sdk list`
- `auth status`
- `table list`
- `table get`
- `table schema`
- `data export`
- `data export-package`
- `data validate`
- `scope list`
- `scope inspect`
- `scope inventory`
- `attachment list`
- `attachment download`
- `api get`
- `codesearch search`
- `completions`

## Denied command surface

The following operations are denied in read-only mode and omitted from
`snow-cli-ro` where possible:

- profile/config writes: `profile add`, `profile edit`, `profile remove`,
  `profile default`, now-sdk import/export
- credential changes/exports: `auth login`, `auth logout`, `auth token`
- table writes: `table create`, `table update`, `table delete`
- import workflows: `data import`, `import-set load`, `import-set transform`
- seed mutations: `seed apply`, `seed cleanup`
- scope mutation: `scope move-file`
- attachment upload: `attachment upload`
- background script execution: `script run`
- raw API write methods: `api post`, `api put`, `api delete`

## `api get` caveat

`api get` is allowed because GET should be safe/read-only by HTTP and API design
convention. A custom endpoint that mutates state on GET is considered a bad API
design, but snow-cli cannot prove that such an endpoint has no side effects.

To reduce accidental method tunneling, read-only mode rejects method override
headers on `api get`, including:

- `X-HTTP-Method-Override`
- `X-Method-Override`
- `X-HTTP-Method`

Use read-only ServiceNow credentials for stronger guarantees.

## Recommended agent deployment

For an agent harness, prefer this setup:

```bash
snow-cli-ro --profile readonly table list incident --query 'active=true' --limit 20
snow-cli-ro --profile readonly api get /api/x_myapp/status
```

Operational recommendations:

1. Configure a dedicated read-only profile/user.
2. Expose `snow-cli-ro` as the only ServiceNow tool in the harness.
3. Do not expose `snow-cli`, `curl`, browser automation, or raw tokens if the
   goal is strict non-mutation.
4. Treat `api get` as an audited escape hatch for known read-oriented endpoints.

## Adding new commands

When adding a new command, update:

- `src/cli/args.rs` for the full CLI parser.
- `src/policy.rs` for the command policy decision.
- `src/cli/readonly_args.rs` if the command should appear in `snow-cli-ro`.
- Integration/unit tests for read-only allow/deny behavior.

New commands should be denied by default until audited.
