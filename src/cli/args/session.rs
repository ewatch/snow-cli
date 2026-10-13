use crate::models::session::SessionSwitchType;
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
#[command(
    after_help = "Native sessions use ServiceNow's own UI endpoints and OS-keychain credentials; SN-Utils is not required. Use a dedicated native_session profile for subsequent session, table, and API commands. UI endpoints are release-dependent; unsupported instances fail explicitly."
)]
pub struct SessionArgs {
    #[command(subcommand)]
    pub command: SessionCommands,
}
#[derive(Subcommand, Debug)]
pub enum SessionCommands {
    /// Log in using a Basic profile and save a separate native session profile
    Login {
        /// Name of the dedicated native session profile
        name: String,
        /// Read the password from stdin instead of the profile's keychain
        #[arg(long)]
        password_stdin: bool,
    },
    /// Validate and securely store an existing cookie session in a separate profile
    Attach {
        name: String,
        /// Read the full Cookie header from stdin (never passed as a CLI argument)
        #[arg(long, required = true)]
        cookie_stdin: bool,
    },
    /// Inspect the current native session and all three picker contexts
    Show,
    /// List selectable context values for this session
    Options {
        #[arg(value_enum)]
        kind: SessionSwitchType,
    },
    /// Switch a picker value and verify all resulting contexts in the same session
    Switch {
        #[arg(value_enum)]
        kind: SessionSwitchType,
        /// Picker ID; application also accepts an accessible scope name
        value: String,
    },
    /// Remove local session credentials; does not log out or alter the remote session
    Forget,
}
