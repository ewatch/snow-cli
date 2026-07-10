# Authentication Architecture

## Overview

All authentication methods implement a common `Authenticator` trait. This allows
the HTTP client to be agnostic about which auth method is in use — it simply calls
`authenticate()` to get headers to attach to each request.

## Trait Definition

```rust
#[async_trait]
pub trait Authenticator: Send + Sync {
    /// Returns HTTP headers to attach to a request for authentication.
    async fn authenticate(&self) -> anyhow::Result<HeaderMap>;

    /// Refresh credentials if supported (e.g., OAuth token refresh).
    /// Returns Ok(true) if refresh succeeded, Ok(false) if not applicable.
    async fn refresh(&mut self) -> anyhow::Result<bool>;

    /// Returns the authentication method type.
    fn auth_type(&self) -> AuthMethod;
}
```

## Auth Methods

### Basic Auth
- Reads username from config, password from OS keychain.
- Constructs `Authorization: Basic <base64>` header.
- No refresh capability.

### OAuth 2.0
- Token endpoint: `https://<instance>/oauth_token.do`
- Automatically caches access tokens in memory with expiry tracking.
- Automatically refreshes expired tokens using `refresh_token` if available,
  otherwise re-authenticates with the original grant.
- Stores `client_id` and `oauth_grant_type` in config.

#### Client Credentials Grant (`grant_type=client_credentials`)
- For machine-to-machine / service account access.
- Stores `client_id` in config, `client_secret` in keychain.
- Token request:
  ```
  POST /oauth_token.do
  Content-Type: application/x-www-form-urlencoded

  grant_type=client_credentials&client_id=<client_id>&client_secret=<client_secret>
  ```

#### Resource Owner Password Credentials Grant (`grant_type=password`)
- For user-context access where a human user's identity is required.
- Stores `client_id` and `username` in config; `client_secret` and `password`
  in keychain (two separate keychain entries).
- Token request:
  ```
  POST /oauth_token.do
  Content-Type: application/x-www-form-urlencoded

  grant_type=password&client_id=<client_id>&client_secret=<client_secret>&username=<username>&password=<password>
  ```
- Note: ServiceNow requires both `client_id` + `client_secret` even for the
  password grant (unlike some OAuth2 implementations that make client auth optional).

#### Authorization Code Grant (`grant_type=authorization_code`)
- For browser-based user login with a local redirect listener and PKCE.
- Stores `client_id`, redirect settings, and requested scope in config.
- Stores the resulting OAuth token set in keychain as `oauth_token`.
- `client_secret` is optional:
  - Public PKCE clients do not need one.
  - Confidential clients can still provide one and `snow-cli` will send it on
    token exchange and refresh requests.
- Token request for public PKCE clients:
  ```
  POST /oauth_token.do
  Content-Type: application/x-www-form-urlencoded

  grant_type=authorization_code&client_id=<client_id>&code=<code>&redirect_uri=<redirect_uri>&code_verifier=<code_verifier>
  ```
- Refresh request for public PKCE clients:
  ```
  grant_type=refresh_token&client_id=<client_id>&refresh_token=<refresh_token>
  ```

#### Token Response (all grants)
```json
{
  "access_token": "...",
  "refresh_token": "...",
  "scope": "useraccount",
  "token_type": "Bearer",
  "expires_in": 1800
}
```

#### Config Fields
- `oauth_grant_type`: `"client_credentials"`, `"password"`, or
  `"authorization_code"` (defaults to `"client_credentials"` if not specified,
  for backwards compatibility).
- `client_id`: Required for all grants.
- `username`: Required for password grant, stored in config.
- `oauth_scope`, `oauth_redirect_host`, `oauth_redirect_port`,
  `oauth_redirect_path`: Used for authorization-code profiles.

#### Keychain Entries
- `client_secret`: Required for client credentials and password grants;
  optional for authorization-code profiles.
- `password`: Required only for password grant.
- `oauth_token`: Required for authorization-code profiles.

### API Key / Token
- Reads token from OS keychain.
- Constructs `Authorization: Bearer <token>` header.
- No refresh capability (user must manually rotate tokens).

### Browser Session
- Reads a full `Cookie` header value from `SNOW_SESSION_COOKIE` or a one-shot login flag.
- Constructs a `Cookie: ...` header for requests.
- Does not store the session cookie in config or keychain.
- Intended as the practical SSO/SAML workaround when users already have an authenticated browser session.

