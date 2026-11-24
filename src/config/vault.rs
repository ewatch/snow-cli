//! Encrypted credential vault (`~/.servicenow/credentials.enc`) with pluggable key sources.
//!
//! Consolidates all `snow-cli` profile secrets into a single AES-256-GCM + CBOR
//! encrypted file so OS Keychain ACL prompts do not multiply across profiles,
//! credential types, or OAuth token refreshes.
//!
//! Supported storage modes (configured via `SNOW_CLI_CREDENTIAL_STORE` or
//! `credential_store` in `config.toml`, defaulting to `keyring`):
//! - `keyring` (default): vault encrypted under one 32-byte key stored in the OS
//!   keychain (`service = "snow-cli"`, `account = "credentials-key"` for the
//!   default `~/.servicenow` directory, or `credentials-key:<hash>` for custom
//!   `SNOW_CLI_CONFIG` directories). Reads fall back transparently to legacy
//!   per-profile keychain entries.
//! - `file`: vault encrypted under a 32-byte key in `<config_dir>/credentials.key`
//!   (mode `0600` on Unix). Never touches the OS keychain (zero popups).
//! - `env`: vault encrypted under the base64 32-byte key in
//!   `SNOW_CLI_ENCRYPTION_KEY`. Never touches the OS keychain.
//! - `legacy-keychain`: legacy per-profile OS keychain entries (`<profile>:<type>`).

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::credentials::{ALL_CREDENTIAL_TYPES, SERVICE_NAME};
use crate::config::encrypted_store::{self, EnvelopeError, FORMAT_VERSION, FileKey};
use crate::config::error::ConfigError;
use crate::config::keychain;
use crate::config::profile::AppConfig;
use crate::models::secret::Secret;

/// Error code returned when `credentials.enc` exists but cannot be decrypted or parsed.
pub const UNREADABLE_CODE: &str = "CREDENTIALS_UNREADABLE";

/// Base keychain account name for the vault master key (used directly for `~/.servicenow`).
pub const VAULT_KEY_ACCOUNT: &str = "credentials-key";

/// Encrypted vault filename inside the config directory.
pub const VAULT_FILE_NAME: &str = "credentials.enc";

/// Key filename inside the config directory for `file` mode.
pub const KEY_FILE_NAME: &str = "credentials.key";

/// Advisory lock filename used to serialize read-modify-write mutations.
const LOCK_FILE_NAME: &str = ".credentials.enc.lock";

/// Environment variable overriding `credential_store` in `config.toml`.
pub const STORE_MODE_ENV: &str = "SNOW_CLI_CREDENTIAL_STORE";

/// Environment variable providing the base64 32-byte key in `env` mode.
pub const ENCRYPTION_KEY_ENV: &str = "SNOW_CLI_ENCRYPTION_KEY";

/// Payload schema version inside the decrypted JSON.
const VAULT_PAYLOAD_VERSION: u32 = 1;

/// Active credential storage backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialStoreMode {
    /// Encrypted vault (`credentials.enc`) with one master key in the OS keychain.
    Keyring,
    /// Encrypted vault (`credentials.enc`) with a `0600` key file (`credentials.key`).
    File,
    /// Encrypted vault (`credentials.enc`) with the master key in `SNOW_CLI_ENCRYPTION_KEY`.
    Env,
    /// Legacy per-profile OS keychain entries (`<profile>:<credential_type>`).
    LegacyKeychain,
}

impl CredentialStoreMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Keyring => "keyring",
            Self::File => "file",
            Self::Env => "env",
            Self::LegacyKeychain => "legacy-keychain",
        }
    }

    pub fn parse(raw: &str) -> anyhow::Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "keyring" => Ok(Self::Keyring),
            "file" => Ok(Self::File),
            "env" => Ok(Self::Env),
            "legacy-keychain" | "legacy_keychain" => Ok(Self::LegacyKeychain),
            other => Err(ConfigError::new(
                "CONFIG_INVALID",
                format!(
                    "Unknown credential_store '{other}'. Expected one of: keyring, file, env, legacy-keychain."
                ),
            )
            .into()),
        }
    }

    /// Whether this mode uses `credentials.enc`.
    pub const fn uses_vault(self) -> bool {
        matches!(self, Self::Keyring | Self::File | Self::Env)
    }
}

/// Where the active `CredentialStoreMode` was resolved from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModeOrigin {
    Env,
    Config,
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedMode {
    pub mode: CredentialStoreMode,
    pub origin: ModeOrigin,
}

