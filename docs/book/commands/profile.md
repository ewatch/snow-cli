# `profile`

Use `profile` to manage saved ServiceNow connection profiles.

```bash
snow-cli profile <verb> [options]
```

`config` remains available as a legacy alias for `profile`.

All `profile` subcommands also accept the global flags described in the [command overview](../commands.md), although most profile-management tasks mainly use the command-specific flags below.

## What a profile stores

A profile can contain:

- the instance URL,
- the auth method,
- the username for basic auth or OAuth password grant,
- OAuth client settings,
- mTLS certificate paths,
- browser-session / SSO entry-point metadata.

The same config file also stores global settings such as the default profile and the default stdout format. Secrets are stored separately in the OS keychain.

## `profile add <name>`

Create a new named profile.

```bash
snow-cli profile add <name> [options]
```

Important options:

- `--instance <url>`: instance URL such as `https://dev.service-now.com`
- `--auth-method <basic|oauth2|api-key|mtls|browser-session>` (`saml` is accepted as a legacy alias)
- `--username <user>`: used for basic auth or OAuth password grant
- `--client-id <id>`: required for OAuth2 profiles
- `--oauth-grant-type <client-credentials|password|authorization-code>`
- `--oauth-scope <scope>`: optional for authorization-code profiles, default `useraccount`
- `--oauth-redirect-host <host>`
- `--oauth-redirect-port <port>`
- `--oauth-redirect-path <path>`
- `--cert-path <path>` and `--key-path <path>`: metadata for future mTLS support
- `--sso-login-url <url>`: browser entry point for SSO/SAML login

Examples:

```bash
snow-cli profile add dev \
  --instance https://dev.service-now.com \
  --auth-method basic \
  --username admin

snow-cli profile add user-scope \
  --instance https://dev.service-now.com \
  --auth-method oauth2 \
  --client-id abc123 \
  --oauth-grant-type authorization-code \
  --oauth-scope useraccount

snow-cli profile add browser-dev \
  --instance https://dev.service-now.com \
  --auth-method browser-session \
  --sso-login-url https://dev.service-now.com/login_with_sso.do
```

Notes:

- When you create the first profile, `snow-cli` makes it the default automatically.
- If the profile already exists, use `profile edit` instead.
- For authorization-code OAuth2, see the dedicated [PKCE guide](../oauth-authorization-code-pkce.md).
- `mtls` profile metadata is accepted, but the mTLS authenticator is not implemented yet.

## `profile edit <name>`

Update an existing profile.

```bash
snow-cli profile edit <name> [options]
```

You can change only the fields you want. Omitted fields keep their existing values.

Examples:

```bash
snow-cli profile edit dev --username new-admin
snow-cli profile edit user-scope --oauth-scope "useraccount email"
snow-cli profile edit saml-dev --sso-login-url https://dev.service-now.com/login_with_sso.do
```

## `profile list`

List all configured profiles.

```bash
snow-cli profile list
```

This is useful for checking:

- profile names,
- instance URLs,
- auth methods,
- which profile is the default.

Example:

```bash
snow-cli --output csv profile list
```

## `profile find --instance <value>`

Find profiles by instance name, host, or full URL.

```bash
snow-cli profile find --instance dev123456
snow-cli profile find --instance dev123456.service-now.com
snow-cli profile find --instance https://dev123456.service-now.com
```

Use this when you know the instance but not the saved profile name.

## `profile sdk`

Interoperate with ServiceNow `now-sdk` aliases.

```bash
snow-cli profile sdk <verb> [options]
```

Subcommands:

- `profile sdk list`: list saved `now-sdk` aliases
- `profile sdk import`: copy one or more `now-sdk` aliases into `snow-cli`
- `profile sdk export`: export a `snow-cli` profile into the `now-sdk` alias store

Examples:

```bash
snow-cli profile sdk list
snow-cli profile sdk import --alias dev
snow-cli profile sdk import --all
snow-cli profile sdk export prod --alias prod-sdk
```

Important options:

- `profile sdk import --alias <name>`: import one alias
- `profile sdk import --all`: import every alias
- `profile sdk import --set-default`: set the imported profile as the `snow-cli` default when importing a single alias
- `profile sdk export <profile> --alias <name>`: change the destination alias name
- `profile sdk export <profile> --set-default`: make the exported alias the `now-sdk` default

Note: current interoperability is aimed at basic-auth profiles.

## `profile default <name>`

Set the profile used when `--profile` is not provided.

```bash
snow-cli profile default dev
```

This updates the `default_profile` entry in the config file.

## `profile current`

Show a small summary of the active profile.

```bash
snow-cli profile current
```

This is the quickest way to confirm which profile the CLI will use right now.

## `profile show`

Show the active profile configuration in more detail.

```bash
snow-cli profile show
```

Compared with `profile current`, this also includes the config path and profile details.

## `profile output [format]`

Show or set the global default output format used when `--output` is not passed.

```bash
snow-cli profile output
snow-cli profile output toon
snow-cli profile output --reset
```

Supported persisted formats are `json`, `csv`, `jsonl`, `toon`, `text`, and `auto`.
`--reset` clears the configured default and returns to the built-in JSON fallback.

## `profile remove <name>`

Delete a saved profile.

```bash
snow-cli profile remove <name>
```

Important options:

- `--yes`: required when deleting the current default profile
- `--new-default <name>`: required replacement default when deleting the current default profile

Example:

```bash
snow-cli profile remove old-dev
snow-cli profile remove dev --yes --new-default prod
```

When a profile is removed, `snow-cli` also deletes stored credentials for that profile on a best-effort basis.

## Related pages

- [Configuration and authentication](../configuration.md)
- [`auth` command reference](./auth.md)
- [OAuth authorization code with PKCE](../oauth-authorization-code-pkce.md)
