//! now-sdk 4.14+ encrypted credential file.
//!
//! now-sdk 4.14.0 moved its alias store out of the OS keychain blob
//! (`ServiceNow` / `now-sdk`) into `credentials.enc`. The file is a CBOR map
//! `{version: 1, iv, data}`: `data` is the alias-store JSON encrypted with
//! AES-256-GCM, with the 16-byte tag appended, under a 12-byte `iv`. The
//! 32-byte key lives in the keychain (`ServiceNow` /
//! `now-sdk-credentials-key`) as base64 text (4.14.2+) or raw bytes (4.14.0),
//! or, with `SN_SDK_KEY_SOURCE=env`, in `SN_SDK_ENCRYPTION_KEY`.
//!
//! This mirrors now-sdk's `credential-store` module, including its refusal
//! to overwrite a file it cannot decrypt.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};

use super::NOW_SDK_SERVICE_NAME;
use crate::config::error::ConfigError;
use crate::config::keychain;

/// Error code for now-sdk credentials snow-cli cannot use as found.
const UNREADABLE_CODE: &str = "NOW_SDK_CREDENTIALS_UNREADABLE";

/// Keychain account holding the file key; its presence marks the new format.
const KEY_ACCOUNT: &str = "now-sdk-credentials-key";
/// File name inside the credentials directory.
const FILE_NAME: &str = "credentials.enc";
/// Only file format version now-sdk 4.14.2 reads or writes.
const FORMAT_VERSION: u64 = 1;
/// AES-256 key length.
const KEY_LEN: usize = 32;
/// Overrides the credentials directory (default `~/.now-sdk`).
const DIR_ENV: &str = "SN_SDK_CREDENTIALS_DIR";
/// `keyring` (default) or `env`.
const KEY_SOURCE_ENV: &str = "SN_SDK_KEY_SOURCE";
/// Base64 key used when `SN_SDK_KEY_SOURCE=env`.
const KEY_ENV: &str = "SN_SDK_ENCRYPTION_KEY";

/// A 32-byte AES-256-GCM key. Never printed.
pub(super) struct FileKey([u8; KEY_LEN]);

impl std::fmt::Debug for FileKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FileKey(<redacted>)")
    }
}

/// Where the active key comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeySource {
    Keyring,
    Env,
}

impl KeySource {
    pub(super) fn from_env() -> anyhow::Result<Self> {
        // An empty value means unset, as in now-sdk.
        match std::env::var(KEY_SOURCE_ENV).unwrap_or_default().as_str() {
            "" | "keyring" => Ok(Self::Keyring),
            "env" => Ok(Self::Env),
            other => anyhow::bail!(
                "Unknown {KEY_SOURCE_ENV} \"{other}\". Expected one of: keyring, env."
            ),
        }
    }

    /// The key, or `None` when none is stored. A keyring value that is
    /// neither raw nor base64 32 bytes counts as missing, as in now-sdk; a
    /// malformed env key is an error.
    pub(super) fn key(self) -> anyhow::Result<Option<FileKey>> {
        match self {
            Self::Keyring => {
                let Some(stored) = keychain::get_secret(NOW_SDK_SERVICE_NAME, KEY_ACCOUNT)? else {
                    return Ok(None);
                };
                if let Ok(raw) = <[u8; KEY_LEN]>::try_from(stored.as_slice()) {
                    return Ok(Some(FileKey(raw)));
                }
                Ok(std::str::from_utf8(&stored)
                    .ok()
                    .and_then(decode_base64_key))
            }
            Self::Env => {
                let value = std::env::var(KEY_ENV).unwrap_or_default();
                let value = value.trim();
                if value.is_empty() {
                    return Ok(None);
                }
                decode_base64_key(value).map(Some).ok_or_else(|| {
                    anyhow::anyhow!("{KEY_ENV} must be a base64-encoded 32-byte key.")
                })
            }
        }
    }
}

