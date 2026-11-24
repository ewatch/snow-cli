# ADR-0003: OS Keychain for Credential Storage

## Status

Superseded by [ADR-0011](0011-encrypted-credential-vault.md) (OS keychain now stores a single vault master key by default instead of one entry per profile/credential type, with opt-in `file`, `env`, and `legacy-keychain` modes)

## Context

The CLI handles sensitive credentials (passwords, OAuth client secrets, API tokens)
that must be stored securely on the local machine. Plaintext storage in config files
is unacceptable for production use.

## Decision

Use OS-native credential storage via the `keyring` crate:
- **macOS:** Keychain
- **Linux:** Secret Service (GNOME Keyring / KDE Wallet)
- **Windows:** Windows Credential Manager

The TOML config file stores non-sensitive settings (instance URLs, usernames,
auth method, certificate paths). All secrets are stored in the OS keychain,
keyed by `snow-cli:<profile>:<credential_type>`.

## Alternatives Considered

| Approach              | Pros                          | Cons                              |
|-----------------------|-------------------------------|-----------------------------------|
| OS keychain           | Native security, no extra deps | Requires desktop env on Linux     |
| Encrypted config file | Portable, no OS dependency    | Master password UX friction       |
| Plain config file     | Simple                        | Insecure, secrets in plaintext    |
| Env vars only         | Simple, CI/CD friendly        | No persistence, manual management |

## Consequences

- **Easier:** Security (OS-managed encryption), no master password needed,
  integrates with OS security policies
- **Harder:** Headless Linux environments may need `gnome-keyring-daemon` or
  fallback to env vars; testing requires mocking the keychain
