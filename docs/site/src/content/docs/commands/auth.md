# `auth`

Use `auth` to store, inspect, and clear credentials for the active profile.

```bash
snow-cli auth <verb> [options]
```

All `auth` subcommands also accept the global flags from the [command overview](/commands/).

## `auth login`

Authenticate and store credentials in the OS keychain.

```bash
snow-cli auth login [options]
```

`auth login` behaves differently depending on the profile's `auth_method`.

### Which credentials are needed?

| Profile auth method | What must already be in the profile | What `auth login` needs |
|---|---|---|
| `basic` | `username` | password |
| `oauth2` + `client-credentials` | `client_id` | client secret |
| `oauth2` + `password` | `client_id`, `username` | client secret and password |
| `oauth2` + `authorization-code` | `client_id`, optional redirect/scope settings | browser login, optional client secret |
| `api-key` | no extra profile secret fields | API token |
| `browser-session` | no extra profile fields | session cookie via env var or flag; `auth login` can validate/print guidance but does not store it |

### Secret input options

Prefer interactive prompts or `--*-stdin` flags over putting secrets directly on the command line.

Important options:

- `--password` / `--password-stdin`: basic auth password or OAuth password-grant password
- `--token` / `--token-stdin`: API token for API-key profiles
- `--client-secret` / `--client-secret-stdin`: OAuth client secret
- `--session-cookie` / `--session-cookie-stdin`: full authenticated `Cookie` header value for browser-session profiles
- `--no-browser`: print the OAuth authorization URL instead of trying to open it automatically
- `--also-now-sdk`: for basic auth, also write the successful login into `now-sdk`
- `--now-sdk-alias <name>`: destination alias name when using `--also-now-sdk`
- `--set-now-sdk-default`: mark that `now-sdk` alias as default

Examples:

```bash
snow-cli auth login
printf '%s' "$SNOW_PASSWORD" | snow-cli auth login --password-stdin
printf '%s' "$SNOW_API_TOKEN" | snow-cli auth login --token-stdin
printf '%s' "$SNOW_CLIENT_SECRET" | snow-cli auth login --client-secret-stdin
printf '%s' "$SNOW_SESSION_COOKIE" | snow-cli auth login --session-cookie-stdin
```

### OAuth authorization-code login

For authorization-code profiles, `snow-cli`:

1. starts a temporary localhost callback listener,
2. generates PKCE values,
3. opens the authorization URL or prints it when `--no-browser` is used,
4. waits for the redirect,
5. exchanges the code for tokens,
6. stores the resulting OAuth token set securely.

Public PKCE clients can omit the client secret. Confidential clients can provide one.

See [OAuth authorization code with PKCE](/oauth-authorization-code-pkce/).

### Browser session

For `browser-session` profiles, provide the full authenticated `Cookie` header value via the `SNOW_SESSION_COOKIE` environment variable or the `--session-cookie` / `--session-cookie-stdin` flags. The cookie is not stored; export it as `SNOW_SESSION_COOKIE` for future requests.

## `auth status`

Show the current authentication state for the active profile.

```bash
snow-cli auth status
snow-cli auth status --verify
```

Sample output when credentials are stored:

```json
{
  "profile": "dev",
  "instance": "https://dev.service-now.com",
  "auth_method": "basic",
  "credential_types": ["password"],
  "credentials_present": true,
  "authenticated": true,
  "username": "admin"
}
```

`credentials_present` only confirms that credentials are available in the
keychain or environment. It does **not** prove the instance accepts them.
`authenticated` is a deprecated alias with the same value, kept for one release.

Add `--verify` to make one authenticated request. This is the command to use
to check credentials, identity, and connectivity in a single call:

```json
{
  "...": "...",
  "credentials_present": true,
  "verified": true,
  "verified_user": "admin",
  "verified_user_sys_id": "6816f79cc0a8016401c5a33be04be441",
  "latency_ms": 182,
  "build": "glide-zurich-07-01-2026__patch1"
}
```

- `verified_user` and `verified_user_sys_id` come from `sys_user` filtered by
  `javascript:gs.getUserID()`, so they name the session user even for OAuth
  and browser-session profiles. They are `null` if the user row is not
  readable.
- `latency_ms` is the round trip of that identity request, including any OAuth
  token acquisition.
- `build` is read on a best-effort basis from `sys_properties`
  (`glide.buildtag`, then `glide.buildtag.last`, `glide.war`,
  `glide.war.assigned`). It is `null` when those properties are not readable,
  which is common for non-admin users.

When verification fails, the output reports `"verified": false` with a
`verification_error` code (for example `UNAUTHORIZED`, or `REQUEST_FAILED` for
network errors), the structured error is written to stderr, and the command
exits non-zero (`5` for API errors). `auth status --verify` only reads data,
so it also works in `snow-cli-ro`.

The output honours `--output` like other commands.

Use `snow-cli auth login` to update stale credentials.

## `auth token`

Print the current access token to stdout.

```bash
snow-cli auth token
```

This is useful for piping into other tools. For OAuth2 profiles, the command prints an actual short-lived access token rather than a stored client secret.

Examples:

```bash
# Copy to clipboard (macOS)
snow-cli auth token | pbcopy

# Use in another API call
TOKEN=$(snow-cli auth token)
curl -H "Authorization: Bearer $TOKEN" https://instance.service-now.com/api/now/table/incident
```

For basic auth profiles, the output is a base64-encoded `username:password` string.

## `auth logout`

Remove stored credentials for the active profile.

```bash
snow-cli auth logout
```

This clears the credential entries used by the current auth method.

## Common workflows

### Basic auth

```bash
snow-cli profile add dev \
  --instance https://dev.service-now.com \
  --auth-method basic \
  --username admin

snow-cli auth login --profile dev
```

### OAuth2 client credentials

```bash
snow-cli profile add integration \
  --instance https://dev.service-now.com \
  --auth-method oauth2 \
  --client-id abc123 \
  --oauth-grant-type client-credentials

printf '%s' "$SNOW_CLIENT_SECRET" | \
  snow-cli auth login --profile integration --client-secret-stdin
```

### OAuth2 authorization code with PKCE

```bash
snow-cli profile add user-scope \
  --instance https://dev.service-now.com \
  --auth-method oauth2 \
  --client-id abc123 \
  --oauth-grant-type authorization-code

snow-cli auth login --profile user-scope
```

## Related pages

- [Configuration and authentication](/configuration/)
- [`profile` command reference](/commands/profile/)
- [OAuth authorization code with PKCE](/oauth-authorization-code-pkce/)
