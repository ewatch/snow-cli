use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::cli::args::{DataArgs, DataCommands, OutputFormat};
use crate::cli::output;
use crate::client::pagination::PaginationConfig;
use crate::models::identifiers::TableName;
use crate::models::record::{Record, SingleRecordResponse};

const LONG_RUNNING_TIMEOUT_SECS: u64 = 180;

pub async fn handle(
    args: DataArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    match args.command {
        DataCommands::Export {
            table,
            query,
            fields,
            limit,
            order_by,
            out_path,
        } => {
            let export = ExportRequest {
                table,
                query,
                fields,
                limit,
                order_by,
                out_path,
            };
            handle_export(profile, format, instance, timeout_secs, export).await
        }
        DataCommands::ExportPackage { file, out_dir } => {
            ensure_json_output(format, "data export-package")?;
            handle_export_package(profile, format, instance, timeout_secs, &file, &out_dir).await
        }
        DataCommands::Validate { file } => {
            ensure_json_output(format, "data validate")?;
            handle_validate(profile, format, instance, timeout_secs, &file).await
        }
        DataCommands::Import {
            file,
            dry_run,
            import_set_table,
            fail_on_error,
        } => {
            ensure_json_output(format, "data import")?;
            handle_import(
                profile,
                format,
                instance,
                timeout_secs,
                &file,
                ImportExecutionOptions {
                    dry_run,
                    import_set_table: import_set_table.as_ref(),
                    fail_on_error,
                },
            )
            .await
        }
    }
}

mod flat;
mod package;
mod types;

use flat::*;
use package::*;
use types::*;

#[cfg(test)]
mod tests;
