use clap::{Args, Subcommand};

use super::OutputFormat;

const PROFILE_AFTER_HELP: &str = "Examples:\n  snow-cli profile add dev --instance https://dev.service-now.com --auth-method basic --username admin\n  snow-cli profile edit dev --username new-admin\n  snow-cli profile add prod --instance https://prod.service-now.com --auth-method oauth2 --client-id abc123\n  snow-cli profile default prod\n  snow-cli profile current\n  snow-cli profile remove old-dev\n  snow-cli profile list\n  snow-cli profile find --instance dev123466\n  snow-cli profile storage status\n  snow-cli profile storage set file\n  snow-cli profile storage migrate --delete-legacy\n  snow-cli profile sdk list\n  snow-cli profile sdk import --alias dev\n  snow-cli profile sdk export prod --alias prod-sdk\n  snow-cli profile show";

pub(crate) const READ_ONLY_PROFILE_AFTER_HELP: &str = "Examples:\n  snow-cli-ro profile add dev --instance https://dev.service-now.com --auth-method basic --username admin\n  snow-cli-ro profile edit dev --username new-admin\n  snow-cli-ro profile add prod --instance https://prod.service-now.com --auth-method oauth2 --client-id abc123\n  snow-cli-ro profile default prod\n  snow-cli-ro profile current\n  snow-cli-ro profile remove old-dev\n  snow-cli-ro profile list\n  snow-cli-ro profile find --instance dev123466\n  snow-cli-ro profile storage status\n  snow-cli-ro profile sdk list\n  snow-cli-ro profile sdk import --alias dev\n  snow-cli-ro profile show";

pub(crate) const PROFILE_INIT_AFTER_HELP: &str = "Notes:\n  - This legacy command is non-interactive by default (safe for agents and CI).\n  - Prefer `snow-cli profile add <name>` for new profiles.\n  - Pass required values as flags.\n\nExamples:\n  snow-cli profile add default --instance https://mycompany.service-now.com --auth-method basic --username admin\n  snow-cli profile init --instance https://mycompany.service-now.com --auth-method basic --username admin\n  snow-cli profile init --name prod --instance https://prod.service-now.com --auth-method oauth2 --oauth-grant-type client-credentials --client-id abc123";
// --- Profile ---

#[derive(Args, Debug)]
#[command(after_help = PROFILE_AFTER_HELP)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommands,
}

#[derive(Args, Debug)]
pub struct ProfileInitArgs {
    /// Instance URL (e.g., https://mycompany.service-now.com)
    #[arg(long)]
    pub instance: Option<String>,

    /// Authentication method
    #[arg(long, value_enum)]
    pub auth_method: Option<CliAuthMethod>,

    /// Username (for basic auth or OAuth2 password grant)
    #[arg(long)]
    pub username: Option<String>,

    /// OAuth client ID (for oauth2)
    #[arg(long)]
    pub client_id: Option<String>,

    /// OAuth grant type (for oauth2 auth method)
    #[arg(long, value_enum)]
    pub oauth_grant_type: Option<CliOAuthGrantType>,

    /// OAuth scopes requested during authorization-code login (default: useraccount)
    #[arg(long)]
    pub oauth_scope: Option<String>,

    /// Host used in the OAuth authorization-code local redirect URI
    #[arg(long)]
    pub oauth_redirect_host: Option<String>,

    /// Port used in the OAuth authorization-code local redirect URI
    #[arg(long)]
    pub oauth_redirect_port: Option<u16>,

    /// Path used in the OAuth authorization-code local redirect URI
    #[arg(long)]
    pub oauth_redirect_path: Option<String>,

    /// Profile name to create (defaults to "default")
    #[arg(long, default_value = "default")]
    pub name: String,
}

#[derive(Args, Debug)]
pub struct ProfileUpsertArgs {
    /// Profile name
    pub name: String,

    /// Instance URL (e.g., https://mycompany.service-now.com)
    #[arg(long)]
    pub instance: Option<String>,

