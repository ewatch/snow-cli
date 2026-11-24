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

use super::NOW_SDK_SERVICE_NAME;
pub(super) use crate::config::encrypted_store::FileKey;
use crate::config::encrypted_store::{self, EnvelopeError, FORMAT_VERSION};
use crate::config::error::ConfigError;
use crate::config::keychain;

/// Error code for now-sdk credentials snow-cli cannot use as found.
const UNREADABLE_CODE: &str = "NOW_SDK_CREDENTIALS_UNREADABLE";

/// Keychain account holding the file key; its presence marks the new format.
const KEY_ACCOUNT: &str = "now-sdk-credentials-key";
/// File name inside the credentials directory.
const FILE_NAME: &str = "credentials.enc";
/// Overrides the credentials directory (default `~/.now-sdk`).
const DIR_ENV: &str = "SN_SDK_CREDENTIALS_DIR";
/// `keyring` (default) or `env`.
const KEY_SOURCE_ENV: &str = "SN_SDK_KEY_SOURCE";
/// Base64 key used when `SN_SDK_KEY_SOURCE=env`.
const KEY_ENV: &str = "SN_SDK_ENCRYPTION_KEY";

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
                Ok(FileKey::from_stored_bytes(&stored))
            }
            Self::Env => {
                let value = std::env::var(KEY_ENV).unwrap_or_default();
                let value = value.trim();
                if value.is_empty() {
                    return Ok(None);
                }
                FileKey::decode_base64(value).map(Some).ok_or_else(|| {
                    anyhow::anyhow!("{KEY_ENV} must be a base64-encoded 32-byte key.")
                })
            }
        }
    }
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
    encrypted_store::read_raw(path)
}

/// Decrypt a `credentials.enc` payload to the alias-store JSON.
pub(super) fn decrypt(key: &FileKey, file: &[u8], path: &Path) -> anyhow::Result<String> {
    encrypted_store::decrypt_envelope(key, file).map_err(|error| match error {
        EnvelopeError::TruncatedOrCorrupted => ConfigError::new(
            UNREADABLE_CODE,
            format!(
                "now-sdk credentials at {} are truncated or corrupted.",
                path.display()
            ),
        )
        .into(),
        EnvelopeError::UnsupportedVersion(version) => ConfigError::new(
            UNREADABLE_CODE,
            format!(
                "now-sdk credentials at {} use format version {}, but snow-cli supports only \
                 version {FORMAT_VERSION}. Upgrade snow-cli to read them.",
                path.display(),
                version.map_or_else(|| "unknown".to_owned(), |v| v.to_string()),
            ),
        )
        .into(),
        EnvelopeError::DecryptFailed => unreadable(path),
    })
}

/// Encrypt alias-store JSON into a `credentials.enc` payload with a fresh IV.
pub(super) fn encrypt(key: &FileKey, json: &str) -> anyhow::Result<Vec<u8>> {
    encrypted_store::encrypt_envelope(key, json)
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

/// Replace `path` atomically with `bytes`: owner-only file in an owner-only
/// directory, written to a temp file and renamed, as now-sdk does.
pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    encrypted_store::write_atomic(path, bytes, FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::encrypted_store::KEY_LEN;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;

    const KEY: FileKey = FileKey::from_bytes([7u8; KEY_LEN]);

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
        let error = decrypt(&FileKey::from_bytes([8u8; KEY_LEN]), &file, path).unwrap_err();
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
        assert!(FileKey::decode_base64(&encoded).is_some());
        assert!(FileKey::decode_base64(&encoded[..40]).is_none());
        assert!(FileKey::decode_base64(&format!("{encoded}x")).is_none());
        assert!(FileKey::decode_base64(&BASE64.encode([9u8; 16])).is_none());
    }

    #[test]
    fn key_debug_is_redacted() {
        assert_eq!(format!("{KEY:?}"), "FileKey(<redacted>)");
    }
}
