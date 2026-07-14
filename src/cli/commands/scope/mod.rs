use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;

use crate::cli::args::{OutputFormat, ScopeArgs, ScopeCommands, ScopeDetailLevel, ScopeListKind};
use crate::cli::output;
use crate::client::pagination::PaginationConfig;
use crate::models::identifiers::{EncodedQueryValue, SysId, TableName};
use crate::models::record::Record;

mod inventory;
mod list;
mod move_file;
mod types;

use inventory::*;
use list::*;
use move_file::*;
use types::*;

pub async fn handle(
    args: ScopeArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    match args.command {
        ScopeCommands::List {
            search,
            kind,
            show_source_table,
            show_sys_id,
        } => {
            handle_list(
                profile,
                format,
                instance,
                timeout_secs,
                search.as_ref(),
                &kind,
                ScopeListTextOptions {
                    show_source_table,
                    show_sys_id,
                },
            )
            .await
        }
        ScopeCommands::Inspect { scope, details } => {
            handle_inspect(profile, format, instance, timeout_secs, &scope, details).await
        }
        ScopeCommands::Inventory { scope } => {
            handle_inventory(profile, format, instance, timeout_secs, &scope).await
        }
        ScopeCommands::MoveFile {
            table,
            sys_id,
            target_scope,
            dry_run,
            yes,
        } => {
            handle_move_file(
                profile,
                format,
                instance,
                timeout_secs,
                MoveFileRequest {
                    table: &table,
                    sys_id: &sys_id,
                    target_scope: &target_scope,
                    dry_run,
                    yes,
                },
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests;
