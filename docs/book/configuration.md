# Configuration and authentication

`snow-cli` keeps connection metadata in a config file and stores secrets in the operating system keychain where possible.

## Where configuration lives

By default, profiles are stored in:

```text
~/.servicenow/config.toml
```

The config file contains non-secret settings such as:

- instance URL,
- authentication method,
- username,
- OAuth client ID and grant settings,
- mTLS certificate paths (not yet implemented),
- default profile name.

Secrets such as passwords, API tokens, OAuth client secrets, and stored OAuth tokens are kept outside the TOML file.

## Profiles

Profiles describe how `snow-cli` connects to a ServiceNow instance.

Create a basic-auth profile:

```bash
snow-cli profile add dev \
  --instance https://dev.service-now.com \
  --auth-method basic \
  --username admin
```

List configured profiles:

```bash
snow-cli profile list
```

Set the default profile:

```bash
snow-cli profile default dev
```

Show which profile is active:

```bash
snow-cli profile current
snow-cli profile show
```

Use a specific profile for one command:

```bash
snow-cli --profile dev table list incident --limit 10
```

For detailed profile management, see [`profile`](./commands/profile.md).

## Authentication commands

Once a profile exists, store or refresh credentials with:

```bash
snow-cli auth login
snow-cli auth status
snow-cli auth token
snow-cli auth logout
```

For detailed auth behavior and secret input options, see [`auth`](./commands/auth.md).

## Supported authentication methods

The CLI supports these `--auth-method` values:

- `basic`
- `oauth2`
- `api-key`
- `browser-session`

In `config.toml`, the API key method is serialized as `api_key`.

### Basic authentication

Store `username` in the profile and the password with `auth login`:

```bash
snow-cli profile add dev \
  --instance https://dev.service-now.com \
  --auth-method basic \
  --username admin

snow-cli auth login --profile dev
```

### OAuth 2.0

`snow-cli` supports three OAuth2 grant types:

- `client-credentials`
- `password`
- `authorization-code`

Use `authorization-code` when you need the CLI to act in user scope. That flow uses a browser login, a localhost callback, and PKCE.

See the dedicated guide:

- [OAuth authorization code with PKCE](./oauth-authorization-code-pkce.md)

### API key

Create a profile and store the token:

```bash
snow-cli profile add integration \
  --instance https://dev.service-now.com \
  --auth-method api-key

printf '%s' "$SNOW_API_TOKEN" | snow-cli auth login --profile integration --token-stdin
```

### Browser session

`browser-session` profiles accept an already-authenticated `Cookie` header value from your browser. This is useful for instances that require SSO or SAML, where you can copy the cookie from an authenticated browser session.

Example profile:

```bash
snow-cli profile add sso-dev \
  --instance https://dev.service-now.com \
  --auth-method browser-session
```

Then provide the cookie at runtime via the `SNOW_SESSION_COOKIE` environment variable or the `--session-cookie` flag:

```bash
export SNOW_SESSION_COOKIE='JSESSIONID=...; glide_user_route=...'
snow-cli table list incident --limit 10 --profile sso-dev
```

The session cookie is never stored in the config file or keychain.

## Global options

Common top-level flags:

```bash
snow-cli --profile dev <command>
snow-cli --instance https://override.service-now.com <command>
snow-cli --output json|csv|jsonl|toon|text <command>
snow-cli --timeout-secs 30 <command>
snow-cli -v <command>
snow-cli -vv <command>
```

## Discover more

- [`profile` command reference](./commands/profile.md)
- [`auth` command reference](./commands/auth.md)
- [`OAuth authorization code with PKCE`](./oauth-authorization-code-pkce.md)
- [`Command reference`](./commands.md)
