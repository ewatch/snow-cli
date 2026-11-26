//! Shared AES-256-GCM + CBOR encrypted credential file primitives.
//!
//! Used by both `now-sdk` 4.14+ interoperability (`~/.now-sdk/credentials.enc`)
//! and `snow-cli`'s own encrypted credential vault (`~/.servicenow/credentials.enc`).
//!
//! File layout: CBOR map `{version: 1, iv: <12 bytes>, data: <ciphertext || 16-byte tag>}`.

use std::io::Read as _;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Only envelope format version currently supported.
pub(crate) const FORMAT_VERSION: u64 = 1;
/// AES-256 key length in bytes.
pub(crate) const KEY_LEN: usize = 32;
/// Maximum encrypted credential store file size read into memory (16 MiB).
pub(crate) const MAX_CREDENTIAL_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// A 32-byte AES-256-GCM key. Never printed in Debug output and zeroized on drop.
#[derive(Clone)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub(crate) struct FileKey([u8; KEY_LEN]);

impl Drop for FileKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl ZeroizeOnDrop for FileKey {}

impl std::fmt::Debug for FileKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FileKey(<redacted>)")
    }
}

impl FileKey {
    /// Construct a key from raw bytes.
    #[cfg(test)]
    pub(crate) const fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(bytes)
    }

    /// Generate a fresh 32-byte key from the OS CSPRNG.
    pub(crate) fn generate() -> anyhow::Result<Self> {
        let mut bytes = [0u8; KEY_LEN];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| anyhow::anyhow!("could not generate a random encryption key"))?;
        Ok(Self(bytes))
    }

    /// Decode a raw 32-byte slice or a strict base64-encoded 32-byte string.
    pub(crate) fn from_stored_bytes(stored: &[u8]) -> Option<Self> {
        if let Ok(raw) = <[u8; KEY_LEN]>::try_from(stored) {
            return Some(Self(raw));
        }
        std::str::from_utf8(stored)
            .ok()
            .map(str::trim)
            .and_then(Self::decode_base64)
    }

    /// Strict base64 decoding: the value must re-encode to itself.
    pub(crate) fn decode_base64(value: &str) -> Option<Self> {
        let bytes = BASE64.decode(value).ok()?;
        let key = <[u8; KEY_LEN]>::try_from(bytes.as_slice()).ok()?;
        (BASE64.encode(key) == value).then_some(Self(key))
    }

    /// Encode the key as canonical standard base64.
    pub(crate) fn encode_base64(&self) -> String {
        BASE64.encode(self.0)
    }
}

/// Typed failure modes when decoding or decrypting an encrypted envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EnvelopeError {
    /// CBOR structure is truncated or cannot be parsed.
    TruncatedOrCorrupted,
    /// Envelope declares an unsupported format version.
    UnsupportedVersion(Option<u64>),
    /// Envelope fields are invalid or AES-256-GCM authentication / UTF-8 failed.
    DecryptFailed,
}

/// Read raw file bytes (capped at [`MAX_CREDENTIAL_FILE_BYTES`]), returning
/// `Ok(None)` when the file does not exist.
pub(crate) fn read_raw(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_CREDENTIAL_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CREDENTIAL_FILE_BYTES {
        anyhow::bail!(
            "Credential file {} exceeds the {} byte safety limit.",
            path.display(),
            MAX_CREDENTIAL_FILE_BYTES
        );
    }
    Ok(Some(bytes))
}

/// Encrypt UTF-8 plaintext into a CBOR `{version: 1, iv, data}` envelope with a fresh IV.
pub(crate) fn encrypt_envelope(key: &FileKey, plaintext: &str) -> anyhow::Result<Vec<u8>> {
    let iv: [u8; NONCE_LEN] = ring::rand::generate(&SystemRandom::new())
        .map_err(|_| anyhow::anyhow!("could not generate a random IV"))?
        .expose();
    let mut data = plaintext.as_bytes().to_vec();
    cipher(key)?
        .seal_in_place_append_tag(Nonce::assume_unique_for_key(iv), Aad::empty(), &mut data)
        .map_err(|_| anyhow::anyhow!("could not encrypt credentials payload"))?;
    encode_envelope(&iv, &data)
}

/// Decrypt a CBOR `{version: 1, iv, data}` envelope into UTF-8 plaintext.
pub(crate) fn decrypt_envelope(key: &FileKey, file: &[u8]) -> Result<String, EnvelopeError> {
    let (iv, mut data) = decode_envelope(file)?;
    let nonce = Nonce::try_assume_unique_for_key(&iv).map_err(|_| EnvelopeError::DecryptFailed)?;
    let cipher = cipher(key).map_err(|_| EnvelopeError::DecryptFailed)?;
    let plaintext = cipher
        .open_in_place(nonce, Aad::empty(), &mut data)
        .map_err(|_| EnvelopeError::DecryptFailed)?;
    String::from_utf8(plaintext.to_vec()).map_err(|_| EnvelopeError::DecryptFailed)
}

