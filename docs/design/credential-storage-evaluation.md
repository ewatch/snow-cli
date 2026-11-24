# Credential Storage Evaluation (Reducing Keychain Popups)

## Status

**Implemented (Option 1)** — see [ADR-0011](../adr/0011-encrypted-credential-vault.md).

## Problem Statement

Users on macOS report frequent OS Keychain authorization popups when using `snow-cli` and `snow-cli-ro`, especially across multiple profiles, OAuth2 token refreshes, and binary updates.

### Root Causes in the Current Implementation

Inspecting the current credential path (`src/config/credentials.rs`, `src/config/keychain.rs`, `src/auth/oauth2.rs`, `src/cli/commands/auth.rs`, `src/cli/commands/config/profiles.rs`, and `.github/workflows/release.yml`) reveals four compounding causes:

1. **Per-`(profile, credential_type)` keychain items**
   `src/config/credentials.rs` stores every secret as a separate OS keychain entry keyed by `service = "snow-cli"`, `account = "<profile>:<credential_type>"` across 8 types (`password`, `api_token`, `client_secret`, `oauth_token`, `cert_passphrase`, `saml_token`, `session_cookie`, `native_session`). Each macOS Keychain item carries its own Access Control List (ACL), so authorization prompts scale linearly with `(profiles × credential_types)`.
2. **Unsigned / ad-hoc-signed macOS binaries**
   Release archives (`.github/workflows/release.yml`) and local `cargo build` binaries are not signed with an Apple Developer ID certificate. macOS Keychain therefore binds item ACLs to the exact binary's CDHash. Every upgrade or local rebuild invalidates prior "Always Allow" grants for every stored item.
3. **Two separate binaries (`snow-cli` and `snow-cli-ro`)**
   Both binaries read and write the same `snow-cli` keychain items (`src/main.rs` and `src/bin/snow-cli-ro.rs`). Credentials created by `snow-cli auth login` trigger a separate macOS Keychain prompt when `snow-cli-ro` reads them (and vice versa).
4. **Repeated runtime reads and writes**
   - `auth status --verify` checks `credentials::has_credential` (which calls `get_password`) and then builds a client that reads the same credential again during verification.
   - OAuth2 token refresh (`src/auth/oauth2.rs`) reads `oauth_token`, reads optional `client_secret`, and writes the refreshed `oauth_token` back to Keychain.
   - `profile remove` iterates over all 8 `ALL_CREDENTIAL_TYPES` and invokes `delete_credential` on each.

## Evaluation Criteria

- **Popup reduction (macOS):** Prompts on initial setup, switching between `snow-cli` and `snow-cli-ro`, OAuth token refreshes, adding new profiles, and upgrading binaries.
- **Security posture:** At-rest confidentiality, integrity/tamper detection, blast radius on a shared user account, and keeping secrets out of plaintext `config.toml` and logs (ADR-0003 invariant).
- **Cross-platform reliability:** macOS, desktop/headless Linux (no D-Bus/Secret Service), and Windows (where Windows Credential Manager enforces a **2,560-byte** `CRED_MAX_CREDENTIAL_BLOB_SIZE` limit).
- **Agent & non-interactive ergonomics:** Must work cleanly for coding agents and scripts without interactive master-password prompts on every invocation.
- **Dependency & migration footprint:** Prefer existing audited crates (`ring`, `ciborium` are already in `Cargo.toml` for `now-sdk 4.14+` interoperability) and non-destructive migration from existing keychain items.

## Options Evaluated

### Option 1: Single Encrypted Vault (`~/.servicenow/credentials.enc`) + Pluggable Key Source (Recommended)

Mirror the architecture adopted by ServiceNow `now-sdk 4.14+` (already implemented for `now-sdk` interop in `src/config/now_sdk/credential_file.rs`):

- Store **all** profile credentials in a single CBOR-enveloped, AES-256-GCM-encrypted file at `~/.servicenow/credentials.enc` (mode `0600` inside a `0700` directory, atomic temp-file + `sync_all` + rename, fresh 96-bit random nonce per write).
- Wrap the vault with one 32-byte master key obtained from a configurable **key source** (`credential_store` / `SNOW_CLI_KEY_SOURCE`):
  - **`keyring` (recommended default):** One single OS keychain entry (`service = "snow-cli"`, `account = "credentials-key"`).
  - **`file` (opt-in, zero-popup mode):** Master key stored in an owner-only key file (`~/.servicenow/credentials.key`, mode `0600` on Unix; DPAPI-protected via `CryptProtectData` on Windows).
  - **`env` (CI / headless):** Base64 32-byte key in `SNOW_CLI_ENCRYPTION_KEY` when `SNOW_CLI_KEY_SOURCE=env`.

