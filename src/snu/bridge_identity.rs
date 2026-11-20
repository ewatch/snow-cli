//! Stable identity of this user's snow-cli SN-Utils bridge.
//!
//! SN-Utils 10.3+ refreshes session tokens on its own when a bridge
//! reconnects, but only for a bridge the user has handed a token to before
//! with a manual `/token`. It recognises the bridge by the `bridgeId` in
//! `hostHello`: a random 64-hex-character id, created once and kept private.
//! snow-cli keeps its own id beside its config file rather than sharing
//! SN-Utils' `~/.sn-scriptsync/bridge-id`, so trust given to sn-scriptsync or
//! `snu` is never extended to snow-cli without a `/token` of its own.
//!
//! Every failure yields an empty id. The helper then never refreshes
//! automatically and a manual `/token` is needed, as before 10.3.

use std::path::Path;

use ring::rand::{SecureRandom, SystemRandom};

/// File name of the identity inside snow-cli's config directory.
const FILE_NAME: &str = "snu-bridge-id";
/// Random bytes in an id (64 hex characters, the format SN-Utils accepts).
const ID_BYTES: usize = 32;

/// The bridge id from snow-cli's config directory, created on first use.
pub fn load() -> String {
    let config = crate::config::profile::AppConfig::config_path();
    match config.parent() {
        Some(dir) => load_from(dir),
        None => String::new(),
    }
}

/// Read `dir/snu-bridge-id`, creating it if missing. Never replaces an
/// existing file, so concurrent bridges converge on the same id.
pub fn load_from(dir: &Path) -> String {
    let file = dir.join(FILE_NAME);
    match read(&file) {
        Ok(id) => return id,
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return String::new(),
        Err(_) => {}
    }
    create(dir, &file).unwrap_or_default()
}

fn read(file: &Path) -> std::io::Result<String> {
    let meta = std::fs::symlink_metadata(file)?;
    if !meta.is_file() {
        return Ok(String::new());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Existing files need the same privacy as newly created ones.
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))?;
    }
    let id = std::fs::read_to_string(file)?.trim().to_owned();
    Ok(if is_valid(&id) { id } else { String::new() })
}

fn create(dir: &Path, file: &Path) -> Option<String> {
    let mut bytes = [0u8; ID_BYTES];
    SystemRandom::new().fill(&mut bytes).ok()?;
    let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::create_dir_all(dir).ok()?;

    let mut suffix = [0u8; 8];
    SystemRandom::new().fill(&mut suffix).ok()?;
    let temporary = dir.join(format!(
        ".{FILE_NAME}-{}",
        suffix
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let published = (|| -> std::io::Result<()> {
        use std::io::Write as _;
        options
            .open(&temporary)?
            .write_all(format!("{id}\n").as_bytes())?;
        // Publish only a complete file, and never replace another bridge's id.
        match std::fs::hard_link(&temporary, file) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(error),
        }
    })();
    let _ = std::fs::remove_file(&temporary);
    published.ok()?;
    read(file).ok().filter(|id| !id.is_empty())
}

fn is_valid(id: &str) -> bool {
    id.len() == ID_BYTES * 2 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_a_private_id_once_and_reuses_it() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_from(dir.path());
        assert!(is_valid(&first), "{first}");
        assert_eq!(load_from(dir.path()), first);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(FILE_NAME))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "temporary files are removed");
    }

    #[test]
    fn an_invalid_file_is_not_replaced_and_yields_no_id() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "not-an-id\n").unwrap();
        assert_eq!(load_from(dir.path()), "");
        assert_eq!(
            std::fs::read_to_string(dir.path().join(FILE_NAME)).unwrap(),
            "not-an-id\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere");
        std::fs::write(&target, format!("{}\n", "a".repeat(64))).unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join(FILE_NAME)).unwrap();
        assert_eq!(load_from(dir.path()), "");
    }
}