/// Resolve the active credential storage mode from env -> config.toml -> default (`keyring`).
pub fn resolve_mode_from_config_path(config_path: &Path) -> anyhow::Result<ResolvedMode> {
    if let Ok(raw) = std::env::var(STORE_MODE_ENV) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(ResolvedMode {
                mode: CredentialStoreMode::parse(trimmed)?,
                origin: ModeOrigin::Env,
            });
        }
    }

    if config_path.exists() {
        let config = AppConfig::load_from(config_path)?;
        if let Some(raw) = config.credential_store.as_deref() {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return Ok(ResolvedMode {
                    mode: CredentialStoreMode::parse(trimmed)?,
                    origin: ModeOrigin::Config,
                });
            }
        }
    }

    Ok(ResolvedMode {
        mode: CredentialStoreMode::Keyring,
        origin: ModeOrigin::Default,
    })
}

pub fn resolve_mode() -> anyhow::Result<ResolvedMode> {
    resolve_mode_from_config_path(&AppConfig::config_path())
}

pub fn vault_path_for(config_dir: &Path) -> PathBuf {
    config_dir.join(VAULT_FILE_NAME)
}

pub fn key_file_path_for(config_dir: &Path) -> PathBuf {
    config_dir.join(KEY_FILE_NAME)
}

fn lock_path_for(config_dir: &Path) -> PathBuf {
    config_dir.join(LOCK_FILE_NAME)
}

/// Normalize a directory path as far as existing ancestors allow, so symlinked
/// prefixes (such as `/var` -> `/private/var` on macOS) and relative paths
/// map consistently to one canonical directory identity.
fn normalize_dir_path(dir: &Path) -> PathBuf {
    let absolute = if dir.is_absolute() {
        dir.to_path_buf()
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(dir)
    } else {
        dir.to_path_buf()
    };

    let mut existing = absolute.as_path();
    let mut tail = Vec::new();
    loop {
        if let Ok(canonical) = std::fs::canonicalize(existing) {
            let mut result = canonical;
            for component in tail.into_iter().rev() {
                result.push(component);
            }
            return result;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_os_string());
                existing = parent;
            }
            _ => return absolute,
        }
    }
}

/// Keychain account name for the vault master key associated with `config_dir`.
///
/// The default `~/.servicenow` directory uses `"credentials-key"`. Any other
/// directory (e.g. a custom `SNOW_CLI_CONFIG`) gets a directory-scoped account
/// (`"credentials-key:<16-hex-sha256>"`) so switching or removing a key for a
/// secondary config directory can never affect the user's primary vault.
pub fn vault_key_account_for(config_dir: &Path) -> String {
    let normalized = normalize_dir_path(config_dir);
    if let Some(home) = crate::config::profile::home_dir() {
        let default_dir = normalize_dir_path(&home.join(".servicenow"));
        if normalized == default_dir {
            return VAULT_KEY_ACCOUNT.to_string();
        }
    }
    let digest = Sha256::digest(normalized.to_string_lossy().as_bytes());
    let mut suffix = String::with_capacity(16);
    for byte in &digest[..8] {
        use std::fmt::Write as _;
        let _ = write!(&mut suffix, "{byte:02x}");
    }
    format!("{VAULT_KEY_ACCOUNT}:{suffix}")
}

/// Decrypted payload persisted inside `credentials.enc`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VaultPayload {
    version: u32,
    #[serde(default)]
    entries: BTreeMap<String, Secret<String>>,
}

