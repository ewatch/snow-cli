# Secure read-only usage

When you give agents, automation, or team members access to ServiceNow data, it is often safer to ensure they cannot modify anything by accident. `snow-cli` supports this through **read-only mode**, exposed as the `snow-cli-ro` binary and the `--read-only` flag.

This guide explains what read-only mode does at a high level, and how to configure ServiceNow authentication so that even if the client-side policy is bypassed, the underlying identity still cannot write data.

## Why `snow-cli-ro` exists

`snow-cli-ro` is a dedicated entry point that presents a smaller, safer command surface. It is intended for agent harnesses and automated workflows where the only required operations are querying data, inspecting schemas, downloading attachments, or calling known read-only APIs.

### What it does (and does not do)

`snow-cli-ro` enforces a read-only policy through two layers:

1. **Command policy** — it refuses to run subcommands that are known to mutate data (for example `table create`, `table update`, `table delete`, `script run`, or `api post`).
2. **Request policy** — even if a command somehow reaches the HTTP layer, it blocks any authenticated request that uses a method other than `GET`.

These checks are fail-closed: new commands are denied by default until they are explicitly audited and allowed.

This is not a complete sandbox. `snow-cli-ro` cannot prevent a determined agent from using other tools such as `curl`, a browser, or a separate copy of the full `snow-cli` binary. For that reason, the strongest protection comes from combining the client-side policy with **server-side read-only restrictions** on the ServiceNow account or OAuth client.

## Read-only option 1: Basic auth with `snc_read_only`

The simplest way to enforce read-only access is to assign the built-in ServiceNow role **`snc_read_only`** to the user account that `snow-cli` authenticates with.

Users with this role can browse and query tables, but cannot create, update, or delete records through the Table API or the web interface.

### Configure a read-only basic-auth profile

```bash
snow-cli profile add readonly-basic \
  --instance https://dev123456.service-now.com \
  --auth-method basic \
  --username readonly_user

snow-cli auth login --profile readonly-basic
```

In ServiceNow, ensure the `readonly_user` account has only the `snc_read_only` role (and any additional roles needed for table visibility, such as `itil` if you need incident data).

For more details on the `snc_read_only` role, see the [ServiceNow documentation](https://www.servicenow.com/docs/r/platform-administration/user-administration/c_ReadOnlyRole.html).

## Read-only option 2: OAuth 2.0 authorization code with `snc_read_only`

You can also use the browser-based OAuth 2.0 authorization-code flow (with PKCE) for a read-only user. The setup is identical to a normal user-scope profile, but the ServiceNow user account that completes the browser login should have the `snc_read_only` role.

Because the token is bound to that user, every API call inherits the read-only restriction from ServiceNow.

Create the profile exactly as described in the [OAuth authorization code with PKCE](/oauth-authorization-code-pkce/) guide, then log in with a user that has the `snc_read_only` role:

```bash
snow-cli profile add readonly-oauth \
  --instance https://dev123456.service-now.com \
  --auth-method oauth2 \
  --client-id YOUR_CLIENT_ID \
  --oauth-grant-type authorization-code

snow-cli auth login --profile readonly-oauth
```

When the browser opens, sign in with the read-only ServiceNow account.

## Read-only option 3: OAuth 2.0 with user scopes and PKCE

ServiceNow lets you restrict what an OAuth client is allowed to do by assigning **user scopes** to the application registry. When combined with the authorization-code flow and PKCE, you can limit the token to specific API capabilities without relying on the user's role set alone.

### Using the `table_read` scope

If your instance supports it, you can configure the OAuth application to request the **`table_read`** scope. This scope allows querying table records through the Table API but does not grant write access.

The client-side setup is the same as a standard PKCE profile (see [OAuth authorization code with PKCE](/oauth-authorization-code-pkce/)). The difference is in the ServiceNow OAuth application configuration:

1. Open the OAuth application registry in ServiceNow.
2. Ensure the application is configured for the authorization-code flow.
3. In the scope or user-scope configuration, include `table_read` (or the relevant read-only scope for your use case).
4. When creating the `snow-cli` profile, request that scope explicitly:

```bash
snow-cli profile add readonly-scoped \
  --instance https://dev123456.service-now.com \
  --auth-method oauth2 \
  --client-id YOUR_CLIENT_ID \
  --oauth-grant-type authorization-code \
  --oauth-scope table_read

snow-cli auth login --profile readonly-scoped
```

The token returned will be restricted to the capabilities defined by the scope, independent of the user's broader role assignments.

For background on restricting OAuth REST API access with user scopes, see this [ServiceNow Community article](https://www.servicenow.com/community/platform-privacy-security-blog/restrict-access-available-to-oauth-client-using-rest-api-auth/ba-p/2524938).

## Read-only option 4: OAuth 2.0 client credentials

The `client-credentials` grant is useful for unattended automation because it does not require a browser login. However, it is **not ideal for read-only scenarios that involve multiple human users**.

### How it works

```bash
snow-cli profile add readonly-machine \
  --instance https://dev123456.service-now.com \
  --auth-method oauth2 \
  --client-id YOUR_CLIENT_ID \
  --oauth-grant-type client-credentials

printf '%s' "$SNOW_CLIENT_SECRET" | \
  snow-cli auth login --profile readonly-machine --client-secret-stdin
```

In ServiceNow, you can bind the OAuth application to a specific service account that has the `snc_read_only` role, or restrict the client's user scopes to read-only operations.

### Downsides to be aware of

- **Shared identity**: every call made with this token appears in ServiceNow audit logs as the same service account. If multiple people or agents share the same `client_id` and `client_secret`, you cannot tell from the logs which individual performed an action.
- **No user context**: because there is no interactive login, the token is not bound to a specific human user. This can make compliance and incident investigation harder.
- **Secret rotation**: the `client_secret` is a long-lived credential. If it leaks, anyone with the secret can obtain tokens until it is revoked or rotated.

For these reasons, prefer the **authorization-code flow with a read-only user** when you need per-user accountability, and reserve `client-credentials` for clearly isolated machine-to-machine scenarios.

## Summary of recommendations

| Scenario | Recommended setup |
|---|---|
| Quick personal read-only access | Basic auth + `snc_read_only` role |
| Agent harness with audit trail | OAuth 2.0 authorization code + PKCE + `snc_read_only` user |
| Strict API capability restriction | OAuth 2.0 authorization code + PKCE + `table_read` user scope |
| Unattended machine automation | OAuth 2.0 client credentials + dedicated read-only service account |

For the client-side read-only policy details, see the design document on [read-only mode and `snow-cli-ro`](https://github.com/ewatch/snow-cli/blob/main/docs/design/read-only-mode.md).