## now-sdk Profile Interoperability

`snow-cli` can explicitly copy or sync basic authentication profiles with the
official ServiceNow `now-sdk` CLI.

### Scope
- Copy/sync only. `snow-cli` does not resolve `now-sdk` aliases during normal
  command execution.
- v1 supports `basic` aliases only.
- `oauth` aliases are listed as unsupported and cannot be imported or exported.

### now-sdk Storage Model
- `now-sdk` stores aliases in the OS keychain under:
  - service: `ServiceNow`
  - account: `now-sdk`
- The keychain value is a single JSON object keyed by alias.
- Each alias entry includes:
  - `alias`
  - `isDefault`
  - `creds`

For basic auth, the `creds` payload contains:

```json
{
  "type": "basic",
  "instanceUrl": "https://dev.service-now.com",
  "username": "admin",
  "password": "secret"
}
```

### `snow-cli` Commands
- `snow-cli profile sdk list`
- `snow-cli profile sdk import --alias <name>`
- `snow-cli profile sdk import --all`
- `snow-cli profile sdk export <profile> [--alias <name>]`
- `snow-cli auth login --also-now-sdk [--now-sdk-alias <name>]`

Legacy `snow-cli config ...` and `snow-cli profile <old-long-name>` forms remain accepted as hidden aliases for compatibility.

### Import Behavior
- Import creates or overwrites a `snow-cli` profile with:
  - `instance`
  - `auth_method = "basic"`
  - `username`
- The imported password is written to the `snow-cli` keychain entry for that
  profile.
- `--set-default` updates the `snow-cli` default profile only for single-alias
  imports.

### Export / Sync Behavior
- Export and `auth login --also-now-sdk` create or overwrite exactly one
  `now-sdk` alias.
- Unrelated `now-sdk` aliases are preserved.
- `--set-default` / `--set-now-sdk-default` marks the destination alias as the
  `now-sdk` default.

### Collision Rules
- Import collisions overwrite the target `snow-cli` profile metadata and stored
  password atomically.
- Export and login-sync collisions overwrite the target `now-sdk` alias payload
  atomically.

### Failure Handling
- Unsupported auth types fail before any destination writes occur.
- Import and dual-write login paths snapshot the destination state and restore it
  on failure so partial writes are not left behind.

### Mutual TLS (mTLS)
- Profile validation accepts certificate and key file paths in config.
- The authenticator factory currently fails fast with "mTLS authentication is not yet implemented."
- A future implementation should configure reqwest with a client certificate.

### SSO / SAML
- A full SSO/SAML callback-capture authenticator is not implemented.
- Use `browser-session` profiles with `SNOW_SESSION_COOKIE` as the current workaround.

## Factory

```rust
pub fn create_authenticator(
    profile_name: &str,
    profile: &Profile,
) -> anyhow::Result<Box<dyn Authenticator>> {
    validate_profile_config(profile_name, profile)?;

    match profile.auth_method {
        AuthMethod::Basic => Ok(Box::new(BasicAuth::new(profile_name, profile)?)),
        AuthMethod::Oauth2 => Ok(Box::new(OAuth2Auth::new(profile_name, profile)?)),
        AuthMethod::ApiKey => Ok(Box::new(ApiKeyAuth::new(profile_name, profile)?)),
        AuthMethod::Mtls => anyhow::bail!("mTLS authentication is not yet implemented"),
        AuthMethod::BrowserSession => Ok(Box::new(BrowserSessionAuth::new(profile_name, profile)?)),
    }
}
```

## Error Handling

Auth errors use the standard JSON error format with specific codes:

| Code                    | Meaning                                |
|-------------------------|----------------------------------------|
| `AUTH_MISSING_CREDENTIALS` | Required credentials not found       |
| `AUTH_INVALID_CREDENTIALS` | Server rejected credentials          |
| `AUTH_TOKEN_EXPIRED`       | Token expired and refresh failed     |
| `AUTH_KEYCHAIN_ERROR`      | Failed to access OS keychain         |
| `AUTH_CERTIFICATE_ERROR`   | Certificate file not found or invalid |
| `AUTH_SAML_TIMEOUT`        | Browser SAML flow timed out (reserved for future full SSO/SAML support) |