impl Default for VaultPayload {
    fn default() -> Self {
        Self {
            version: VAULT_PAYLOAD_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

/// In-process key cache scoped to the current mode, config directory, test store,
/// and hashed env key (never storing or printing the raw secret).
#[derive(Clone, PartialEq, Eq)]
struct KeyCacheScope {
    mode: CredentialStoreMode,
    config_dir: PathBuf,
    test_store: Option<String>,
    env_key_digest: Option<[u8; 32]>,
}

impl std::fmt::Debug for KeyCacheScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyCacheScope")
            .field("mode", &self.mode)
            .field("config_dir", &self.config_dir)
            .field("has_test_store", &self.test_store.is_some())
            .field("has_env_key", &self.env_key_digest.is_some())
            .finish()
    }
}

fn key_cache() -> &'static Mutex<Option<(KeyCacheScope, FileKey)>> {
    static CACHE: OnceLock<Mutex<Option<(KeyCacheScope, FileKey)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn current_cache_scope(mode: CredentialStoreMode, config_dir: &Path) -> KeyCacheScope {
    let env_key_digest = if mode == CredentialStoreMode::Env {
        std::env::var(ENCRYPTION_KEY_ENV).ok().map(|raw| {
            let digest = Sha256::digest(raw.trim().as_bytes());
            let mut out = [0u8; 32];
            out.copy_from_slice(&digest);
            out
        })
    } else {
        None
    };

    KeyCacheScope {
        mode,
        config_dir: normalize_dir_path(config_dir),
        test_store: std::env::var("SNOW_CLI_TEST_KEYCHAIN_STORE").ok(),
        env_key_digest,
    }
}

fn cached_key(scope: &KeyCacheScope) -> Option<FileKey> {
    key_cache().lock().ok().and_then(|guard| {
        guard
            .as_ref()
            .and_then(|(cached_scope, key)| (cached_scope == scope).then(|| key.clone()))
    })
}

fn store_cached_key(scope: KeyCacheScope, key: FileKey) {
    if let Ok(mut guard) = key_cache().lock() {
        *guard = Some((scope, key));
    }
}

/// Clear the in-process master-key cache (called on mode switches and in tests).
pub fn clear_key_cache() {
    if let Ok(mut guard) = key_cache().lock() {
        *guard = None;
    }
}

fn ensure_not_symlink(path: &Path, label: &str) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(ConfigError::new(
            UNREADABLE_CODE,
            format!(
                "Refusing to access {label} through symlink {}.",
                path.display()
            ),
        )
        .into()),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Load the master key for `mode` if present (never generates a new key).
fn load_existing_key(
    mode: CredentialStoreMode,
    config_dir: &Path,
) -> anyhow::Result<Option<FileKey>> {
    let scope = current_cache_scope(mode, config_dir);
    if let Some(key) = cached_key(&scope) {
        return Ok(Some(key));
    }

    let key = match mode {
        CredentialStoreMode::Keyring => {
            let account = vault_key_account_for(config_dir);
            let Some(stored) = keychain::get_secret(SERVICE_NAME, &account)? else {
                return Ok(None);
            };
            let decoded = FileKey::from_stored_bytes(&stored).ok_or_else(|| {
                ConfigError::new(
                    UNREADABLE_CODE,
                    format!(
                        "Stored master key in OS keychain ({SERVICE_NAME}/{account}) is invalid. \
                         snow-cli did not modify it."
                    ),
                )
            })?;
            Some(decoded)
        }
        CredentialStoreMode::File => {
            let path = key_file_path_for(config_dir);
            ensure_not_symlink(&path, "credential key file")?;
            #[cfg(unix)]
            if let Ok(meta) = std::fs::metadata(&path) {
                use std::os::unix::fs::PermissionsExt;
                let mode_bits = meta.permissions().mode() & 0o777;
                if mode_bits & 0o077 != 0 {
                    tracing::warn!(
                        permissions = format_args!("{mode_bits:04o}"),
                        "credential key file has overly permissive permissions; expected 0600"
                    );
                }
            }
            let Some(raw) = encrypted_store::read_raw(&path)? else {
                return Ok(None);
            };
            let key = FileKey::from_stored_bytes(&raw).ok_or_else(|| {
                ConfigError::new(
                    UNREADABLE_CODE,
                    format!(
                        "Credential key file at {} is invalid. snow-cli did not modify it.",
                        path.display()
                    ),
                )
            })?;
            Some(key)
        }
        CredentialStoreMode::Env => {
            let raw = std::env::var(ENCRYPTION_KEY_ENV).unwrap_or_default();
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            let key = FileKey::decode_base64(trimmed).ok_or_else(|| {
                ConfigError::new(
                    UNREADABLE_CODE,
                    format!("{ENCRYPTION_KEY_ENV} must be a base64-encoded 32-byte key."),
                )
            })?;
            Some(key)
        }
        CredentialStoreMode::LegacyKeychain => return Ok(None),
    };

    if let Some(ref loaded) = key {
        store_cached_key(scope, loaded.clone());
    }
    Ok(key)
}

/// Persist a master key into the destination backing store for `mode`.
fn persist_key(mode: CredentialStoreMode, config_dir: &Path, key: &FileKey) -> anyhow::Result<()> {
    match mode {
        CredentialStoreMode::Keyring => {
            let account = vault_key_account_for(config_dir);
            keychain::set_password(SERVICE_NAME, &account, &key.encode_base64())?;
        }
        CredentialStoreMode::File => {
            let path = key_file_path_for(config_dir);
            ensure_not_symlink(&path, "credential key file")?;
            let encoded = format!("{}\n", key.encode_base64());
            encrypted_store::write_atomic(&path, encoded.as_bytes(), KEY_FILE_NAME)?;
        }
        CredentialStoreMode::Env => {
            return Err(ConfigError::new(
                UNREADABLE_CODE,
                format!(
                    "Cannot persist a generated key when credential_store is 'env'. Set {ENCRYPTION_KEY_ENV} to a base64-encoded 32-byte key."
                ),
            )
            .into());
        }
        CredentialStoreMode::LegacyKeychain => {}
    }
    store_cached_key(current_cache_scope(mode, config_dir), key.clone());
    Ok(())
}

/// Acquire an exclusive advisory lock around vault read-modify-write operations.
fn acquire_vault_lock(config_dir: &Path) -> anyhow::Result<File> {
    let existed = config_dir.exists();
    std::fs::create_dir_all(config_dir)?;
    #[cfg(unix)]
    if !existed {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) =
            std::fs::set_permissions(config_dir, std::fs::Permissions::from_mode(0o700))
        {
            tracing::debug!(%error, "could not restrict snow-cli config directory");
        }
    }

