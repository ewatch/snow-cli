use crate::cli::args::IncidentArgs;

pub async fn handle(args: IncidentArgs) -> anyhow::Result<()> {
    match args.command {
        crate::cli::args::IncidentCommands::List { .. } => {
            tracing::info!("Listing incidents");
            todo!("incident list not yet implemented")
        }
        crate::cli::args::IncidentCommands::Get { number } => {
            tracing::info!("Getting incident: {}", number);
            todo!("incident get not yet implemented")
        }
        crate::cli::args::IncidentCommands::Create { .. } => {
            tracing::info!("Creating incident");
            todo!("incident create not yet implemented")
        }
        crate::cli::args::IncidentCommands::Update { number, .. } => {
            tracing::info!("Updating incident: {}", number);
            todo!("incident update not yet implemented")
        }
        crate::cli::args::IncidentCommands::Resolve { number, .. } => {
            tracing::info!("Resolving incident: {}", number);
            todo!("incident resolve not yet implemented")
        }
    }
}
