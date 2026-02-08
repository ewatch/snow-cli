use crate::cli::args::ApiArgs;

pub async fn handle(args: ApiArgs) -> anyhow::Result<()> {
    match args.command {
        crate::cli::args::ApiCommands::Get { path, .. } => {
            tracing::info!("GET {}", path);
            todo!("api get not yet implemented")
        }
        crate::cli::args::ApiCommands::Post { path, .. } => {
            tracing::info!("POST {}", path);
            todo!("api post not yet implemented")
        }
        crate::cli::args::ApiCommands::Put { path, .. } => {
            tracing::info!("PUT {}", path);
            todo!("api put not yet implemented")
        }
        crate::cli::args::ApiCommands::Delete { path, .. } => {
            tracing::info!("DELETE {}", path);
            todo!("api delete not yet implemented")
        }
    }
}