    let lock_path = lock_path_for(config_dir);
    ensure_not_symlink(&lock_path, "credential vault lock")?;
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&lock_path)?;
    file.lock()?;
    Ok(file)
}

fn unreadable_error(path: &Path, detail: &str) -> anyhow::Error {
    ConfigError::new(
        UNREADABLE_CODE,
        format!(
            "Encrypted credentials at {} {detail}. snow-cli did not modify them.",
            path.display()
        ),
    )
    .into()
}

fn missing_key_detail(mode: CredentialStoreMode) -> &'static str {
    match mode {
        CredentialStoreMode::Keyring => {
            "exist but their master key is unavailable in the OS keychain (missing or locked). Unlock the keychain or check `snow-cli profile storage status`"
        }
        CredentialStoreMode::File => {
            "exist but the key file credentials.key is missing. Restore credentials.key or switch storage mode"
        }
        CredentialStoreMode::Env => {
            "exist but SNOW_CLI_ENCRYPTION_KEY is not set. Export the base64 key and retry"
        }
        CredentialStoreMode::LegacyKeychain => "exist while legacy-keychain mode is selected",
    }
}

fn decrypt_payload(key: &FileKey, raw: &[u8], path: &Path) -> anyhow::Result<VaultPayload> {
    let json = encrypted_store::decrypt_envelope(key, raw).map_err(|error| match error {
        EnvelopeError::TruncatedOrCorrupted => unreadable_error(path, "are truncated or corrupted"),
        EnvelopeError::UnsupportedVersion(version) => ConfigError::new(
            UNREADABLE_CODE,
            format!(
                "Encrypted credentials at {} use format version {}, but snow-cli supports only \
                 version {FORMAT_VERSION}. Upgrade snow-cli to read them.",
                path.display(),
                version.map_or_else(|| "unknown".to_owned(), |v| v.to_string()),
            ),
        )
        .into(),
        EnvelopeError::DecryptFailed => unreadable_error(
            path,
            "could not be decrypted: the key does not match or the file is corrupted",
        ),
    })?;
    let payload: VaultPayload = serde_json::from_str(&json)
        .map_err(|_| unreadable_error(path, "contain invalid JSON payload"))?;
    if payload.version != VAULT_PAYLOAD_VERSION {
        return Err(ConfigError::new(
            UNREADABLE_CODE,
            format!(
                "Encrypted credentials at {} use payload version {}, but snow-cli supports only \
                 version {VAULT_PAYLOAD_VERSION}. Upgrade snow-cli to read them.",
                path.display(),
                payload.version,
            ),
        )
        .into());
    }
    Ok(payload)
}

fn load_vault_for_read(
    mode: CredentialStoreMode,
    config_dir: &Path,
) -> anyhow::Result<Option<VaultPayload>> {
    let path = vault_path_for(config_dir);
    ensure_not_symlink(&path, "credential vault")?;
    let Some(raw) = encrypted_store::read_raw(&path)? else {
        return Ok(None);
    };
    let key = load_existing_key(mode, config_dir)?
        .ok_or_else(|| unreadable_error(&path, missing_key_detail(mode)))?;
    Ok(Some(decrypt_payload(&key, &raw, &path)?))
}

fn load_or_create_for_write(
    mode: CredentialStoreMode,
    config_dir: &Path,
) -> anyhow::Result<(FileKey, VaultPayload, PathBuf)> {
    let path = vault_path_for(config_dir);
    ensure_not_symlink(&path, "credential vault")?;
    let raw = encrypted_store::read_raw(&path)?;
    let existing_key = load_existing_key(mode, config_dir)?;

    match (raw, existing_key) {
        (Some(bytes), Some(key)) => {
            let payload = decrypt_payload(&key, &bytes, &path)?;
            Ok((key, payload, path))
        }
        (Some(_), None) => Err(unreadable_error(&path, missing_key_detail(mode))),
        (None, Some(key)) => Ok((key, VaultPayload::default(), path)),
        (None, None) => {
            if mode == CredentialStoreMode::Env {
                return Err(ConfigError::new(
                    UNREADABLE_CODE,
                    format!(
                        "{STORE_MODE_ENV}=env (or credential_store = \"env\") requires {ENCRYPTION_KEY_ENV} to be set to a base64-encoded 32-byte key before writing credentials."
                    ),
                )
                .into());
            }
            let key = FileKey::generate()?;
            persist_key(mode, config_dir, &key)?;
            Ok((key, VaultPayload::default(), path))
        }
    }
}

fn save_payload(key: &FileKey, payload: &VaultPayload, path: &Path) -> anyhow::Result<()> {
    let json = serde_json::to_string(payload)?;
    let encrypted = encrypted_store::encrypt_envelope(key, &json)?;
    encrypted_store::write_atomic(path, &encrypted, VAULT_FILE_NAME)
}

