use clap::{Args, Subcommand};

const SEED_AFTER_HELP: &str = "Examples:\n  snow-cli seed plan --file qa-fixture.json\n  snow-cli seed apply --file qa-fixture.json\n  snow-cli seed cleanup <run-id> --dry-run";
// --- Seed ---

#[derive(Args, Debug)]
#[command(after_help = SEED_AFTER_HELP)]
pub struct SeedArgs {
    #[command(subcommand)]
    pub command: SeedCommands,
}

#[derive(Subcommand, Debug)]
pub enum SeedCommands {
    /// Validate a seed spec and show the execution plan
    Plan {
        /// Seed specification file
        #[arg(long, short = 'f')]
        file: String,
    },

    /// Create test data from a seed spec
    Apply {
        /// Seed specification file
        #[arg(long, short = 'f')]
        file: String,
    },

    /// Remove data created by a prior seed run
    Cleanup {
        /// Seed run identifier
        run_id: String,

        /// Preview what would be deleted without mutating data
        #[arg(long)]
        dry_run: bool,

        /// Delete tracked records without prompting
        #[arg(long)]
        yes: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn test_parse_seed_cleanup_dry_run() {
        let cli = Cli::parse_from(["snow-cli", "seed", "cleanup", "run-123", "--dry-run"]);

        match cli.command {
            Commands::Seed(args) => match args.command {
                SeedCommands::Cleanup {
                    run_id,
                    dry_run,
                    yes,
                } => {
                    assert_eq!(run_id, "run-123");
                    assert!(dry_run);
                    assert!(!yes);
                }
                _ => panic!("Expected Seed Cleanup command"),
            },
            _ => panic!("Expected Seed command"),
        }
    }
}
