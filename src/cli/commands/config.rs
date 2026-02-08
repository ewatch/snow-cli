use crate::cli::args::ConfigArgs;

pub async fn handle(args: ConfigArgs) -> anyhow::Result<()> {
    match args.command {
        crate::cli::args::ConfigCommands::Init => {
            tracing::info!("Running config init");
            todo!("config init not yet implemented")
        }
        crate::cli::args::ConfigCommands::SetProfile { name } => {
            tracing::info!("Setting profile: {}", name);
            todo!("config set-profile not yet implemented")
        }
        crate::cli::args::ConfigCommands::ListProfiles => {
            tracing::info!("Listing profiles");
            todo!("config list-profiles not yet implemented")
        }
        crate::cli::args::ConfigCommands::UseProfile { name } => {
            tracing::info!("Activating profile: {}", name);
            todo!("config use-profile not yet implemented")
        }
        crate::cli::args::ConfigCommands::Show => {
            tracing::info!("Showing current config");
            todo!("config show not yet implemented")
        }
    }
}