/// Outcome of reading a credential from the encrypted vault.
pub enum VaultLookup {
    /// Entry was present in `credentials.enc`.
    Found(String),
    /// Entry was absent from `credentials.enc`.
    Missing,
}

/// Look up `key` (formatted `<profile>:<credential_type>`) in the vault.
pub fn get_entry(
    mode: CredentialStoreMode,
    config_dir: &Path,
    key: &str,
) -> anyhow::Result<VaultLookup> {
    let Some(payload) = load_vault_for_read(mode, config_dir)? else {
        return Ok(VaultLookup::Missing);
    };
    match payload.entries.get(key) {
        Some(secret) => Ok(VaultLookup::Found(secret.expose_secret().clone())),
        None => Ok(VaultLookup::Missing),
    }
}

/// Insert or update `key` in the encrypted vault under an exclusive file lock.
pub fn set_entry(
    mode: CredentialStoreMode,
    config_dir: &Path,
    key: &str,
    value: &str,
) -> anyhow::Result<()> {
    let _lock = acquire_vault_lock(config_dir)?;
    let (file_key, mut payload, path) = load_or_create_for_write(mode, config_dir)?;
    payload
        .entries
        .insert(key.to_string(), Secret::new(value.to_string()));
    save_payload(&file_key, &payload, &path)
}

/// Remove one or more keys from the encrypted vault in a single atomic write.
pub fn delete_entries(
    mode: CredentialStoreMode,
    config_dir: &Path,
    keys: &[String],
) -> anyhow::Result<()> {
    if keys.is_empty() {
        return Ok(());
    }
    let path = vault_path_for(config_dir);
    if !path.exists() {
        return Ok(());
    }
    let _lock = acquire_vault_lock(config_dir)?;
    let (file_key, mut payload, path) = load_or_create_for_write(mode, config_dir)?;
    let mut changed = false;
    for key in keys {
        if payload.entries.remove(key).is_some() {
            changed = true;
        }
    }
    if changed {
        save_payload(&file_key, &payload, &path)?;
    }
    Ok(())
}

/// Non-intrusive summary of local credential storage (never prompts the OS keychain).
#[derive(Debug, Clone, Serialize)]
pub struct StorageStatus {
    pub mode: CredentialStoreMode,
    pub source: ModeOrigin,
    pub vault_path: String,
    pub vault_exists: bool,
    pub key_file_path: String,
    pub key_file_exists: bool,
    pub env_key_set: bool,
}

pub fn storage_status(config_path: &Path) -> anyhow::Result<StorageStatus> {
    let resolved = resolve_mode_from_config_path(config_path)?;
    let config_dir = config_dir_for(config_path);
    let vault_path = vault_path_for(&config_dir);
    let key_file_path = key_file_path_for(&config_dir);
    let env_key_set = std::env::var(ENCRYPTION_KEY_ENV)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);

    Ok(StorageStatus {
        mode: resolved.mode,
        source: resolved.origin,
        vault_path: vault_path.display().to_string(),
        vault_exists: vault_path.exists(),
        key_file_path: key_file_path.display().to_string(),
        key_file_exists: key_file_path.exists(),
        env_key_set,
    })
}

