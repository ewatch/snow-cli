# Phase 2 — Authentication

Implement all authentication mechanisms behind a common trait.

## Work Items

- [ ] Define `Authenticator` trait
  - [ ] `authenticate()` — returns auth headers
  - [ ] `refresh()` — refresh credentials if applicable
  - [ ] `auth_type()` — returns the method enum variant
- [ ] Implement Basic Auth
  - [ ] Read username from config, password from keychain
  - [ ] Construct `Authorization: Basic` header
  - [ ] Tests with wiremock
- [ ] Implement OAuth 2.0
  - [ ] Client credentials flow
  - [ ] Token caching in memory
  - [ ] Automatic token refresh on 401
  - [ ] Token endpoint discovery from instance
  - [ ] Tests with wiremock (token exchange, refresh, expiry)
- [ ] Implement API Key / Token
  - [ ] Read token from keychain
  - [ ] Construct `Authorization: Bearer` header
  - [ ] Tests with wiremock
- [ ] Implement auth commands
  - [ ] `auth login` — authenticate and store credentials
  - [ ] `auth logout` — clear stored credentials from keychain
  - [ ] `auth status` — show current auth state
  - [ ] `auth token` — print access token to stdout
- [ ] Implement authenticator factory
  - [ ] `create_authenticator(profile)` dispatches to correct implementation
  - [ ] Error handling for missing/invalid config
- [ ] Integration with HTTP client
  - [ ] SnowClient uses authenticator for all requests
  - [ ] Auto-retry on 401 with token refresh

## Deferred to Phase 5

- [ ] Mutual TLS (mTLS) — requires reqwest client cert configuration
- [ ] SSO / SAML — requires browser interaction and local callback server
