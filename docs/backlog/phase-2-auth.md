# Phase 2 — Authentication

Implement all authentication mechanisms behind a common trait.

## Already Done (Phase 1)

- [x] Define `Authenticator` trait (`authenticate()`, `refresh()`, `auth_type()`)
- [x] Implement Basic Auth (username from config, password from keychain, `Authorization: Basic` header)
- [x] Implement auth commands (`auth login`, `auth logout`, `auth status`, `auth token`)
- [x] Implement authenticator factory (`create_authenticator(profile)`)
- [x] Integration with HTTP client (SnowClient uses authenticator, auto-retry on 401)

## Work Items — OAuth 2.0

- [x] Add `oauth_grant_type` field to `Profile` config model
  - [x] New enum: `OAuthGrantType` with `ClientCredentials` and `Password` variants
  - [x] Defaults to `client_credentials` if not specified (backward compat)
  - [x] Update `config set-profile` to accept `--oauth-grant-type` flag
- [x] Implement OAuth 2.0 Client Credentials flow
  - [x] Token request to `<instance>/oauth_token.do` with `grant_type=client_credentials`
  - [x] Reads `client_id` from config, `client_secret` from keychain
  - [x] In-memory token caching with expiry tracking
  - [x] Auto-refresh using `refresh_token` when available
  - [x] Fallback to full re-authentication if refresh fails
  - [x] Tests with wiremock (token exchange, refresh, expiry, error cases)
- [x] Implement OAuth 2.0 Resource Owner Password Credentials flow
  - [x] Token request to `<instance>/oauth_token.do` with `grant_type=password`
  - [x] Reads `client_id` + `username` from config; `client_secret` + `password` from keychain
  - [x] Same token caching and refresh logic as client credentials
  - [x] Tests with wiremock (token exchange, refresh, missing credentials)
- [x] Update `auth login` for OAuth2
  - [x] Accept `--client-secret` (both flows)
  - [x] Accept `--password` (password flow only)
  - [x] Store each credential as separate keychain entry
- [x] Update `auth status` to show grant type info

## Work Items — API Key / Token

- [x] Implement API Key authenticator
  - [x] Read token from keychain
  - [x] Construct `Authorization: Bearer <token>` header
  - [x] No refresh capability
  - [x] Unit tests (header construction, refresh returns false, auth type)

## Deferred to Phase 5

- [ ] Mutual TLS (mTLS) — requires reqwest client cert configuration
- [ ] SSO / SAML — requires browser interaction and local callback server