fn config_dir_for(config_path: &Path) -> PathBuf {
    match config_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SwitchModeReport {
    pub status: &'static str,
    pub previous_mode: CredentialStoreMode,
    pub mode: CredentialStoreMode,
    pub vault_rekeyed: bool,
    pub key_migrated: bool,
    pub old_key_removed: bool,
    pub exported_to_legacy: Vec<String>,
    pub vault_path: String,
}

/// Switch `credential_store` in `config.toml` and migrate/re-wrap the vault key
/// (or export entries back to legacy keychain) so existing logins keep working.
pub fn switch_mode(
    config_path: &Path,
    target: CredentialStoreMode,
    keep_old_key: bool,
) -> anyhow::Result<SwitchModeReport> {
    let resolved = resolve_mode_from_config_path(config_path)?;
    if resolved.origin == ModeOrigin::Env && resolved.mode != target {
        return Err(ConfigError::new(
            "CONFIG_INVALID",
            format!(
                "{STORE_MODE_ENV} is set to '{}' in your environment and overrides config.toml. \
                 Unset {STORE_MODE_ENV} before running `profile storage set {}`.",
                resolved.mode.as_str(),
                target.as_str(),
            ),
        )
        .into());
    }

    let previous = resolved.mode;
    let config_dir = config_dir_for(config_path);
    let vault_path = vault_path_for(&config_dir);
    let _lock = acquire_vault_lock(&config_dir)?;

    if previous == target {
        let mut config = AppConfig::load_from(config_path)?;
        if config.credential_store.as_deref() != Some(target.as_str()) {
            config.credential_store = Some(target.as_str().to_string());
            config.save_to(config_path)?;
        }
        return Ok(SwitchModeReport {
            status: "unchanged",
            previous_mode: previous,
            mode: target,
            vault_rekeyed: false,
            key_migrated: false,
            old_key_removed: false,
            exported_to_legacy: Vec::new(),
            vault_path: vault_path.display().to_string(),
        });
    }

    ensure_not_symlink(&vault_path, "credential vault")?;
    let vault_bytes = encrypted_store::read_raw(&vault_path)?;
    let prev_key = if previous.uses_vault() {
        load_existing_key(previous, &config_dir)?
    } else {
        None
    };

    // If a vault exists under the previous vault mode, verify decryption before
    // mutating any key, vault file, or config.toml.
    let decrypted_payload = match (&vault_bytes, &prev_key) {
        (Some(bytes), Some(key)) => Some(decrypt_payload(key, bytes, &vault_path)?),
        (Some(_), None) if previous.uses_vault() => {
            return Err(unreadable_error(&vault_path, missing_key_detail(previous)));
        }
        _ => None,
    };

    let mut key_migrated = false;
    let mut vault_rekeyed = false;
    let mut old_key_removed = false;
    let mut exported_to_legacy = Vec::new();
    let mut remove_vault_after_save = false;

    match target {
        CredentialStoreMode::Keyring | CredentialStoreMode::File => {
            let target_existing = load_existing_key(target, &config_dir)?;
            match (prev_key, target_existing, &vault_bytes) {
                // Moving an existing vault key from previous mode -> target mode:
                // reuse the exact same key so credentials.enc needs no re-encryption.
                (Some(key), _, _) => {
                    persist_key(target, &config_dir, &key)?;
                    key_migrated = true;
                }
                // Coming from `legacy-keychain` with an existing vault on disk:
                // only succeed if the target already has a matching key that decrypts it.
                (None, Some(existing), Some(bytes)) => {
                    decrypt_payload(&existing, bytes, &vault_path)?;
                }
                (None, None, Some(_)) => {
                    return Err(unreadable_error(&vault_path, missing_key_detail(target)));
                }
                (None, Some(_), None) => {}
                (None, None, None) => {
                    let fresh = FileKey::generate()?;
                    persist_key(target, &config_dir, &fresh)?;
                    key_migrated = true;
                }
            }
        }
        CredentialStoreMode::Env => {
            let env_key =
                load_existing_key(CredentialStoreMode::Env, &config_dir)?.ok_or_else(|| {
                    ConfigError::new(
                        UNREADABLE_CODE,
                        format!(
                            "Set {ENCRYPTION_KEY_ENV} to a base64-encoded 32-byte key before switching credential_store to 'env'."
                        ),
                    )
                })?;
            match (&decrypted_payload, &vault_bytes) {
                (Some(payload), _) => {
                    save_payload(&env_key, payload, &vault_path)?;
                    vault_rekeyed = true;
                    key_migrated = true;
                }
                (None, Some(bytes)) => {
                    // Switching from `legacy-keychain` to `env` with an existing vault:
                    // verify the env key decrypts it before changing config.
                    decrypt_payload(&env_key, bytes, &vault_path)?;
                }
                (None, None) => {}
            }
        }
        CredentialStoreMode::LegacyKeychain => {
            // Export all vault entries back to legacy per-profile keychain items
            // so existing logins remain usable after switching away from the vault.
            if let Some(payload) = decrypted_payload.as_ref() {
                for (entry_key, secret) in &payload.entries {
                    keychain::set_password(SERVICE_NAME, entry_key, secret.expose_secret())?;
                    exported_to_legacy.push(entry_key.clone());
                }
            }
            if vault_bytes.is_some() && !keep_old_key {
                remove_vault_after_save = true;
            }
        }
    }

    // Persist config.toml; if that fails after re-encrypting the vault under a
    // new env key, restore the original encrypted bytes so config and vault agree.
    let mut config = AppConfig::load_from(config_path)?;
    config.credential_store = Some(target.as_str().to_string());
    if let Err(save_err) = config.save_to(config_path) {
        if vault_rekeyed && let Some(original) = vault_bytes.as_deref() {
            let _ = encrypted_store::write_atomic(&vault_path, original, VAULT_FILE_NAME);
        }
        return Err(save_err);
    }

    if remove_vault_after_save && vault_path.exists() {
        std::fs::remove_file(&vault_path)?;
    }

    // Never delete the old key when switching to `env` (an ephemeral shell variable
    // should not become the sole copy of the key) or when `--keep-old-key` was requested
    // or when `credentials.enc` still relies on it.
    let should_remove_old_key = !keep_old_key
        && target != CredentialStoreMode::Env
        && (target != CredentialStoreMode::LegacyKeychain || !vault_path.exists());

    if should_remove_old_key {
        match previous {
            CredentialStoreMode::Keyring if target != CredentialStoreMode::Keyring => {
                let account = vault_key_account_for(&config_dir);
                if keychain::delete_password(SERVICE_NAME, &account).is_ok() {
                    old_key_removed = true;
                }
            }
            CredentialStoreMode::File if target != CredentialStoreMode::File => {
                let old_key_file = key_file_path_for(&config_dir);
                if old_key_file.exists() && std::fs::remove_file(&old_key_file).is_ok() {
                    old_key_removed = true;
                }
            }
            _ => {}
        }
    }
    clear_key_cache();

    Ok(SwitchModeReport {
        status: "updated",
        previous_mode: previous,
        mode: target,
        vault_rekeyed,
        key_migrated,
        old_key_removed,
        exported_to_legacy,
        vault_path: vault_path.display().to_string(),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationReport {
    pub status: &'static str,
    pub mode: CredentialStoreMode,
    pub vault_path: String,
    pub migrated: Vec<String>,
    pub skipped: Vec<String>,
    pub deleted_legacy: Vec<String>,
}

/// Copy legacy per-profile OS keychain credentials for all configured profiles
/// into the active encrypted vault.
pub fn migrate_legacy_keychain(
    config_path: &Path,
    overwrite: bool,
    delete_legacy: bool,
) -> anyhow::Result<MigrationReport> {
    let resolved = resolve_mode_from_config_path(config_path)?;
    if !resolved.mode.uses_vault() {
        return Err(ConfigError::new(
            "CONFIG_INVALID",
            "Cannot migrate into the encrypted vault while credential_store is 'legacy-keychain'. Switch to 'keyring', 'file', or 'env' first.",
        )
        .into());
    }

    let config = AppConfig::load_from(config_path)?;
    let config_dir = config_dir_for(config_path);

    // Read legacy keychain items BEFORE acquiring the exclusive vault lock so
    // interactive OS keychain prompts never block other processes on the file lock.
    let mut legacy_entries = Vec::new();
    for profile_name in config.profiles.keys() {
        for cred_type in ALL_CREDENTIAL_TYPES {
            let entry_key = format!("{profile_name}:{cred_type}");
            if let Some(secret) = keychain::get_password(SERVICE_NAME, &entry_key)? {
                legacy_entries.push((entry_key, Secret::new(secret)));
            }
        }
    }

    let _lock = acquire_vault_lock(&config_dir)?;
    let (file_key, mut payload, vault_path) = load_or_create_for_write(resolved.mode, &config_dir)?;

    let mut migrated = Vec::new();
    let mut skipped = Vec::new();
    let mut legacy_keys_to_delete = Vec::new();

    for (entry_key, secret) in legacy_entries {
        if payload.entries.contains_key(&entry_key) && !overwrite {
            skipped.push(entry_key);
            continue;
        }
        payload.entries.insert(entry_key.clone(), secret);
        migrated.push(entry_key.clone());
        if delete_legacy {
            legacy_keys_to_delete.push(entry_key);
        }
    }

    if !migrated.is_empty() || !vault_path.exists() {
        save_payload(&file_key, &payload, &vault_path)?;
    }

    // Drop the vault file lock before performing legacy keychain deletions.
    drop(_lock);

    let mut deleted_legacy = Vec::new();
    for entry_key in legacy_keys_to_delete {
        keychain::delete_password(SERVICE_NAME, &entry_key)?;
        deleted_legacy.push(entry_key);
    }

    Ok(MigrationReport {
        status: "migrated",
        mode: resolved.mode,
        vault_path: vault_path.display().to_string(),
        migrated,
        skipped,
        deleted_legacy,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_modes_accepts_expected_values_and_rejects_unknown() {
        assert_eq!(
            CredentialStoreMode::parse("keyring").unwrap(),
            CredentialStoreMode::Keyring
        );
        assert_eq!(
            CredentialStoreMode::parse("FILE").unwrap(),
            CredentialStoreMode::File
        );
        assert_eq!(
            CredentialStoreMode::parse("env").unwrap(),
            CredentialStoreMode::Env
        );
        assert_eq!(
            CredentialStoreMode::parse("legacy-keychain").unwrap(),
            CredentialStoreMode::LegacyKeychain
        );
        assert!(CredentialStoreMode::parse("keychain").is_err());
        assert!(CredentialStoreMode::parse("plaintext").is_err());
    }

    #[test]
    fn key_cache_scope_debug_redacts_env_key() {
        let scope = KeyCacheScope {
            mode: CredentialStoreMode::Env,
            config_dir: PathBuf::from("/tmp/example"),
            test_store: Some("/tmp/store.json".to_string()),
            env_key_digest: Some([0xAB; 32]),
        };
        let rendered = format!("{scope:?}");
        assert!(rendered.contains("has_env_key: true"));
        assert!(!rendered.contains("171"));
    }

    #[test]
    fn vault_key_account_scopes_non_default_directories() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        let acc_a = vault_key_account_for(dir_a.path());
        let acc_b = vault_key_account_for(dir_b.path());
        assert!(acc_a.starts_with("credentials-key:"));
        assert!(acc_b.starts_with("credentials-key:"));
        assert_ne!(acc_a, acc_b);
        assert_eq!(acc_a, vault_key_account_for(dir_a.path()));
    }

    #[test]
    fn file_mode_round_trips_and_never_stores_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        clear_key_cache();
        set_entry(
            CredentialStoreMode::File,
            dir.path(),
            "dev:password",
            "hunter2",
        )
        .unwrap();
        set_entry(
            CredentialStoreMode::File,
            dir.path(),
            "prod:oauth_token",
            r#"{"access_token":"tok-123"}"#,
        )
        .unwrap();

        let vault_bytes = std::fs::read(vault_path_for(dir.path())).unwrap();
        assert!(!String::from_utf8_lossy(&vault_bytes).contains("hunter2"));
        assert!(!String::from_utf8_lossy(&vault_bytes).contains("tok-123"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let key_mode = std::fs::metadata(key_file_path_for(dir.path()))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            let vault_mode = std::fs::metadata(vault_path_for(dir.path()))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(key_mode, 0o600);
            assert_eq!(vault_mode, 0o600);
        }

        clear_key_cache();
        match get_entry(CredentialStoreMode::File, dir.path(), "dev:password").unwrap() {
            VaultLookup::Found(value) => assert_eq!(value, "hunter2"),
            VaultLookup::Missing => panic!("expected stored password"),
        }

        delete_entries(
            CredentialStoreMode::File,
            dir.path(),
            &["dev:password".to_string()],
        )
        .unwrap();
        assert!(matches!(
            get_entry(CredentialStoreMode::File, dir.path(), "dev:password").unwrap(),
            VaultLookup::Missing
        ));
    }

    #[test]
    fn file_mode_refuses_to_overwrite_unreadable_vault() {
        let dir = tempfile::tempdir().unwrap();
        clear_key_cache();
        set_entry(
            CredentialStoreMode::File,
            dir.path(),
            "dev:password",
            "keep-me",
        )
        .unwrap();
        let before = std::fs::read(vault_path_for(dir.path())).unwrap();

        // Replace key with a different random key.
        let wrong_key = FileKey::generate().unwrap();
        std::fs::write(
            key_file_path_for(dir.path()),
            format!("{}\n", wrong_key.encode_base64()),
        )
        .unwrap();
        clear_key_cache();

        let err = set_entry(
            CredentialStoreMode::File,
            dir.path(),
            "dev:password",
            "overwrite",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("did not modify"), "{err}");
        assert_eq!(std::fs::read(vault_path_for(dir.path())).unwrap(), before);
    }

    #[test]
    fn unsupported_vault_envelope_version_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        clear_key_cache();
        let key = FileKey::generate().unwrap();
        persist_key(CredentialStoreMode::File, dir.path(), &key).unwrap();
        let value = ciborium::Value::Map(vec![
            ("version".into(), ciborium::Value::Integer(99.into())),
            ("iv".into(), ciborium::Value::Bytes(vec![0; 12])),
            ("data".into(), ciborium::Value::Bytes(vec![0; 16])),
        ]);
        let mut raw = Vec::new();
        ciborium::into_writer(&value, &mut raw).unwrap();
        std::fs::write(vault_path_for(dir.path()), raw).unwrap();

        let err = get_entry(CredentialStoreMode::File, dir.path(), "dev:password")
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("format version 99"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_vault_or_key_file_is_refused() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("other");
        std::fs::write(&target, "dummy").unwrap();
        symlink(&target, vault_path_for(dir.path())).unwrap();

        let err = get_entry(CredentialStoreMode::File, dir.path(), "dev:password")
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("symlink"), "{err}");
    }

    #[test]
    fn concurrent_file_mode_writes_preserve_all_entries() {
        let dir = tempfile::tempdir().unwrap();
        clear_key_cache();
        // Initialize vault key once before spawning threads so all share the same key.
        set_entry(CredentialStoreMode::File, dir.path(), "init:password", "ok").unwrap();

        std::thread::scope(|scope| {
            for idx in 0..8 {
                let path = dir.path();
                scope.spawn(move || {
                    set_entry(
                        CredentialStoreMode::File,
                        path,
                        &format!("p{idx}:password"),
                        &format!("secret-{idx}"),
                    )
                    .unwrap();
                });
            }
        });

        clear_key_cache();
        for idx in 0..8 {
            match get_entry(
                CredentialStoreMode::File,
                dir.path(),
                &format!("p{idx}:password"),
            )
            .unwrap()
            {
                VaultLookup::Found(value) => assert_eq!(value, format!("secret-{idx}")),
                VaultLookup::Missing => panic!("missing entry for p{idx}"),
            }
        }
    }
}
