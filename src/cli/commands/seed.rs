use crate::cli::args::{OutputFormat, SeedArgs, SeedCommands};

pub async fn handle(
    args: SeedArgs,
    _profile: &str,
    _format: &OutputFormat,
    _instance: Option<&str>,
    _timeout_secs: Option<u64>,
    _proxy: &crate::client::ProxyOptions,
) -> anyhow::Result<()> {
    match args.command {
        SeedCommands::Plan { .. } => {
            anyhow::bail!("`seed plan` is planned but not implemented yet")
        }
        SeedCommands::Apply { .. } => {
            anyhow::bail!("`seed apply` is planned but not implemented yet")
        }
        SeedCommands::Cleanup { .. } => {
            anyhow::bail!("`seed cleanup` is planned but not implemented yet")
        }
    }
}