    /// Authentication method
    #[arg(long, value_enum)]
    pub auth_method: Option<CliAuthMethod>,

    /// Username (for basic auth or OAuth2 password grant)
    #[arg(long)]
    pub username: Option<String>,

    /// OAuth client ID (for oauth2)
    #[arg(long)]
    pub client_id: Option<String>,

    /// OAuth grant type (for oauth2 auth method)
    #[arg(long, value_enum)]
    pub oauth_grant_type: Option<CliOAuthGrantType>,

    /// OAuth scopes requested during authorization-code login (default: useraccount)
    #[arg(long)]
    pub oauth_scope: Option<String>,

    /// Host used in the OAuth authorization-code local redirect URI
    #[arg(long)]
    pub oauth_redirect_host: Option<String>,

    /// Port used in the OAuth authorization-code local redirect URI
    #[arg(long)]
    pub oauth_redirect_port: Option<u16>,

    /// Path used in the OAuth authorization-code local redirect URI
    #[arg(long)]
    pub oauth_redirect_path: Option<String>,

    /// Path to client certificate (for mTLS)
    #[arg(long)]
    pub cert_path: Option<String>,

    /// Path to client key (for mTLS)
    #[arg(long)]
    pub key_path: Option<String>,

    /// Browser entry point for SSO/SAML login
    #[arg(long)]
    pub sso_login_url: Option<String>,
}

#[derive(Args, Debug)]
pub struct ProfileFindArgs {
    /// Instance name, host, or URL (e.g., dev123466, dev123466.service-now.com, https://dev123466.service-now.com)
    #[arg(long)]
    pub instance: String,
}

#[derive(Args, Debug)]
pub struct ProfileSdkImportArgs {
    /// Import a single now-sdk alias
    #[arg(long)]
    pub alias: Option<String>,

    /// Import all saved now-sdk aliases
    #[arg(long)]
    pub all: bool,

    /// Set the imported profile as the snow-cli default
    #[arg(long)]
    pub set_default: bool,
}

#[derive(Args, Debug)]
pub struct ProfileUseArgs {
    /// Profile name to make the default
    pub name: String,
}

#[derive(Args, Debug)]
pub struct ProfileDeleteArgs {
    /// Profile name to delete
    pub name: String,

    /// Confirm deleting the current default profile
    #[arg(long)]
    pub yes: bool,

    /// New default profile to set when deleting the current default profile
    #[arg(long)]
    pub new_default: Option<String>,
}

#[derive(Args, Debug)]
pub struct ProfileOutputArgs {
    /// Format to persist as the default (json, csv, jsonl, toon, text, auto).
    /// Omit to show the current setting.
    #[arg(value_enum)]
    pub format: Option<OutputFormat>,

