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
    async fn authenticate(&self) -> Result<HeaderMap, AuthError>;

    /// Refresh credentials if supported (e.g., OAuth token refresh).
    /// Returns Ok(true) if refresh succeeded, Ok(false) if not applicable.
    async fn refresh(&mut self) -> Result<bool, AuthError>;

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

#### Token Response (both grants)
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
- `oauth_grant_type`: `"client_credentials"` or `"password"` (defaults to
  `"client_credentials"` if not specified, for backwards compatibility).
- `client_id`: Required for both grants.
- `username`: Required for password grant, stored in config.

#### Keychain Entries
- `client_secret`: Required for both grants.
- `password`: Required only for password grant.

### API Key / Token
- Reads token from OS keychain.
- Constructs `Authorization: Bearer <token>` header.
- No refresh capability (user must manually rotate tokens).

### Mutual TLS (mTLS)
- Reads certificate and key file paths from config.
- Configures reqwest client with client certificate.
- No additional headers needed — TLS handshake handles auth.

### SSO / SAML
- Spawns a local HTTP server on a random port.
- Opens browser to ServiceNow SAML login endpoint with redirect to local server.
- Captures the session token from the callback.
- Stores token in keychain for subsequent requests.

## Factory

```rust
pub fn create_authenticator(profile: &Profile) -> Result<Box<dyn Authenticator>, AuthError> {
    match profile.auth_method {
        AuthMethod::Basic => Ok(Box::new(BasicAuth::new(profile)?)),
        AuthMethod::OAuth2 => Ok(Box::new(OAuth2Auth::new(profile)?)),
        AuthMethod::ApiKey => Ok(Box::new(ApiKeyAuth::new(profile)?)),
        AuthMethod::Mtls => Ok(Box::new(MtlsAuth::new(profile)?)),
        AuthMethod::Saml => Ok(Box::new(SamlAuth::new(profile)?)),
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
| `AUTH_SAML_TIMEOUT`        | Browser SAML flow timed out          |
