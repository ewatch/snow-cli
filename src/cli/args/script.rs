use clap::{Args, Subcommand};

const SCRIPT_RUN_AFTER_HELP: &str = "Examples:\n  snow-cli script run --code 'gs.info(\"hello from snow-cli\")'\n  snow-cli script run --file ./cleanup.js --sandbox\n  printf '%s' 'gs.info(\"from stdin\")' | snow-cli script run --scope x_my_app\n  snow-cli script run --code 'gs.print(\"done\")' --rollback --quota-managed-transaction\n\nNotes:\n  - `script run` executes a background script directly against the ServiceNow instance.\n  - If you are using SN-Utils, the `snu` command family is for browser/session helper actions, not background scripts.";
// --- Script ---

#[derive(Args, Debug)]
pub struct ScriptArgs {
    #[command(subcommand)]
    pub command: ScriptCommands,
}

#[derive(Subcommand, Debug)]
pub enum ScriptCommands {
    /// Execute a background script on the ServiceNow instance
    #[command(after_help = SCRIPT_RUN_AFTER_HELP)]
    Run {
        /// Path to a script file to execute
        #[arg(long, short = 'f', group = "script_source")]
        file: Option<String>,

        /// Inline script code to execute
        #[arg(long, short = 'c', group = "script_source")]
        code: Option<String>,

        /// Scope in which to run the script (e.g., global, x_myapp)
        #[arg(long, default_value = "global")]
        scope: String,

        /// Endpoint to execute script against (defaults to /sys.scripts.do)
        #[arg(long, default_value = "/sys.scripts.do")]
        endpoint: String,

        /// Record rollback context for database changes
        #[arg(long)]
        rollback: bool,

        /// Run in sandbox mode to prevent database writes
        #[arg(long)]
        sandbox: bool,

        /// Run as scriptlet with access to global server-side objects
        #[arg(long)]
        scriptlet: bool,

        /// Use managed transaction limits (up to 4 hours)
        #[arg(long = "quota-managed-transaction", alias = "limit-transaction")]
        quota_managed_transaction: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Cli, Commands};
    use clap::{CommandFactory, Parser};

    #[test]
    fn test_script_run_help_includes_examples() {
        let help = Cli::command()
            .find_subcommand_mut("script")
            .unwrap()
            .find_subcommand_mut("run")
            .unwrap()
            .render_long_help()
            .to_string();
        assert!(help.contains("snow-cli script run --code 'gs.info(\"hello from snow-cli\")'"));
        assert!(help.contains("background script directly against the ServiceNow instance"));
    }

    #[test]
    fn test_parse_script_run_flags() {
        let cli = Cli::parse_from([
            "snow-cli",
            "script",
            "run",
            "--code",
            "gs.info('x')",
            "--rollback",
            "--sandbox",
            "--scriptlet",
            "--quota-managed-transaction",
        ]);

        match cli.command {
            Commands::Script(args) => match args.command {
                ScriptCommands::Run {
                    rollback,
                    sandbox,
                    scriptlet,
                    quota_managed_transaction,
                    ..
                } => {
                    assert!(rollback);
                    assert!(sandbox);
                    assert!(scriptlet);
                    assert!(quota_managed_transaction);
                }
            },
            _ => panic!("Expected Script command"),
        }
    }
}