**Why this works so well:**

- **With `keyring` default:** Reduces keychain items from `O(profiles × types)` to **1 single item total**. Adding profiles, rotating OAuth tokens, updating native sessions, or deleting profiles **never** creates or updates a keychain item—only the local `.enc` file changes. Even with unsigned binaries, a version upgrade requires at most 1 "Always Allow" per binary (2 total, regardless of whether you have 1 or 50 profiles).
- **With `file` opt-in:** **Zero macOS Keychain popups ever** across both binaries, local dev rebuilds, upgrades, SSH sessions, and headless Linux. On Windows, DPAPI provides per-user OS-managed encryption with zero popups. On Unix, security relies on OS user-account isolation (`0700`/`0600`) plus full-disk encryption (FileVault / LUKS), the same trust boundary used by `~/.aws/credentials`, `~/.kube/config`, `~/.ssh/*`, and `gh --insecure-storage`.
- **Zero new dependencies:** `ring` (AES-256-GCM + `SystemRandom`) and `ciborium` (CBOR) are already in `Cargo.toml`.
- **No Windows size limit:** Only the 32-byte key ever enters Windows Credential Manager; OAuth JWTs and multi-profile session cookies live in `credentials.enc`.

**Trade-offs:**

- Under `file` mode on Unix, storing the key next to `credentials.enc` protects against accidental plaintext exposure in `config.toml`, dotfile backups, grep, or screen sharing, **not** against a malicious process running under the same OS user UID. That trade-off must be explicit and opt-in (never a silent downgrade from `keyring`).
- Concurrent writes from parallel `snow-cli` / `snow-cli-ro` invocations (e.g., simultaneous OAuth token refreshes) require an advisory file lock (`credentials.enc.lock`) around read-modify-write cycles to avoid lost updates.

---

### Option 2: Single Consolidated Keychain Blob (`snow-cli` / `credentials`)

Store a single JSON map of all profiles and credentials in one OS keychain entry (the pre-4.14 `now-sdk` model).

- **Pros:** Reduces keychain items to 1; no encrypted file on disk; simple migration.
- **Cons:**
  - **Breaks on Windows:** Windows Credential Manager rejects blobs larger than **2,560 bytes**. Even 2–3 OAuth2 JWTs or native session cookies exceed this limit (the exact reason `now-sdk 4.14` abandoned this model for `credentials.enc`).
  - **Still writes to Keychain on every OAuth refresh / cookie rotation**, which can trigger macOS prompts or ACL re-checks on unsigned binaries.
- **Verdict:** **Reject.** Inferior to Option 1 on every cross-platform metric for similar implementation effort.

---

### Option 3: Apple Developer ID Code Signing + Notarization in Release CI

Sign and notarize `snow-cli` and `snow-cli-ro` macOS release binaries in `.github/workflows/release.yml` with a stable Developer ID certificate and Team ID designated requirement.

- **Pros:** macOS Keychain preserves "Always Allow" across release upgrades (`v0.11.0` → `v0.12.0`) for signed releases.
- **Cons:**
  - Requires an Apple Developer Program membership ($99/yr) and CI signing/notarization secrets.
  - Does **not** help users building from source (`cargo install`, local dev builds).
  - Does **not** fix the `O(profiles × types)` keychain item multiplication or the `snow-cli` vs `snow-cli-ro` two-binary boundary on its own.
- **Verdict:** **Complementary, not sufficient alone.** Worth doing alongside Option 1 for Homebrew / GitHub Release users.

---

### Option 4: Single Physical Binary (`snow-cli-ro` as symlink / `argv[0]` dispatch)

Ship one binary and invoke read-only mode when `argv[0]` ends with `snow-cli-ro`.

- **Pros:** Eliminates the cross-binary Keychain ACL mismatch between `snow-cli` and `snow-cli-ro`.
- **Cons:** Weakens the physical isolation guarantee documented in `docs/design/read-only-mode.md` (an agent harness that mounts or allow-lists only the `snow-cli-ro` binary currently gets a separate executable with no write subcommand parser). Still does not fix upgrades on unsigned binaries or per-profile item proliferation.
- **Verdict:** **Reject** as a storage workaround; keep `snow-cli-ro` as a distinct binary.

---

### Option 5: Passphrase-Encrypted Vault (Argon2id / `age`) + Session Agent

