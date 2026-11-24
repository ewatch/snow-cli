# ADR-0011: Encrypted Local Credential Vault with Pluggable Key Source

## Status

Accepted (supersedes [ADR-0003](0003-os-keychain-credentials.md))

## Context

Under ADR-0003, `snow-cli` stored every `(profile, credential_type)` secret as a separate entry in the OS keychain (`service = "snow-cli"`, `account = "<profile>:<credential_type>"` across 8 credential types).

In practice, this caused severe UX friction on macOS and platform limits on Windows and Linux:

1. **Keychain popup multiplication on macOS:** Unsigned/ad-hoc-signed release and local builds bind macOS Keychain ACLs to the creating binary's CDHash. Because `snow-cli` and `snow-cli-ro` are two distinct binaries and every profile/credential type had its own keychain item, users faced `O(profiles × credential_types × 2)` authorization dialogs on upgrades, rebuilds, and cross-binary usage.
2. **Windows Credential Manager size limits:** Consolidating secrets directly into one OS keychain blob fails on Windows due to its 2,560-byte `CRED_MAX_CREDENTIAL_BLOB_SIZE` limit (multiple OAuth2 JWTs or native session cookies exceed it).
3. **Headless Linux / agent ergonomics:** Environments without a running Secret Service daemon had no persistent encrypted storage alternative outside per-invocation environment variables.

Meanwhile, `snow-cli` already carries audited AES-256-GCM (`ring`) and CBOR (`ciborium`) dependencies for ServiceNow `now-sdk 4.14+` encrypted credential file interoperability (`~/.now-sdk/credentials.enc`).

See [docs/design/credential-storage-evaluation.md](../design/credential-storage-evaluation.md) for the full options analysis.

## Decision

Consolidate all `snow-cli` profile credentials into a single AES-256-GCM + CBOR encrypted vault at `~/.servicenow/credentials.enc` (mode `0600`, atomic temp-file + `sync_all` + rename, advisory file lock around read-modify-write mutations, fresh 96-bit random IV per write) wrapped by a single 32-byte master key from a configurable storage mode (`credential_store` in `config.toml` or `SNOW_CLI_CREDENTIAL_STORE`):

1. **`keyring` (default):** Stores one 32-byte base64 master key in the OS keychain (`service = "snow-cli"`, `account = "credentials-key"` for `~/.servicenow`, or `credentials-key:<sha256-prefix>` for custom `SNOW_CLI_CONFIG` directories so secondary configs never clobber the primary vault key). Reads check `credentials.enc` first, transparently fall back to legacy per-profile keychain entries (`<profile>:<credential_type>`), and finally fall back to `SNOW_CLI_*` environment variables. Writes only update `credentials.enc` using the cached master key (never creating or updating per-profile keychain items); logout and profile removal also clean up any legacy per-profile entries so deleted secrets cannot resurface.
2. **`file` (opt-in, zero-popup mode):** Stores the 32-byte base64 master key in an owner-only file (`~/.servicenow/credentials.key`, mode `0600` on Unix). Never touches the OS keychain on read, write, or delete.
3. **`env` (CI / headless):** Reads the base64 32-byte master key from `SNOW_CLI_ENCRYPTION_KEY`. Never touches the OS keychain.
4. **`legacy-keychain` (opt-out):** Retains the legacy per-profile OS keychain entry model for environments that explicitly require it.

### Safety & Migration Guarantees

- **Fail-closed on unreadable vault:** If `credentials.enc` exists on disk but its master key is missing/locked/malformed or decryption fails, `snow-cli` returns `CREDENTIALS_UNREADABLE` (exit 2) — including from `auth status` — and never overwrites or regenerates the vault.
- **Symlink refusal:** Refuses to read or write `credentials.enc`, `credentials.key`, or `.credentials.enc.lock` through symlinks.
- **In-process key caching:** The unlocked master key is cached in memory per `(mode, config_dir)` scope (storing only a digest of `SNOW_CLI_ENCRYPTION_KEY`, never raw secret text in `Debug`) for the duration of a CLI invocation so multi-step commands (`auth status --verify`, OAuth refresh) access the OS keychain at most once.
- **CLI management (`snow-cli profile storage`):**
  - `profile storage status` inspects active mode and file state without prompting the OS keychain (allowed in `snow-cli-ro`).
  - `profile storage set <mode> [--keep-old-key]` updates `credential_store` and migrates/re-wraps the master key automatically (or exports vault entries back to legacy keychain when switching to `legacy-keychain`). Refuses when `SNOW_CLI_CREDENTIAL_STORE` is overriding `config.toml`, keeps the previous persistent key when switching to `env`, and is blocked in read-only policy.
  - `profile storage migrate [--delete-legacy] [--overwrite]` copies existing legacy per-profile keychain entries into `credentials.enc` and optionally deletes the migrated old items (never deleting skipped entries; blocked in read-only policy).

## Alternatives Considered

- **Single consolidated OS keychain blob:** Breaks Windows Credential Manager's 2,560-byte limit.
- **Apple Developer ID code signing alone:** Complementary for signed macOS releases, but does not help local/`cargo install` builds or eliminate per-profile item proliferation on its own.
- **Unifying `snow-cli` and `snow-cli-ro` into a symlinked single binary:** Weakens the physical read-only binary boundary.
- **Passphrase-encrypted vault + agent daemon:** Breaks non-interactive coding agent and CI workflows.

## Consequences

- **Easier:** macOS Keychain prompts drop from `O(profiles × types)` to at most 1 per binary per upgrade in default `keyring` mode, and **0 ever** in opt-in `file` mode; OAuth token refreshes and new profiles never trigger Keychain popups; headless Linux and Windows multi-profile OAuth tokens work reliably; zero new crate dependencies added.
- **Harder / Trade-offs:** Unlocking the single vault key grants access to all profiles stored in `credentials.enc` (rather than per-profile Keychain ACLs). In explicit opt-in `file` mode on Unix, `credentials.key` sits beside `credentials.enc` with `0600` permissions, relying on OS user-account isolation + full-disk encryption (same trust boundary as `~/.aws/credentials`, `~/.kube/config`, and `~/.ssh/*`) rather than per-binary macOS Keychain ACLs.
