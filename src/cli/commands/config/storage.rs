use std::path::Path;

use crate::cli::args::{CliCredentialStoreMode, OutputFormat};
use crate::cli::output;
use crate::config::vault::{self, CredentialStoreMode};

fn to_store_mode(cli: CliCredentialStoreMode) -> CredentialStoreMode {
    match cli {
        CliCredentialStoreMode::Keyring => CredentialStoreMode::Keyring,
        CliCredentialStoreMode::File => CredentialStoreMode::File,
        CliCredentialStoreMode::Env => CredentialStoreMode::Env,
        CliCredentialStoreMode::LegacyKeychain => CredentialStoreMode::LegacyKeychain,
    }
}

pub(super) async fn handle_storage_status(
    config_path: &Path,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let status = vault::storage_status(config_path)?;
    output::print_output(&status, output_format)
}

pub(super) async fn handle_storage_set(
    config_path: &Path,
    mode: CliCredentialStoreMode,
    keep_old_key: bool,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let report = vault::switch_mode(config_path, to_store_mode(mode), keep_old_key)?;
    output::print_output(&report, output_format)
}

pub(super) async fn handle_storage_migrate(
    config_path: &Path,
    overwrite: bool,
    delete_legacy: bool,
    output_format: &OutputFormat,
) -> anyhow::Result<()> {
    let report = vault::migrate_legacy_keychain(config_path, overwrite, delete_legacy)?;
    output::print_output(&report, output_format)
}