/// Strict base64 decoding: the value must re-encode to itself.
fn decode_base64_key(value: &str) -> Option<FileKey> {
    let bytes = BASE64.decode(value).ok()?;
    let key = <[u8; KEY_LEN]>::try_from(bytes.as_slice()).ok()?;
    (BASE64.encode(key) == value).then_some(FileKey(key))
}

/// The directory holding `credentials.enc`.
/// `None` when neither `SN_SDK_CREDENTIALS_DIR` nor a home directory is set:
/// never fall back to the current directory, which may be a repository.
pub(super) fn credentials_dir() -> Option<PathBuf> {
    match std::env::var(DIR_ENV) {
        Ok(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => crate::config::profile::home_dir()
            .filter(|home| !home.as_os_str().is_empty())
            .map(|home| home.join(".now-sdk")),
    }
}

pub(super) fn credentials_path() -> Option<PathBuf> {
    credentials_dir().map(|dir| dir.join(FILE_NAME))
}

/// Raw file bytes, or `None` when the file does not exist.
pub(super) fn read_raw(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Decrypt a `credentials.enc` payload to the alias-store JSON.
pub(super) fn decrypt(key: &FileKey, file: &[u8], path: &Path) -> anyhow::Result<String> {
    let (iv, mut data) = decode_envelope(file, path)?;
    let nonce = Nonce::try_assume_unique_for_key(&iv).map_err(|_| unreadable(path))?;
    let plaintext = cipher(key)
        .open_in_place(nonce, Aad::empty(), &mut data)
        .map_err(|_| unreadable(path))?;
    String::from_utf8(plaintext.to_vec()).map_err(|_| unreadable(path))
}

/// Encrypt alias-store JSON into a `credentials.enc` payload with a fresh IV.
pub(super) fn encrypt(key: &FileKey, json: &str) -> anyhow::Result<Vec<u8>> {
    let mut iv = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut iv)
        .map_err(|_| anyhow::anyhow!("could not generate a random IV"))?;
    let mut data = json.as_bytes().to_vec();
    cipher(key)
        .seal_in_place_append_tag(Nonce::assume_unique_for_key(iv), Aad::empty(), &mut data)
        .map_err(|_| anyhow::anyhow!("could not encrypt now-sdk credentials"))?;
    encode_envelope(&iv, &data)
}

fn cipher(key: &FileKey) -> LessSafeKey {
    // A 32-byte key is always valid for AES-256-GCM.
    LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &key.0).unwrap_or_else(|_| unreachable!()))
}

fn unreadable(path: &Path) -> anyhow::Error {
    ConfigError::new(
        UNREADABLE_CODE,
        format!(
            "now-sdk credentials at {} could not be decrypted: the key does not match or the file \
             is corrupted. snow-cli did not modify them; check them with now-sdk.",
            path.display()
        ),
    )
    .into()
}

fn decode_envelope(file: &[u8], path: &Path) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let value: ciborium::Value = ciborium::from_reader(file).map_err(|_| {
        ConfigError::new(
            UNREADABLE_CODE,
            format!(
                "now-sdk credentials at {} are truncated or corrupted.",
                path.display()
            ),
        )
    })?;
    let map = value.as_map().ok_or_else(|| unreadable(path))?;
    let field = |name: &str| {
        map.iter()
            .find(|(key, _)| key.as_text() == Some(name))
            .map(|(_, value)| value)
    };
    let version = field("version")
        .and_then(ciborium::Value::as_integer)
        .and_then(|version| u64::try_from(version).ok());
    if version != Some(FORMAT_VERSION) {
        return Err(ConfigError::new(
            UNREADABLE_CODE,
            format!(
                "now-sdk credentials at {} use format version {}, but snow-cli supports only \
                 version {FORMAT_VERSION}. Upgrade snow-cli to read them.",
                path.display(),
                version.map_or_else(|| "unknown".to_owned(), |v| v.to_string()),
            ),
        )
        .into());
    }
    let bytes = |name: &str| {
        field(name)
            .and_then(ciborium::Value::as_bytes)
            .cloned()
            .ok_or_else(|| unreadable(path))
    };
    Ok((bytes("iv")?, bytes("data")?))
}