Encrypt `~/.servicenow/credentials.enc` with a user-chosen master passphrase and cache the derived key in a short-lived local daemon.

- **Pros:** Strong at-rest cryptographic isolation independent of OS keychains; zero OS keychain popups.
- **Cons:** Master-password prompts break unattended coding agents, CI jobs, and scripts; requires building and securing a long-lived key-caching daemon. Already rejected in ADR-0003 for UX friction.
- **Verdict:** **Reject as default.**

---

### Option 6: External Credential Provider Hook (`credential_process`)

Allow a profile or global config to invoke an external command (e.g., `op read ...` for 1Password, `pass`, Vault, AWS Secrets Manager) that returns a credential JSON payload on stdout (analogous to AWS CLI's `credential_process`).

- **Pros:** Zero OS keychain popups; integrates cleanly with enterprise secret managers and Touch ID-backed vaults (1Password CLI, etc.).
- **Cons:** Requires third-party software and user configuration; cannot be the out-of-the-box default.
- **Verdict:** **Good Phase 2 extension**, orthogonal to Option 1.

## Comparison Summary

| Option | macOS Popups (10 profiles, 2 bins, upgrade) | Windows >2.5 KB safe? | Headless Linux / CI | Zero new deps | Agent-friendly |
|---|---|---|---|---|---|
| Current (per-profile keychain) | Up to 20–40+ per upgrade | Yes (per item) | Env vars only | Yes | Poor on macOS |
| **1a. Vault + `keyring` key (default)** | **At most 1 per binary per upgrade** (0 on refresh/add) | **Yes** | Via `env` key | **Yes (`ring`+`ciborium`)** | **Good** |
| **1b. Vault + `file` key (opt-in)** | **0 ever** | **Yes (DPAPI)** | **Yes** | **Yes** | **Excellent** |
| 2. Single keychain blob | At most 1 per binary per upgrade | **No (2,560 B limit)** | Env vars only | Yes | Medium |
| 3. Apple Developer ID signing | 0 on upgrade (releases only), still per-item without #1 | N/A | N/A | N/A | Complementary |
| 4. Symlink single binary | Halves prompts, still per-item + per-upgrade | N/A | N/A | Yes | Weakens RO isolation |
| 5. Passphrase vault + agent | 0 OS popups (master passphrase instead) | Yes | Needs passphrase | No | Poor |
| 6. External `credential_process` | 0 OS popups (delegated to vault) | Yes | Depends on tool | Yes | Phase 2 |

## Independent Quick Wins (Applicable Immediately)

Even before or alongside vault storage, two small fixes reduce unnecessary keychain touches:

1. **Avoid redundant reads in `auth status --verify`:** `handle_status` currently calls `credentials::has_credential` (which fetches the secret from Keychain) and then `verify_credentials` fetches the same secret again. Checking presence once—or inferring presence from the vault index without decrypting—cuts reads in half.
2. **Avoid unnecessary `client_secret` lookups for public PKCE profiles:** `OAuth2Auth::refresh_token` queries `client_secret` on every refresh even when the profile uses a public PKCE client (no secret stored). Recording whether a client secret is expected or checking the vault map in memory avoids extra keychain lookups.

## Recommended Implementation Plan (Pending Decision)

1. **Supersede/amend ADR-0003** with a new ADR documenting the encrypted vault (`~/.servicenow/credentials.enc`) and key sources (`keyring` default, `file` opt-in, `env` for CI).
2. **Extract shared AEAD/CBOR + atomic write helpers** from `src/config/now_sdk/credential_file.rs` into a shared internal module (`src/config/vault.rs`) with an advisory lock file for concurrent read-modify-write safety.
3. **Add `credential_store` / key-source selection** in `AppConfig` (`~/.servicenow/config.toml`) and env overrides (`SNOW_CLI_KEY_SOURCE=keyring|file|env|legacy-keychain`, `SNOW_CLI_ENCRYPTION_KEY`), plus a CLI command (e.g., `snow-cli profile storage status|migrate`) so users can inspect or switch modes explicitly.
4. **Transparent non-destructive migration:**
   - On read: check encrypted vault first; if absent, fall back to legacy per-profile `<profile>:<credential_type>` keychain entry, then env vars.
   - On write (`auth login`, token refresh, `session login`): write to the vault; optionally offer `migrate` to copy existing legacy entries into the vault and clean up old per-profile keychain items.
5. **(Optional follow-up)** Add Apple Developer ID signing/notarization to `.github/workflows/release.yml` so macOS release upgrades keep Keychain authorization for the single `credentials-key` item without re-prompting.
