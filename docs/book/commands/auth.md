# `auth`

Use `auth` to store, inspect, and clear credentials for the active profile.

```bash
snow-cli auth <verb> [options]
```

All `auth` subcommands also accept the global flags from the [command overview](../commands.md).

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
| `browser-session` | no extra profile fields | session cookie via env var or flag; `auth login` is not used |

### Secret input options

Prefer interactive prompts or `--*-stdin` flags over putting secrets directly on the command line.

Important options:

- `--password` / `--password-stdin`: basic auth password or OAuth password-grant password
- `--token` / `--token-stdin`: API token for API-key profiles
- `--client-secret` / `--client-secret-stdin`: OAuth client secret
- `--session-cookie` / `--session-cookie-stdin`: full authenticated `Cookie` header value for SAML profiles
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

See [OAuth authorization code with PKCE](../oauth-authorization-code-pkce.md).

### Browser session

For `browser-session` profiles, provide the full authenticated `Cookie` header value via the `SNOW_SESSION_COOKIE` environment variable or the `--session-cookie` / `--session-cookie-stdin` flags. This auth method does not use `auth login`.

## `auth status`

Show the current authentication state for the active profile.

```bash
snow-cli auth status
```

Use it to confirm whether the required credentials are available.

## `auth token`

Print the current access token to stdout.

```bash
snow-cli auth token
```

This is useful for piping into other tools. For OAuth2 profiles, the command prints an actual short-lived access token rather than a stored client secret.

Example:

```bash
snow-cli auth token | pbcopy
```

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

- [Configuration and authentication](../configuration.md)
- [`profile` command reference](./profile.md)
- [OAuth authorization code with PKCE](../oauth-authorization-code-pkce.md)
