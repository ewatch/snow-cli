use crate::cli::args::AuthArgs;

pub async fn handle(args: AuthArgs) -> anyhow::Result<()> {
    match args.command {
        crate::cli::args::AuthCommands::Login => {
            tracing::info!("Running auth login");
            todo!("auth login not yet implemented")
        }
        crate::cli::args::AuthCommands::Logout => {
            tracing::info!("Running auth logout");
            todo!("auth logout not yet implemented")
        }
        crate::cli::args::AuthCommands::Status => {
            tracing::info!("Checking auth status");
            todo!("auth status not yet implemented")
        }
        crate::cli::args::AuthCommands::Token => {
            tracing::info!("Printing access token");
            todo!("auth token not yet implemented")
        }
    }
}
