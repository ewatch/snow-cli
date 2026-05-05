# Configuration and authentication

## Profiles

Profiles describe how `snow-cli` connects to a ServiceNow instance.

Create a profile:

```bash
snow-cli profile add dev \
  --instance https://dev.service-now.com \
  --auth-method basic \
  --username admin
```

List profiles:

```bash
snow-cli profile list
```

Set the default profile:

```bash
snow-cli profile default dev
```

Show the active profile:

```bash
snow-cli profile current
```

Use a specific profile for one command:

```bash
snow-cli --profile dev table list incident --limit 10
```

## Authentication

Log in:

```bash
snow-cli auth login
```

Check status:

```bash
snow-cli auth status
```

Show the current token or credential status where supported:

```bash
snow-cli auth token
```

Log out:

```bash
snow-cli auth logout
```

## Supported authentication methods

The CLI supports multiple authentication patterns, including:

- Basic authentication,
- OAuth2 flows,
- API keys,
- SAML/session-cookie based workflows,
- mTLS profile configuration.

Exact flags depend on the selected auth method. Use command help for the current options:

```bash
snow-cli profile add --help
snow-cli auth login --help
```

## Global options

Common global flags:

```bash
snow-cli --profile dev <command>
snow-cli --instance https://override.service-now.com <command>
snow-cli --output json|csv|jsonl|toon|text <command>
snow-cli --timeout-secs 30 <command>
snow-cli -v <command>
snow-cli -vv <command>
```