    /// Clear the configured default, reverting to the built-in fallback (json)
    #[arg(long, conflicts_with = "format")]
    pub reset: bool,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommands {
    /// First-time setup (legacy; prefer `profile add <name>`)
    #[command(after_help = PROFILE_INIT_AFTER_HELP, hide = true)]
    Init(ProfileInitArgs),

    /// Create a new profile
    Add(ProfileUpsertArgs),

    /// Edit an existing profile
    Edit(ProfileUpsertArgs),

    /// Create or update a named profile (legacy upsert)
    #[command(name = "set", aliases = ["set-profile"], hide = true)]
    SetProfile(ProfileUpsertArgs),

    /// List all configured profiles
    #[command(name = "list", alias = "list-profiles")]
    ListProfiles,

    /// Find configured profiles for a ServiceNow instance name, host, or URL
    #[command(name = "find", alias = "find-profile")]
    FindProfile(ProfileFindArgs),

    /// Interoperate with now-sdk authentication aliases
    Sdk(ProfileSdkArgs),

    /// Inspect or configure encrypted credential vault storage
    Storage(ProfileStorageArgs),

    /// List saved now-sdk authentication aliases
    #[command(name = "list-now-sdk-profiles", hide = true)]
    ListNowSdkProfiles,

    /// Import saved now-sdk aliases into snow-cli profiles
    #[command(name = "import-now-sdk", hide = true)]
    ImportNowSdk(ProfileSdkImportArgs),

    /// Export a basic snow-cli profile into the now-sdk alias store
    #[command(name = "export-now-sdk", hide = true)]
    ExportNowSdk {
        /// snow-cli profile name to export
        // A distinct id keeps the global `--profile` flag usable here.
        #[arg(id = "export_profile", value_name = "PROFILE")]
        profile: String,

        /// Override the destination now-sdk alias name
        #[arg(long)]
        alias: Option<String>,

        /// Set the exported alias as the now-sdk default
        #[arg(long)]
        set_default: bool,
    },

    /// Set the default profile used when --profile is not provided
    #[command(name = "default", aliases = ["use", "use-profile"])]
    UseProfile(ProfileUseArgs),

    /// Show the currently selected profile
    Current,

    /// Show the current active profile configuration
    Show,

    /// Remove a named profile
    #[command(name = "remove", aliases = ["delete", "delete-profile"])]
    DeleteProfile(ProfileDeleteArgs),

    /// Show or set the default output format used when --output is not passed
    Output(ProfileOutputArgs),
}

#[derive(Args, Debug)]
pub struct ProfileSdkArgs {
    #[command(subcommand)]
    pub command: ProfileSdkCommands,
}

#[derive(Subcommand, Debug)]
pub enum ProfileSdkCommands {
    /// List saved now-sdk authentication aliases
    List,

    /// Import saved now-sdk aliases into snow-cli profiles
    Import(ProfileSdkImportArgs),

    /// Export a basic snow-cli profile into the now-sdk alias store
    Export {
        /// snow-cli profile name to export
        // A distinct id keeps the global `--profile` flag usable here.
        #[arg(id = "export_profile", value_name = "PROFILE")]
        profile: String,

        /// Override the destination now-sdk alias name
        #[arg(long)]
        alias: Option<String>,

        /// Set the exported alias as the now-sdk default
        #[arg(long)]
        set_default: bool,
    },
}

#[derive(Args, Debug)]
pub struct ProfileStorageArgs {
    #[command(subcommand)]
    pub command: ProfileStorageCommands,
}

#[derive(Subcommand, Debug)]
pub enum ProfileStorageCommands {
    /// Show the active credential storage mode and vault status (never prompts keychain)
    Status,

    /// Switch credential storage mode (migrating the vault key automatically)
    Set {
        /// Target storage mode
        #[arg(value_enum)]
        mode: CliCredentialStoreMode,

        /// Keep the previous master key in its old location instead of removing it
        #[arg(long)]
        keep_old_key: bool,
    },

    /// Copy legacy per-profile OS keychain credentials into the encrypted vault
    Migrate {
        /// Overwrite entries that already exist in the encrypted vault
        #[arg(long)]
        overwrite: bool,

        /// Delete migrated legacy per-profile keychain items after vault write succeeds
        #[arg(long)]
        delete_legacy: bool,
    },
}

/// Credential storage mode for CLI argument parsing.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum CliCredentialStoreMode {
    /// Encrypted vault with a single master key in the OS keychain (default)
    Keyring,
    /// Encrypted vault with an owner-only key file (zero OS keychain popups)
    File,
    /// Encrypted vault keyed by SNOW_CLI_ENCRYPTION_KEY (CI / headless)
    Env,
    /// Legacy per-profile OS keychain entries
    LegacyKeychain,
}

/// Authentication method for CLI argument parsing.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum CliAuthMethod {
    Basic,
    Oauth2,
    ApiKey,
    Mtls,
    /// Browser session token — cookie provided via SNOW_SESSION_COOKIE env var at runtime.
    /// Accepts the legacy alias `saml` for backward compatibility.
    #[value(name = "browser-session", alias = "saml")]
    BrowserSession,
}

/// OAuth 2.0 grant type for CLI argument parsing.
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum CliOAuthGrantType {
    ClientCredentials,
    Password,
    AuthorizationCode,
}