fn cipher(key: &FileKey) -> anyhow::Result<LessSafeKey> {
    let unbound = UnboundKey::new(&AES_256_GCM, &key.0)
        .map_err(|_| anyhow::anyhow!("invalid AES-256-GCM key length"))?;
    Ok(LessSafeKey::new(unbound))
}

fn decode_envelope(file: &[u8]) -> Result<(Vec<u8>, Vec<u8>), EnvelopeError> {
    let value: ciborium::Value =
        ciborium::from_reader(file).map_err(|_| EnvelopeError::TruncatedOrCorrupted)?;
    let map = value.as_map().ok_or(EnvelopeError::DecryptFailed)?;
    let field = |name: &str| {
        map.iter()
            .find(|(key, _)| key.as_text() == Some(name))
            .map(|(_, value)| value)
    };
    let version = field("version")
        .and_then(ciborium::Value::as_integer)
        .and_then(|version| u64::try_from(version).ok());
    if version != Some(FORMAT_VERSION) {
        return Err(EnvelopeError::UnsupportedVersion(version));
    }
    let bytes = |name: &str| {
        field(name)
            .and_then(ciborium::Value::as_bytes)
            .cloned()
            .ok_or(EnvelopeError::DecryptFailed)
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
/// directory (when created here), written to an exclusive unpredictable temp
/// file, synced, and renamed.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8], file_name: &str) -> anyhow::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid credential file path"))?;
    // Restrict only a directory created here: an existing one (possibly a
    // symlink to a shared directory) keeps the permissions its owner chose.
    let existed = dir.exists();
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    if !existed {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)) {
            tracing::debug!(%error, "could not restrict credentials directory");
        }
    }
    // Unpredictable name, created exclusively: a symlink planted in a shared
    // credentials directory is never followed or truncated.
    let mut suffix = [0u8; 16];
    SystemRandom::new()
        .fill(&mut suffix)
        .map_err(|_| anyhow::anyhow!("could not generate a temporary file name"))?;
    let suffix: String = suffix.iter().map(|b| format!("{b:02x}")).collect();
    let tmp = dir.join(format!(".{file_name}.tmp-{suffix}"));
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

    const KEY: FileKey = FileKey::from_bytes([7u8; KEY_LEN]);

    #[test]
    fn envelope_round_trips_with_fresh_iv() {
        let json = r#"{"dev:password":"secret"}"#;
        let first = encrypt_envelope(&KEY, json).unwrap();
        let second = encrypt_envelope(&KEY, json).unwrap();
        assert_eq!(decrypt_envelope(&KEY, &first).unwrap(), json);
        assert_ne!(first, second);
    }

    #[test]
    fn wrong_key_and_tampering_fail() {
        let mut file = encrypt_envelope(&KEY, "{}").unwrap();
        assert_eq!(
            decrypt_envelope(&FileKey::from_bytes([8u8; KEY_LEN]), &file),
            Err(EnvelopeError::DecryptFailed)
        );
        let last = file.len() - 1;
        file[last] ^= 1;
        assert_eq!(
            decrypt_envelope(&KEY, &file),
            Err(EnvelopeError::DecryptFailed)
        );
    }

    #[test]
    fn unsupported_version_is_reported() {
        let value = ciborium::Value::Map(vec![
            ("version".into(), ciborium::Value::Integer(2.into())),
            ("iv".into(), ciborium::Value::Bytes(vec![0; 12])),
            ("data".into(), ciborium::Value::Bytes(vec![0; 16])),
        ]);
        let mut file = Vec::new();
        ciborium::into_writer(&value, &mut file).unwrap();
        assert_eq!(
            decrypt_envelope(&KEY, &file),
            Err(EnvelopeError::UnsupportedVersion(Some(2)))
        );
    }

    #[test]
    fn base64_key_round_trip_and_strictness() {
        let key = FileKey::generate().unwrap();
        let encoded = key.encode_base64();
        assert_eq!(FileKey::decode_base64(&encoded), Some(key));
        assert!(FileKey::decode_base64(&encoded[..40]).is_none());
        assert!(FileKey::decode_base64(&format!("{encoded}x")).is_none());
        assert!(FileKey::decode_base64(&BASE64.encode([9u8; 16])).is_none());
    }

    #[test]
    fn key_debug_is_redacted() {
        assert_eq!(format!("{KEY:?}"), "FileKey(<redacted>)");
    }

    #[test]
    fn file_key_implements_zeroize_on_drop() {
        fn assert_zeroize_on_drop<T: zeroize::ZeroizeOnDrop>() {}
        assert_zeroize_on_drop::<FileKey>();
    }

    #[test]
    fn read_raw_rejects_oversized_credential_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.enc");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_CREDENTIAL_FILE_BYTES + 1).unwrap();
        drop(file);
        let err = read_raw(&path).unwrap_err().to_string();
        assert!(err.contains("safety limit"), "{err}");
    }
}