fn encode_envelope(iv: &[u8], data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let value = ciborium::Value::Map(vec![
        (
            "version".into(),
            ciborium::Value::Integer(FORMAT_VERSION.into()),
        ),
        ("iv".into(), ciborium::Value::Bytes(iv.to_vec())),
        ("data".into(), ciborium::Value::Bytes(data.to_vec())),
    ]);
    let mut out = Vec::new();
    ciborium::into_writer(&value, &mut out)?;
    Ok(out)
}

/// Replace `path` atomically with `bytes`: owner-only file in an owner-only
/// directory, written to a temp file and renamed, as now-sdk does.
pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid now-sdk credentials path"))?;
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Best effort: the directory may be a shared mount this user does not own.
        if let Err(error) = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)) {
            tracing::debug!(%error, "could not restrict now-sdk credentials directory");
        }
    }
    // Unpredictable name, created exclusively: a symlink planted in a shared
    // credentials directory is never followed or truncated.
    let mut suffix = [0u8; 16];
    SystemRandom::new()
        .fill(&mut suffix)
        .map_err(|_| anyhow::anyhow!("could not generate a temporary file name"))?;
    let suffix: String = suffix.iter().map(|b| format!("{b:02x}")).collect();
    let tmp = dir.join(format!(".{FILE_NAME}.tmp-{suffix}"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let write = || -> std::io::Result<()> {
        use std::io::Write as _;
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    };
    write().inspect_err(|error| {
        // Only remove a temp file this call created.
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            let _ = std::fs::remove_file(&tmp);
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: FileKey = FileKey([7u8; KEY_LEN]);

    #[test]
    fn round_trips_through_the_envelope() {
        let path = Path::new("credentials.enc");
        let json = r#"{"dev":{"isDefault":true,"alias":"dev"}}"#;
        let file = encrypt(&KEY, json).unwrap();
        assert_eq!(decrypt(&KEY, &file, path).unwrap(), json);
        // Fresh IV per write.
        assert_ne!(encrypt(&KEY, json).unwrap(), file);
    }

    #[test]
    fn wrong_key_or_tampering_is_rejected_without_details() {
        let path = Path::new("credentials.enc");
        let mut file = encrypt(&KEY, "{}").unwrap();
        let error = decrypt(&FileKey([8u8; KEY_LEN]), &file, path).unwrap_err();
        assert!(error.to_string().contains("did not modify"), "{error}");
        let last = file.len() - 1;
        file[last] ^= 1;
        assert!(decrypt(&KEY, &file, path).is_err());
    }

    #[test]
    fn newer_format_version_asks_for_an_upgrade() {
        let value = ciborium::Value::Map(vec![
            ("version".into(), ciborium::Value::Integer(2.into())),
            ("iv".into(), ciborium::Value::Bytes(vec![0; 12])),
            ("data".into(), ciborium::Value::Bytes(vec![0; 16])),
        ]);
        let mut file = Vec::new();
        ciborium::into_writer(&value, &mut file).unwrap();
        let error = decrypt(&KEY, &file, Path::new("x")).unwrap_err();
        assert!(error.to_string().contains("format version 2"), "{error}");
    }

    #[test]
    fn base64_keys_must_round_trip_exactly() {
        let encoded = BASE64.encode([9u8; KEY_LEN]);
        assert!(decode_base64_key(&encoded).is_some());
        assert!(decode_base64_key(&encoded[..40]).is_none());
        assert!(decode_base64_key(&format!("{encoded}x")).is_none());
        assert!(decode_base64_key(&BASE64.encode([9u8; 16])).is_none());
    }

    #[test]
    fn key_debug_is_redacted() {
        assert_eq!(format!("{KEY:?}"), "FileKey(<redacted>)");
    }
}
