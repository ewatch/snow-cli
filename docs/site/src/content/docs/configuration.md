# Configuration and authentication

`snow-cli` keeps connection metadata in a config file and stores secrets outside it in an AES-256-GCM encrypted credential vault (`~/.servicenow/credentials.enc`).

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
- browser-session / SSO entry-point metadata,
- default profile name,
- default output format.

Secrets such as passwords, API tokens, OAuth client secrets, and stored OAuth tokens are kept outside the TOML file in an AES-256-GCM encrypted vault at `~/.servicenow/credentials.enc`.

## Credential storage modes

By default, `snow-cli` encrypts `~/.servicenow/credentials.enc` under a single 32-byte master key stored in the OS keychain (`service = "snow-cli"`, `account = "credentials-key"`). Because all profiles share one master key, adding profiles and refreshing OAuth tokens never trigger new OS Keychain prompts. Existing per-profile keychain entries from earlier releases continue to work via transparent fallback.

Available storage modes (`credential_store` in `config.toml` or `SNOW_CLI_CREDENTIAL_STORE`):

- `keyring` (default): Encrypted vault with one master key in the OS keychain.
- `file` (opt-in, zero-popup mode): Encrypted vault with an owner-only key file (`~/.servicenow/credentials.key`, mode `0600` on Unix). Never touches the OS keychain — eliminates macOS Keychain prompts across upgrades and unsigned/local builds. Security relies on OS user-account permissions and full-disk encryption (the same boundary as `~/.aws/credentials` or `~/.ssh/*`).
- `env` (CI / headless): Encrypted vault keyed by a base64-encoded 32-byte key in `SNOW_CLI_ENCRYPTION_KEY`. Never touches the OS keychain.
- `legacy-keychain`: Legacy per-profile OS keychain entries (`<profile>:<credential_type>`).

Manage storage modes and migrate legacy keychain entries with:

```bash
snow-cli profile storage status
snow-cli profile storage set file
snow-cli profile storage migrate --delete-legacy
```

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

For detailed profile management, see [`profile`](/commands/profile/).

## Authentication commands

Once a profile exists, store or refresh credentials with:

```bash
snow-cli auth login
snow-cli auth status
snow-cli auth token
snow-cli auth logout
```

For detailed auth behavior and secret input options, see [`auth`](/commands/auth/).

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

Use `authorization-code` when you need the CLI to act in user scope. That flow uses a browser login, a localhost callback, and PKCE. With `auth login --sdk-oauth`, it can instead reuse the ServiceNow SDK OAuth app and its paste-the-code callback.

See the dedicated guide:

- [OAuth authorization code with PKCE](/oauth-authorization-code-pkce/)

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
snow-cli --output json|csv|jsonl|toon|text|auto <command>
snow-cli --timeout-secs 30 <command>
snow-cli --read-only <command>
snow-cli -v <command>
snow-cli -vv <command>
```

When `--output` is omitted, output format resolution is: command-line flag, `SNOW_CLI_OUTPUT`, `profile output`, then JSON. Set a persistent default with
`snow-cli profile output <format>`; use `auto` to select the most compact
lossless format for each payload. See the [`profile` command reference](/commands/profile/)
for the supported formats and reset behavior.

For runtime classifications, exit statuses, and conservative retry rules, see
[Errors and retry guidance](/errors/).

## Discover more

- [`profile` command reference](/commands/profile/)
- [`auth` command reference](/commands/auth/)
- [`OAuth authorization code with PKCE`](/oauth-authorization-code-pkce/)
- [`Command reference`](/commands/)
