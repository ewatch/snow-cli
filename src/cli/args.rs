use clap::{Args, Parser, Subcommand};
use clap_complete::Shell;

const TOP_LEVEL_AFTER_HELP: &str = "Common workflows:\n  1) First-time setup\n     snow-cli profile add default --instance https://mycompany.service-now.com --auth-method basic --username admin\n\n  2) Store credentials\n     snow-cli auth login\n\n  3) List recent incidents\n     snow-cli table list incident --query 'active=true' --limit 20\n\n  4) Create and update records\n     snow-cli table create incident --data '{\"short_description\":\"Disk alert\"}'\n     snow-cli table update incident <sys_id> --data '{\"state\":\"2\"}'\n\n  5) Call a custom API\n     snow-cli api get /api/x_myapp/status";

const PROFILE_AFTER_HELP: &str = "Examples:\n  snow-cli profile add dev --instance https://dev.service-now.com --auth-method basic --username admin\n  snow-cli profile edit dev --username new-admin\n  snow-cli profile add prod --instance https://prod.service-now.com --auth-method oauth2 --client-id abc123\n  snow-cli profile default prod\n  snow-cli profile current\n  snow-cli profile remove old-dev\n  snow-cli profile list\n  snow-cli profile find --instance dev123466\n  snow-cli profile sdk list\n  snow-cli profile sdk import --alias dev\n  snow-cli profile sdk export prod --alias prod-sdk\n  snow-cli profile show";

const PROFILE_INIT_AFTER_HELP: &str = "Notes:\n  - This legacy command is non-interactive by default (safe for agents and CI).\n  - Prefer `snow-cli profile add <name>` for new profiles.\n  - Pass required values as flags.\n\nExamples:\n  snow-cli profile add default --instance https://mycompany.service-now.com --auth-method basic --username admin\n  snow-cli profile init --instance https://mycompany.service-now.com --auth-method basic --username admin\n  snow-cli profile init --name prod --instance https://prod.service-now.com --auth-method oauth2 --oauth-grant-type client-credentials --client-id abc123";

const AUTH_AFTER_HELP: &str = "Examples:\n  snow-cli auth login\n  printf '%s' \"$SNOW_PASSWORD\" | snow-cli auth login --password-stdin\n  snow-cli auth status\n  snow-cli auth token\n  snow-cli auth logout";

const AUTH_LOGIN_AFTER_HELP: &str = "Examples:\n  snow-cli auth login\n  printf '%s' \"$SNOW_PASSWORD\" | snow-cli auth login --password-stdin\n  printf '%s' \"$SNOW_API_TOKEN\" | snow-cli auth login --token-stdin\n  printf '%s' \"$SNOW_CLIENT_SECRET\" | snow-cli auth login --client-secret-stdin\n  snow-cli auth login --no-browser\n  snow-cli auth login --session-cookie 'JSESSIONID=...; glide_user_route=...'\n\nTip:\n  Prefer interactive prompts or --*-stdin flags over command-line secret flags, which can leak through shell history and process listings.\n  If a required secret flag is omitted and stdin is a TTY, you will be prompted securely.\n  For OAuth2 authorization-code profiles, snow-cli opens the authorization URL and waits for a local redirect callback. Public PKCE clients can omit --client-secret.\n  For browser-session profiles, provide the full Cookie header value from your authenticated browser session via --session-cookie or the SNOW_SESSION_COOKIE environment variable. The token is not stored.";

const TABLE_AFTER_HELP: &str = "Examples:\n  snow-cli table list incident --query 'active=true' --limit 10\n  snow-cli table get incident <sys_id>\n  snow-cli table create incident --data '{\"short_description\":\"Disk alert\"}'\n  snow-cli table update incident <sys_id> --data '{\"state\":\"2\"}'\n  snow-cli table schema incident --extended";

const DATA_AFTER_HELP: &str = "Examples:\n  snow-cli data export incident --query 'active=true'\n  snow-cli data export sys_user --fields sys_id,user_name,email --out users.json\n  snow-cli data export-package --file dataset-spec.json --out-dir exported-dataset\n  snow-cli data validate --file export.json\n  snow-cli data import --file export.json\n  snow-cli data import --file users.json --import-set-table imp_user";

const DATA_EXPORT_AFTER_HELP: &str = "Examples:\n  snow-cli data export incident --query 'active=true'\n  snow-cli data export incident --fields sys_id,number,short_description --limit 50\n  snow-cli --output csv data export sys_user --fields sys_id,user_name,email --out users.csv";

const DATA_EXPORT_PACKAGE_AFTER_HELP: &str = "Examples:\n  snow-cli data export-package --file dataset-spec.json --out-dir exported-dataset\n  snow-cli data validate --file exported-dataset/manifest.json\n  snow-cli data import --file exported-dataset/manifest.json";

const IMPORT_SET_AFTER_HELP: &str = "Examples:\n  snow-cli import-set load imp_user --data '{\"user_name\":\"snow-cli-user\",\"email\":\"snow-cli-user@example.com\"}'\n  echo '{\"user_name\":\"stdin-user\",\"email\":\"stdin-user@example.com\"}' | snow-cli import-set load imp_user\n  snow-cli import-set load imp_user --fail-on-error --data '{\"user_name\":\"ci-user\",\"email\":\"ci-user@example.com\"}'\n\nNotes:\n  - `import-set load` posts to /api/now/import/{table}.\n  - On the validated `sprint` instance, this endpoint also ran the transform map automatically for `imp_user`.\n  - Use `--fail-on-error` when row-level transform errors should make the command exit non-zero.\n  - `import-set transform` remains a placeholder until a supported separate transform trigger is implemented.";

const SEED_AFTER_HELP: &str = "Examples:\n  snow-cli seed plan --file qa-fixture.json\n  snow-cli seed apply --file qa-fixture.json\n  snow-cli seed cleanup <run-id> --dry-run";

const SCOPE_AFTER_HELP: &str = "Examples:\n  snow-cli scope list\n  snow-cli scope list incident\n  snow-cli scope list sn_ot_incident_mgmt\n  snow-cli scope inspect x_my_app\n  snow-cli scope inspect 4f7f9bfe1b2a9010d9f2ed7c2e4bcb12 --details full\n  snow-cli scope move-file sys_script_include 4f7f9bfe1b2a9010d9f2ed7c2e4bcb12 --target-scope x_target_app --dry-run\n  snow-cli scope move-file sys_script_include 4f7f9bfe1b2a9010d9f2ed7c2e4bcb12 --target-scope x_target_app --yes";

const TABLE_LIST_AFTER_HELP: &str = "Examples:\n  snow-cli table list incident --query 'active=true' --limit 20\n  snow-cli table list sys_user --fields sys_id,user_name,email --order-by user_name";

const TABLE_CREATE_AFTER_HELP: &str = "Examples:\n  snow-cli table create incident --data '{\"short_description\":\"VPN down\"}'\n  echo '{\"short_description\":\"From stdin\"}' | snow-cli table create incident";

const API_AFTER_HELP: &str = "Examples:\n  snow-cli api get /api/now/table/incident?sysparm_limit=1\n  snow-cli api post /api/x_myapp/action --data '{\"dry_run\":true}'\n  snow-cli api get /api/x_myapp/status -H 'X-Trace-Id:abc123'";

const SCRIPT_RUN_AFTER_HELP: &str = "Examples:\n  snow-cli script run --code 'gs.info(\"hello from snow-cli\")'\n  snow-cli script run --file ./cleanup.js --sandbox\n  printf '%s' 'gs.info(\"from stdin\")' | snow-cli script run --scope x_my_app\n  snow-cli script run --code 'gs.print(\"done\")' --rollback --quota-managed-transaction\n\nNotes:\n  - `script run` executes a background script directly against the ServiceNow instance.\n  - If you are using SN-Utils, the `snu` command family is for browser/session helper actions, not background scripts.";

const SNU_AFTER_HELP: &str = "Examples:\n  snow-cli snu check-connection\n  snow-cli snu get-instance-info\n  snow-cli snu list-tables\n  snow-cli snu create-record incident --data '{\"short_description\":\"Created via snu\"}'\n  snow-cli snu app-meta x_my_app\n  snow-cli snu get-record incident 46d44a1b1b223010d9f2ed7c2e4bcb1 --fields sys_id,number,short_description\n  snow-cli snu update-record sys_script_include 46d44a1b1b223010d9f2ed7c2e4bcb1 --field script --content 'gs.info(\"hello\")'\n  snow-cli snu update-record sp_widget 46d44a1b1b223010d9f2ed7c2e4bcb1 --data '{\"script\":\"data.hello = \\\"world\\\";\",\"css\":\".c1 { color: red; }\"}'\n  snow-cli snu delete-record incident 46d44a1b1b223010d9f2ed7c2e4bcb1\n  snow-cli snu wait-token\n  snow-cli snu query incident --query 'active=true' --fields sys_id,number --limit 10\n  snow-cli snu schema incident\n  snow-cli snu execute-bg-script --code 'gs.info(\"hello from SN-Utils\")'\n  snow-cli snu slash /tn\n  snow-cli snu tab activate 'https://dev12345.service-now.com/incident.do*' --open-if-not-found\n  snow-cli snu context switch application x_my_app --tab-url 'https://dev12345.service-now.com/*'\n  snow-cli snu screenshot --url 'https://dev12345.service-now.com/*' --out incident.png\n  snow-cli snu attachment-upload incident <sys_id> --file ./attachment.png\n  snow-cli --instance https://dev12345.service-now.com snu query incident --query 'active=true'\n  snow-cli snu broker status\n  snow-cli snu broker clear --instance https://dev12345.service-now.com\n\nNotes:\n  - SN-Utils must be installed in the browser and the SN-Utils ScriptSync helper tab must be open. Commands auto-start a local broker that owns the SN-Utils WebSocket port and waits for helper/session metadata as needed; run /token in a ServiceNow tab if prompted.\n  - When the SN-Utils tab is a portal to several instances, pass the global `--instance <url-or-host>` to pick which instance's browser session is used; without it, commands target the most recently active instance. `snu broker status` lists every instance that currently has a live g_ck.\n  - `snu broker clear [--instance <url>]` drops cached browser sessions from broker memory (all instances, or just one); the next command re-prompts for /token.\n  - The g_ck token is treated as live browser-session metadata only. snow-cli keeps it in broker memory per instance while the broker is running, but does not store it in the OS keychain or use it as a standalone reusable credential.\n  - `script run` is the direct background-script command; `snu execute-bg-script` runs the same kind of server-side script through the browser helper tab.\n  - `snu check-connection` and `snu get-instance-info` are lightweight diagnostics for the websocket bridge and browser session.\n  - `snu list-tables`, `snu get-record`, `snu create-record`, `snu update-record`, `snu delete-record`, and `snu app-meta` map to SN-Utils helper/browser-session actions.\n  - The broker binds 127.0.0.1:1978, the port hard-coded by SN-Utils scriptsync.html, and shuts down after an idle timeout. Use `snu broker status` or `snu broker stop` if something unexpected happens. Stop sn-scriptsync first if it owns that port.";
pub const DEFAULT_SNU_TIMEOUT_SECS: u64 = 180;
/// Default sysparm_fields used by snu record reads (query, get-record).
pub const DEFAULT_SNU_FIELDS: &str = "sys_id,number,short_description";

/// ❄️ snow-cli — ServiceNow CLI for humans and coding agents
#[derive(Parser, Debug)]
#[command(
    name = "snow-cli",
    version,
    about = "❄️ snow-cli — CLI gateway for LLMs and coding agents to access ServiceNow instances",
    long_about = None,
    after_help = TOP_LEVEL_AFTER_HELP
)]
pub struct Cli {
    /// ServiceNow profile to use
    #[arg(long, global = true)]
    pub profile: Option<String>,

    /// Override the ServiceNow instance URL
    #[arg(long, global = true)]
    pub instance: Option<String>,

    /// Output format
    #[arg(long, alias = "format", global = true, default_value = "json")]
    pub output: OutputFormat,

    /// Override the HTTP request timeout in seconds
    #[arg(long, global = true)]
    pub timeout_secs: Option<u64>,

    /// Block commands and HTTP methods that can mutate ServiceNow
    #[arg(long, global = true)]
    pub read_only: bool,

    /// Increase verbosity (-v info, -vv debug, -vvv trace)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum OutputFormat {
    Json,
    Csv,
    Jsonl,
    Toon,
    Text,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Manage ServiceNow connection profiles
    #[command(alias = "config")]
    Profile(ConfigArgs),

    /// Authentication operations
    Auth(AuthArgs),

    /// Table API operations (CRUD on any ServiceNow table)
    Table(TableArgs),

    /// Portable data export, validation, and import workflows
    Data(DataArgs),

    /// Declarative test-data planning, apply, and cleanup workflows
    Seed(SeedArgs),

    /// Analyze application scope metadata
    Scope(ScopeArgs),

    /// Attachment operations
    Attachment(AttachmentArgs),

    /// Import set operations
    ImportSet(ImportSetArgs),

    /// Raw REST API calls to any endpoint
    Api(ApiArgs),

    /// Execute background scripts on ServiceNow
    Script(ScriptArgs),

    /// Search code across ServiceNow instance (scripts, business rules, etc.)
    Codesearch(CodesearchArgs),

    /// Use the SN-Utils browser extension helper tab directly (without sn-scriptsync)
    Snu(SnuArgs),

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

// --- Profile ---

#[derive(Args, Debug)]
#[command(after_help = PROFILE_AFTER_HELP)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommands,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommands {
    /// First-time setup (legacy; prefer `profile add <name>`)
    #[command(after_help = PROFILE_INIT_AFTER_HELP, hide = true)]
    Init {
        /// Instance URL (e.g., https://mycompany.service-now.com)
        #[arg(long)]
        instance: Option<String>,

        /// Authentication method
        #[arg(long, value_enum)]
        auth_method: Option<CliAuthMethod>,

        /// Username (for basic auth or OAuth2 password grant)
        #[arg(long)]
        username: Option<String>,

        /// OAuth client ID (for oauth2)
        #[arg(long)]
        client_id: Option<String>,

        /// OAuth grant type (for oauth2 auth method)
        #[arg(long, value_enum)]
        oauth_grant_type: Option<CliOAuthGrantType>,

        /// OAuth scopes requested during authorization-code login (default: useraccount)
        #[arg(long)]
        oauth_scope: Option<String>,

        /// Host used in the OAuth authorization-code local redirect URI
        #[arg(long)]
        oauth_redirect_host: Option<String>,

        /// Port used in the OAuth authorization-code local redirect URI
        #[arg(long)]
        oauth_redirect_port: Option<u16>,

        /// Path used in the OAuth authorization-code local redirect URI
        #[arg(long)]
        oauth_redirect_path: Option<String>,

        /// Profile name to create (defaults to "default")
        #[arg(long, default_value = "default")]
        name: String,
    },

    /// Create a new profile
    Add {
        /// Profile name
        name: String,

        /// Instance URL (e.g., https://mycompany.service-now.com)
        #[arg(long)]
        instance: Option<String>,

        /// Authentication method
        #[arg(long, value_enum)]
        auth_method: Option<CliAuthMethod>,

        /// Username (for basic auth or OAuth2 password grant)
        #[arg(long)]
        username: Option<String>,

        /// OAuth client ID (for oauth2)
        #[arg(long)]
        client_id: Option<String>,

        /// OAuth grant type (for oauth2 auth method)
        #[arg(long, value_enum)]
        oauth_grant_type: Option<CliOAuthGrantType>,

        /// OAuth scopes requested during authorization-code login (default: useraccount)
        #[arg(long)]
        oauth_scope: Option<String>,

        /// Host used in the OAuth authorization-code local redirect URI
        #[arg(long)]
        oauth_redirect_host: Option<String>,

        /// Port used in the OAuth authorization-code local redirect URI
        #[arg(long)]
        oauth_redirect_port: Option<u16>,

        /// Path used in the OAuth authorization-code local redirect URI
        #[arg(long)]
        oauth_redirect_path: Option<String>,

        /// Path to client certificate (for mTLS)
        #[arg(long)]
        cert_path: Option<String>,

        /// Path to client key (for mTLS)
        #[arg(long)]
        key_path: Option<String>,

        /// Browser entry point for SSO/SAML login
        #[arg(long)]
        sso_login_url: Option<String>,
    },

    /// Edit an existing profile
    Edit {
        /// Profile name
        name: String,

        /// Instance URL (e.g., https://mycompany.service-now.com)
        #[arg(long)]
        instance: Option<String>,

        /// Authentication method
        #[arg(long, value_enum)]
        auth_method: Option<CliAuthMethod>,

        /// Username (for basic auth or OAuth2 password grant)
        #[arg(long)]
        username: Option<String>,

        /// OAuth client ID (for oauth2)
        #[arg(long)]
        client_id: Option<String>,

        /// OAuth grant type (for oauth2 auth method)
        #[arg(long, value_enum)]
        oauth_grant_type: Option<CliOAuthGrantType>,

        /// OAuth scopes requested during authorization-code login (default: useraccount)
        #[arg(long)]
        oauth_scope: Option<String>,

        /// Host used in the OAuth authorization-code local redirect URI
        #[arg(long)]
        oauth_redirect_host: Option<String>,

        /// Port used in the OAuth authorization-code local redirect URI
        #[arg(long)]
        oauth_redirect_port: Option<u16>,

        /// Path used in the OAuth authorization-code local redirect URI
        #[arg(long)]
        oauth_redirect_path: Option<String>,

        /// Path to client certificate (for mTLS)
        #[arg(long)]
        cert_path: Option<String>,

        /// Path to client key (for mTLS)
        #[arg(long)]
        key_path: Option<String>,

        /// Browser entry point for SSO/SAML login
        #[arg(long)]
        sso_login_url: Option<String>,
    },

    /// Create or update a named profile (legacy upsert)
    #[command(name = "set", aliases = ["set-profile"], hide = true)]
    SetProfile {
        /// Profile name
        name: String,

        /// Instance URL (e.g., https://mycompany.service-now.com)
        #[arg(long)]
        instance: Option<String>,

        /// Authentication method
        #[arg(long, value_enum)]
        auth_method: Option<CliAuthMethod>,

        /// Username (for basic auth or OAuth2 password grant)
        #[arg(long)]
        username: Option<String>,

        /// OAuth client ID (for oauth2)
        #[arg(long)]
        client_id: Option<String>,

        /// OAuth grant type (for oauth2 auth method)
        #[arg(long, value_enum)]
        oauth_grant_type: Option<CliOAuthGrantType>,

        /// OAuth scopes requested during authorization-code login (default: useraccount)
        #[arg(long)]
        oauth_scope: Option<String>,

        /// Host used in the OAuth authorization-code local redirect URI
        #[arg(long)]
        oauth_redirect_host: Option<String>,

        /// Port used in the OAuth authorization-code local redirect URI
        #[arg(long)]
        oauth_redirect_port: Option<u16>,

        /// Path used in the OAuth authorization-code local redirect URI
        #[arg(long)]
        oauth_redirect_path: Option<String>,

        /// Path to client certificate (for mTLS)
        #[arg(long)]
        cert_path: Option<String>,

        /// Path to client key (for mTLS)
        #[arg(long)]
        key_path: Option<String>,

        /// Browser entry point for SSO/SAML login
        #[arg(long)]
        sso_login_url: Option<String>,
    },

    /// List all configured profiles
    #[command(name = "list", alias = "list-profiles")]
    ListProfiles,

    /// Find configured profiles for a ServiceNow instance name, host, or URL
    #[command(name = "find", alias = "find-profile")]
    FindProfile {
        /// Instance name, host, or URL (e.g., dev123466, dev123466.service-now.com, https://dev123466.service-now.com)
        #[arg(long)]
        instance: String,
    },

    /// Interoperate with now-sdk authentication aliases
    Sdk(ProfileSdkArgs),

    /// List saved now-sdk authentication aliases
    #[command(name = "list-now-sdk-profiles", hide = true)]
    ListNowSdkProfiles,

    /// Import saved now-sdk aliases into snow-cli profiles
    #[command(name = "import-now-sdk", hide = true)]
    ImportNowSdk {
        /// Import a single now-sdk alias
        #[arg(long)]
        alias: Option<String>,

        /// Import all saved now-sdk aliases
        #[arg(long)]
        all: bool,

        /// Set the imported profile as the snow-cli default
        #[arg(long)]
        set_default: bool,
    },

    /// Export a basic snow-cli profile into the now-sdk alias store
    #[command(name = "export-now-sdk", hide = true)]
    ExportNowSdk {
        /// snow-cli profile name to export
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
    UseProfile {
        /// Profile name to make the default
        name: String,
    },

    /// Show the currently selected profile
    Current,

    /// Show the current active profile configuration
    Show,

    /// Remove a named profile
    #[command(name = "remove", aliases = ["delete", "delete-profile"])]
    DeleteProfile {
        /// Profile name to delete
        name: String,

        /// Confirm deleting the current default profile
        #[arg(long)]
        yes: bool,

        /// New default profile to set when deleting the current default profile
        #[arg(long)]
        new_default: Option<String>,
    },
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
    Import {
        /// Import a single now-sdk alias
        #[arg(long)]
        alias: Option<String>,

        /// Import all saved now-sdk aliases
        #[arg(long)]
        all: bool,

        /// Set the imported profile as the snow-cli default
        #[arg(long)]
        set_default: bool,
    },

    /// Export a basic snow-cli profile into the now-sdk alias store
    Export {
        /// snow-cli profile name to export
        profile: String,

        /// Override the destination now-sdk alias name
        #[arg(long)]
        alias: Option<String>,

        /// Set the exported alias as the now-sdk default
        #[arg(long)]
        set_default: bool,
    },
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

// --- Auth ---

#[derive(Args, Debug)]
#[command(after_help = AUTH_AFTER_HELP)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommands,
}

#[derive(Subcommand, Debug)]
pub enum AuthCommands {
    /// Authenticate and store credentials in the OS keychain
    #[command(after_help = AUTH_LOGIN_AFTER_HELP)]
    Login {
        /// Password for basic auth or OAuth2 password grant
        #[arg(long, conflicts_with = "password_stdin")]
        password: Option<String>,

        /// Read password from stdin
        #[arg(long, conflicts_with = "password")]
        password_stdin: bool,

        /// API token (for api_key auth)
        #[arg(long, conflicts_with = "token_stdin")]
        token: Option<String>,

        /// Read API token from stdin
        #[arg(long, conflicts_with = "token")]
        token_stdin: bool,

        /// OAuth client secret (required for client_credentials/password, optional for public authorization-code PKCE clients)
        #[arg(long, conflicts_with = "client_secret_stdin")]
        client_secret: Option<String>,

        /// Read OAuth client secret from stdin
        #[arg(long, conflicts_with = "client_secret")]
        client_secret_stdin: bool,

        /// Full Cookie header value from a browser session (for browser-session auth).
        /// This value is NOT stored; export it as SNOW_SESSION_COOKIE for future requests.
        #[arg(long, conflicts_with = "session_cookie_stdin")]
        session_cookie: Option<String>,

        /// Read the Cookie header value from stdin (for browser-session auth)
        #[arg(long, conflicts_with = "session_cookie")]
        session_cookie_stdin: bool,

        /// Print the OAuth authorization URL instead of opening it in a browser
        #[arg(long)]
        no_browser: bool,

        /// Also write the successful basic login into now-sdk
        #[arg(long)]
        also_now_sdk: bool,

        /// Destination alias name for now-sdk
        #[arg(long, requires = "also_now_sdk")]
        now_sdk_alias: Option<String>,

        /// Mark the now-sdk alias as default
        #[arg(long, requires = "also_now_sdk")]
        set_now_sdk_default: bool,
    },

    /// Clear stored credentials for the active profile
    Logout,

    /// Show current authentication status
    Status,

    /// Print the current access token to stdout (for piping to other tools)
    Token,
}

// --- Table ---

#[derive(Args, Debug)]
#[command(after_help = TABLE_AFTER_HELP)]
pub struct TableArgs {
    #[command(subcommand)]
    pub command: TableCommands,
}

#[derive(Subcommand, Debug)]
pub enum TableCommands {
    /// List records from a table (auto-paginated)
    #[command(after_help = TABLE_LIST_AFTER_HELP)]
    List {
        /// Table name (e.g., incident, sys_user, cmdb_ci)
        table: String,

        /// Encoded query string
        #[arg(long)]
        query: Option<String>,

        /// Comma-separated list of fields to return
        #[arg(long)]
        fields: Option<String>,

        /// Maximum number of records to return
        #[arg(long)]
        limit: Option<usize>,

        /// Field to order results by
        #[arg(long)]
        order_by: Option<String>,
    },

    /// Get a single record by sys_id
    Get {
        /// Table name
        table: String,

        /// Record sys_id
        sys_id: String,

        /// Comma-separated list of fields to return
        #[arg(long)]
        fields: Option<String>,
    },

    /// Create a new record
    #[command(after_help = TABLE_CREATE_AFTER_HELP)]
    Create {
        /// Table name
        table: String,

        /// JSON data for the record
        #[arg(long)]
        data: Option<String>,
    },

    /// Update an existing record
    Update {
        /// Table name
        table: String,

        /// Record sys_id
        sys_id: String,

        /// JSON data for the update
        #[arg(long)]
        data: Option<String>,
    },

    /// Delete a record
    Delete {
        /// Table name
        table: String,

        /// Record sys_id
        sys_id: String,

        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },

    /// Show table schema (columns, types, labels) from sys_dictionary
    Schema {
        /// Table name (e.g., incident, sys_user, cmdb_ci)
        table: String,

        /// Show extended field metadata (required, read-only, max length, default, reference table)
        #[arg(long)]
        extended: bool,

        /// Include fields inherited from parent tables (e.g., incident inherits from task)
        #[arg(long)]
        include_inherited: bool,
    },
}

// --- Scope ---

#[derive(Args, Debug)]
#[command(after_help = DATA_AFTER_HELP)]
pub struct DataArgs {
    #[command(subcommand)]
    pub command: DataCommands,
}

#[derive(Subcommand, Debug)]
pub enum DataCommands {
    /// Export records from a single table
    #[command(after_help = DATA_EXPORT_AFTER_HELP)]
    Export {
        /// Table name (e.g., incident, sys_user, cmdb_ci)
        table: String,

        /// Encoded query string
        #[arg(long)]
        query: Option<String>,

        /// Comma-separated list of fields to return
        #[arg(long)]
        fields: Option<String>,

        /// Maximum number of records to return
        #[arg(long)]
        limit: Option<usize>,

        /// Field to order results by
        #[arg(long)]
        order_by: Option<String>,

        /// Write the exported artifact to a file instead of stdout
        #[arg(long = "out", short = 'o')]
        out_path: Option<String>,
    },

    /// Export a multi-table dataset package from a manifest spec
    #[command(after_help = DATA_EXPORT_PACKAGE_AFTER_HELP)]
    ExportPackage {
        /// Dataset export spec file
        #[arg(long, short = 'f')]
        file: String,

        /// Output directory for manifest and table files
        #[arg(long)]
        out_dir: String,
    },

    /// Validate a dataset file against the target instance
    Validate {
        /// Dataset file to validate
        #[arg(long, short = 'f')]
        file: String,
    },

    /// Import a dataset file into the target instance
    Import {
        /// Dataset file to import
        #[arg(long, short = 'f')]
        file: String,

        /// Preview the import plan without creating records
        #[arg(long)]
        dry_run: bool,

        /// Staging table to use for flat Import Set API loads
        #[arg(long)]
        import_set_table: Option<String>,

        /// Exit non-zero when Import Set API responses contain row-level errors
        #[arg(long)]
        fail_on_error: bool,
    },
}

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

// --- Scope ---

#[derive(Args, Debug)]
#[command(after_help = SCOPE_AFTER_HELP)]
pub struct ScopeArgs {
    #[command(subcommand)]
    pub command: ScopeCommands,
}

#[derive(Subcommand, Debug)]
pub enum ScopeCommands {
    /// List scopes and classify them by origin
    List {
        /// Optional search term for partial name matches or exact scope names
        search: Option<String>,

        /// Restrict results to one or more scope kinds
        #[arg(long, value_enum)]
        kind: Vec<ScopeListKind>,

        /// Include the source table column in text output
        #[arg(long)]
        show_source_table: bool,

        /// Include the sys_id column in text output
        #[arg(long)]
        show_sys_id: bool,
    },

    /// Inspect scope metadata and artifact counts
    Inspect {
        /// Scope name (e.g., x_my_app) or scope sys_id
        scope: String,

        /// Detail level for output payload
        #[arg(long, value_enum, default_value = "basic")]
        details: ScopeDetailLevel,
    },

    /// Export normalized scope artifacts for analysis
    Inventory {
        /// Scope name (e.g., x_my_app) or scope sys_id
        scope: String,
    },

    /// Move one application file to a different custom scope without changing sys_id
    MoveFile {
        /// Source table name for the application file
        table: String,

        /// Source record sys_id
        sys_id: String,

        /// Target scope name (e.g., x_my_app) or scope sys_id
        #[arg(long = "target-scope")]
        target_scope: String,

        /// Validate and preview the move without persisting changes
        #[arg(long)]
        dry_run: bool,

        /// Confirm execution when warnings are reported
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum ScopeDetailLevel {
    Basic,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq, clap::ValueEnum)]
pub enum ScopeListKind {
    StoreApp,
    Plugin,
    CustomApp,
    Platform,
    PlatformApp,
}

impl ScopeListKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::StoreApp => "store_app",
            Self::Plugin => "plugin",
            Self::CustomApp => "custom_app",
            Self::Platform => "platform",
            Self::PlatformApp => "platform_app",
        }
    }
}

// --- Attachment ---

#[derive(Args, Debug)]
pub struct AttachmentArgs {
    #[command(subcommand)]
    pub command: AttachmentCommands,
}

#[derive(Subcommand, Debug)]
pub enum AttachmentCommands {
    /// List attachments for a record
    List {
        /// Table name
        table: String,

        /// Record sys_id
        sys_id: String,
    },

    /// Download an attachment
    Download {
        /// Attachment sys_id
        sys_id: String,

        /// Output file path (defaults to original filename)
        #[arg(long = "out", short = 'o')]
        out_path: Option<String>,
    },

    /// Upload a file as an attachment
    Upload {
        /// Table name
        table: String,

        /// Record sys_id
        sys_id: String,

        /// Path to the file to upload
        #[arg(long, short)]
        file: String,
    },
}

// --- Import Set ---

#[derive(Args, Debug)]
#[command(after_help = IMPORT_SET_AFTER_HELP)]
pub struct ImportSetArgs {
    #[command(subcommand)]
    pub command: ImportSetCommands,
}

#[derive(Subcommand, Debug)]
pub enum ImportSetCommands {
    /// Load data into a staging table
    Load {
        /// Staging table name
        table: String,

        /// JSON data to load
        #[arg(long)]
        data: Option<String>,

        /// Exit non-zero when the import response contains row-level errors
        #[arg(long)]
        fail_on_error: bool,
    },

    /// Transform staged data
    Transform {
        /// Import set sys_id
        sys_id: String,
    },
}

// --- API (raw) ---

#[derive(Args, Debug)]
#[command(after_help = API_AFTER_HELP)]
pub struct ApiArgs {
    #[command(subcommand)]
    pub command: ApiCommands,
}

#[derive(Subcommand, Debug)]
pub enum ApiCommands {
    /// Send a GET request
    Get {
        /// API path (e.g., /api/x_myapp/my_endpoint)
        path: String,

        /// Custom headers (key:value)
        #[arg(long, short = 'H')]
        header: Vec<String>,
    },

    /// Send a POST request
    Post {
        /// API path
        path: String,

        /// JSON request body
        #[arg(long)]
        data: Option<String>,

        /// Custom headers (key:value)
        #[arg(long, short = 'H')]
        header: Vec<String>,
    },

    /// Send a PUT request
    Put {
        /// API path
        path: String,

        /// JSON request body
        #[arg(long)]
        data: Option<String>,

        /// Custom headers (key:value)
        #[arg(long, short = 'H')]
        header: Vec<String>,
    },

    /// Send a DELETE request
    Delete {
        /// API path
        path: String,

        /// Custom headers (key:value)
        #[arg(long, short = 'H')]
        header: Vec<String>,
    },
}

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

// --- SN-Utils bridge ---

#[derive(Args, Debug)]
#[command(after_help = SNU_AFTER_HELP)]
pub struct SnuArgs {
    #[command(subcommand)]
    pub command: SnuCommands,
}

#[derive(Subcommand, Debug)]
pub enum SnuCommands {
    /// Check whether the SN-Utils bridge and browser helper are connected
    CheckConnection {
        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Get SN-Utils bridge instance info
    GetInstanceInfo {
        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Wait for /token from SN-Utils and print the received browser session metadata
    WaitToken {
        /// Seconds to wait for the helper tab and /token message
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Query ServiceNow records through the active SN-Utils browser session
    Query {
        /// Table name
        table: String,

        /// Encoded query
        #[arg(long)]
        query: Option<String>,

        /// Comma-separated sysparm_fields
        #[arg(long, default_value = DEFAULT_SNU_FIELDS)]
        fields: String,

        /// Maximum records to return
        #[arg(long, default_value = "10")]
        limit: u32,

        /// ORDERBY clause or field expression appended to sysparm_query
        #[arg(long)]
        order_by: Option<String>,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Execute a server-side background script through the active SN-Utils browser session
    ExecuteBgScript {
        /// Path to a script file to execute
        #[arg(long, short = 'f', group = "script_source")]
        file: Option<String>,

        /// Inline script code to execute
        #[arg(long, short = 'c', group = "script_source")]
        code: Option<String>,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Create a record through the active SN-Utils browser session
    CreateRecord {
        /// Table name
        table: String,

        /// JSON object of field/value pairs for the new record
        #[arg(long)]
        data: Option<String>,

        /// Application scope for the ACL/transaction context (sysparm_transaction_scope)
        #[arg(long)]
        scope: Option<String>,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Fetch a scoped application's artifacts/metadata (SN-Utils requestAppMeta)
    AppMeta {
        /// Application scope or sys_id (sysparm_transaction_scope)
        app_id: String,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// List available tables through the active SN-Utils browser session
    ListTables {
        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Fetch a single record through the active SN-Utils browser session
    GetRecord {
        /// Table name
        table: String,

        /// Record sys_id
        sys_id: String,

        /// Optional comma-separated fields list
        #[arg(long)]
        fields: Option<String>,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Update one or more fields through the active SN-Utils browser session
    UpdateRecord {
        /// Table name
        table: String,

        /// Record sys_id
        sys_id: String,

        /// JSON object of field/value pairs to update (use for multiple fields)
        #[arg(long, group = "update_source")]
        data: Option<String>,

        /// Single field name to update (use with --content; convenient for large values)
        #[arg(long, group = "update_source", requires = "content")]
        field: Option<String>,

        /// New content for the field named by --field
        #[arg(long, requires = "field")]
        content: Option<String>,

        /// Confirm the updated values by reading them back
        #[arg(long = "await")]
        await_confirmation: bool,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Delete a record through the active SN-Utils browser session
    DeleteRecord {
        /// Table name
        table: String,

        /// Record sys_id (single delete)
        #[arg(long, group = "delete_target")]
        sys_id: Option<String>,

        /// Encoded query (bulk delete)
        #[arg(long, group = "delete_target")]
        query: Option<String>,

        /// Required for bulk delete to confirm destructive intent
        #[arg(long)]
        confirm: bool,

        /// Maximum records to delete in bulk mode
        #[arg(long)]
        limit: Option<u32>,

        /// Preview without deleting
        #[arg(long)]
        dry_run: bool,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Fetch table UI metadata through the active SN-Utils browser session
    Schema {
        /// Table name
        table: String,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Run an SN-Utils slash command in a ServiceNow browser tab
    Slash {
        /// Slash command, e.g. /tn or tn
        command: String,

        /// Browser tab URL pattern to target
        #[arg(long, default_value = "https://*.service-now.com/*")]
        url: String,

        /// Specific browser tab id to target
        #[arg(long)]
        tab_id: Option<u64>,

        /// Insert/show but do not auto-run the slash command
        #[arg(long)]
        no_auto_run: bool,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Browser tab operations through SN-Utils
    Tab(SnuTabArgs),

    /// Switch browser session context through SN-Utils
    Context(SnuContextArgs),

    /// Capture a browser screenshot through SN-Utils
    Screenshot {
        /// Browser tab URL/pattern to capture
        #[arg(long)]
        url: Option<String>,

        /// Specific browser tab id to capture
        #[arg(long)]
        tab_id: Option<u64>,

        /// Output PNG path; defaults to the helper-provided filename
        #[arg(long = "out", short = 'o')]
        out_path: Option<String>,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Upload an attachment through the active SN-Utils browser session
    AttachmentUpload {
        /// Table name
        table: String,

        /// Record sys_id
        sys_id: String,

        /// Path to file to upload
        #[arg(long, short)]
        file: String,

        /// Content type override
        #[arg(long)]
        content_type: Option<String>,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },

    /// Inspect or stop the auto-started SN-Utils broker
    Broker(SnuBrokerArgs),
}

#[derive(Args, Debug)]
pub struct SnuBrokerArgs {
    #[command(subcommand)]
    pub command: SnuBrokerCommands,
}

#[derive(Subcommand, Debug)]
pub enum SnuBrokerCommands {
    /// Show broker status
    Status,

    /// Stop the running broker
    Stop,

    /// Clear cached browser sessions from broker memory
    Clear {
        /// Clear only this instance (URL or host); omit to clear all instances
        #[arg(long)]
        instance: Option<String>,
    },

    /// Run the broker server process
    #[command(hide = true)]
    Serve,
}

#[derive(Args, Debug)]
pub struct SnuTabArgs {
    #[command(subcommand)]
    pub command: SnuTabCommands,
}

#[derive(Subcommand, Debug)]
pub enum SnuTabCommands {
    /// Activate a browser tab matching a URL pattern
    Activate {
        /// URL or browser extension match pattern
        url: String,

        /// Reload after activation
        #[arg(long)]
        reload: bool,

        /// Wait for page load completion
        #[arg(long)]
        wait_for_load: bool,

        /// Open the URL if no tab matches
        #[arg(long)]
        open_if_not_found: bool,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },
}

#[derive(Args, Debug)]
pub struct SnuContextArgs {
    #[command(subcommand)]
    pub command: SnuContextCommands,
}

#[derive(Subcommand, Debug)]
pub enum SnuContextCommands {
    /// Switch update set, application, or domain in the browser session
    Switch {
        /// Context type to switch
        #[arg(value_enum)]
        switch_type: SnuSwitchType,

        /// sys_id/app_id/domain value
        value: String,

        /// Do not reload a ServiceNow tab after switching
        #[arg(long)]
        no_reload_tab: bool,

        /// Browser tab URL pattern to reload
        #[arg(long, default_value = "https://*.service-now.com/*")]
        tab_url: String,

        /// Seconds to wait for helper/session/response
        #[arg(long, default_value_t = DEFAULT_SNU_TIMEOUT_SECS)]
        timeout_secs: u64,
    },
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum SnuSwitchType {
    Updateset,
    Application,
    Domain,
}

impl SnuSwitchType {
    pub fn as_action_value(&self) -> &'static str {
        match self {
            Self::Updateset => "updateset",
            Self::Application => "application",
            Self::Domain => "domain",
        }
    }
}

// --- Codesearch ---

#[derive(Args, Debug)]
pub struct CodesearchArgs {
    #[command(subcommand)]
    pub command: CodesearchCommands,
}

#[derive(Subcommand, Debug)]
pub enum CodesearchCommands {
    /// Search code across the ServiceNow instance
    Search {
        /// Search query text
        query: String,

        /// Limit to a specific table (e.g., sys_script_include, sys_script, sysevent_script_action)
        #[arg(long = "source-table", alias = "table")]
        source_table: Option<String>,

        /// Restrict search to a specific application scope (e.g., x_my_app, global)
        #[arg(long)]
        scope: Option<String>,

        /// Maximum number of results to return (default: 100)
        #[arg(long, default_value = "100")]
        limit: usize,

        /// Restrict search to the current scope only
        #[arg(long)]
        current_scope: bool,

        /// Search group to use (advanced)
        #[arg(long, default_value = "sn_devstudio.Studio Search Group")]
        search_group: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_debug_assert() {
        Cli::command().debug_assert();
    }

    #[test]
    fn test_top_level_help_includes_snowflake() {
        let help = Cli::command().render_long_help().to_string();
        assert!(help.contains("❄️ snow-cli"));
    }

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
    fn test_snu_help_mentions_browser_helpers_and_background_scripts() {
        let help = Cli::command()
            .find_subcommand_mut("snu")
            .unwrap()
            .render_long_help()
            .to_string();
        assert!(help.contains("snow-cli snu check-connection"));
        assert!(help.contains("snow-cli snu context switch application x_my_app"));
        assert!(help.contains("snu execute-bg-script"));
    }

    #[test]
    fn test_parse_verbose_flags() {
        let cli = Cli::parse_from(["snow-cli", "-vvv", "config", "show"]);
        assert_eq!(cli.verbose, 3);
    }

    #[test]
    fn test_parse_profile_flag() {
        let cli = Cli::parse_from(["snow-cli", "--profile", "prod", "config", "show"]);
        assert_eq!(cli.profile, Some("prod".to_string()));
    }

    #[test]
    fn test_parse_profile_is_optional() {
        let cli = Cli::parse_from(["snow-cli", "config", "show"]);
        assert_eq!(cli.profile, None);
    }

    #[test]
    fn test_parse_output_format() {
        let cli = Cli::parse_from(["snow-cli", "--output", "csv", "config", "show"]);
        assert!(matches!(cli.output, OutputFormat::Csv));

        let cli = Cli::parse_from(["snow-cli", "--output", "jsonl", "config", "show"]);
        assert!(matches!(cli.output, OutputFormat::Jsonl));

        let cli = Cli::parse_from(["snow-cli", "--output", "toon", "config", "show"]);
        assert!(matches!(cli.output, OutputFormat::Toon));

        let cli = Cli::parse_from(["snow-cli", "--format", "jsonl", "config", "show"]);
        assert!(matches!(cli.output, OutputFormat::Jsonl));
    }

    #[test]
    fn test_parse_table_list() {
        let cli = Cli::parse_from([
            "snow-cli",
            "table",
            "list",
            "incident",
            "--query",
            "active=true",
            "--limit",
            "10",
        ]);
        match cli.command {
            Commands::Table(args) => match args.command {
                TableCommands::List {
                    table,
                    query,
                    limit,
                    ..
                } => {
                    assert_eq!(table, "incident");
                    assert_eq!(query, Some("active=true".to_string()));
                    assert_eq!(limit, Some(10));
                }
                _ => panic!("Expected Table List command"),
            },
            _ => panic!("Expected Table command"),
        }
    }

    #[test]
    fn test_parse_snu_execute_bg_script() {
        let cli = Cli::parse_from([
            "snow-cli",
            "snu",
            "execute-bg-script",
            "--code",
            "gs.info('hi')",
        ]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::ExecuteBgScript { code, file, .. } => {
                    assert_eq!(code, Some("gs.info('hi')".to_string()));
                    assert_eq!(file, None);
                }
                _ => panic!("Expected Snu ExecuteBgScript command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_check_connection() {
        let cli = Cli::parse_from(["snow-cli", "snu", "check-connection"]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::CheckConnection { timeout_secs } => {
                    assert_eq!(timeout_secs, DEFAULT_SNU_TIMEOUT_SECS);
                }
                _ => panic!("Expected Snu CheckConnection command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_get_instance_info() {
        let cli = Cli::parse_from(["snow-cli", "snu", "get-instance-info"]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::GetInstanceInfo { timeout_secs } => {
                    assert_eq!(timeout_secs, DEFAULT_SNU_TIMEOUT_SECS);
                }
                _ => panic!("Expected Snu GetInstanceInfo command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_list_tables() {
        let cli = Cli::parse_from(["snow-cli", "snu", "list-tables"]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::ListTables { timeout_secs } => {
                    assert_eq!(timeout_secs, DEFAULT_SNU_TIMEOUT_SECS);
                }
                _ => panic!("Expected Snu ListTables command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_create_record() {
        let cli = Cli::parse_from([
            "snow-cli",
            "snu",
            "create-record",
            "incident",
            "--data",
            "{\"short_description\":\"hi\"}",
            "--scope",
            "x_app",
        ]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::CreateRecord {
                    table, data, scope, ..
                } => {
                    assert_eq!(table, "incident");
                    assert_eq!(data.as_deref(), Some("{\"short_description\":\"hi\"}"));
                    assert_eq!(scope.as_deref(), Some("x_app"));
                }
                _ => panic!("Expected Snu CreateRecord command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_app_meta() {
        let cli = Cli::parse_from(["snow-cli", "snu", "app-meta", "x_my_app"]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::AppMeta {
                    app_id,
                    timeout_secs,
                } => {
                    assert_eq!(app_id, "x_my_app");
                    assert_eq!(timeout_secs, DEFAULT_SNU_TIMEOUT_SECS);
                }
                _ => panic!("Expected Snu AppMeta command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_get_record() {
        let cli = Cli::parse_from([
            "snow-cli",
            "snu",
            "get-record",
            "incident",
            "abc123",
            "--fields",
            "sys_id,number",
        ]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::GetRecord {
                    table,
                    sys_id,
                    fields,
                    ..
                } => {
                    assert_eq!(table, "incident");
                    assert_eq!(sys_id, "abc123");
                    assert_eq!(fields, Some("sys_id,number".to_string()));
                }
                _ => panic!("Expected Snu GetRecord command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_update_record_with_data() {
        let cli = Cli::parse_from([
            "snow-cli",
            "snu",
            "update-record",
            "sp_widget",
            "abc123",
            "--data",
            "{\"script\":\"gs.info('x')\",\"css\":\".a{}\"}",
        ]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::UpdateRecord {
                    table,
                    sys_id,
                    data,
                    field,
                    content,
                    await_confirmation,
                    ..
                } => {
                    assert_eq!(table, "sp_widget");
                    assert_eq!(sys_id, "abc123");
                    assert_eq!(
                        data.as_deref(),
                        Some("{\"script\":\"gs.info('x')\",\"css\":\".a{}\"}")
                    );
                    assert_eq!(field, None);
                    assert_eq!(content, None);
                    assert!(!await_confirmation);
                }
                _ => panic!("Expected Snu UpdateRecord command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_update_record_single_field() {
        let cli = Cli::parse_from([
            "snow-cli",
            "snu",
            "update-record",
            "incident",
            "abc123",
            "--field",
            "state",
            "--content",
            "2",
        ]);

        match cli.command {
            Commands::Snu(args) => match args.command {
                SnuCommands::UpdateRecord {
                    field, content, data, ..
                } => {
                    assert_eq!(field.as_deref(), Some("state"));
                    assert_eq!(content.as_deref(), Some("2"));
                    assert_eq!(data, None);
                }
                _ => panic!("Expected Snu UpdateRecord command"),
            },
            _ => panic!("Expected Snu command"),
        }
    }

    #[test]
    fn test_parse_snu_update_record_data_and_field_conflict() {
        let result = Cli::try_parse_from([
            "snow-cli",
            "snu",
            "update-record",
            "incident",
            "abc123",
            "--data",
            "{\"state\":\"2\"}",
            "--field",
            "state",
            "--content",
            "2",
        ]);
        assert!(result.is_err(), "--data and --field must be mutually exclusive");
    }

    #[test]
    fn test_parse_scope_inspect_defaults_to_basic_details() {
        let cli = Cli::parse_from(["snow-cli", "scope", "inspect", "x_my_app"]);

        match cli.command {
            Commands::Scope(args) => match args.command {
                ScopeCommands::Inspect { scope, details } => {
                    assert_eq!(scope, "x_my_app");
                    assert!(matches!(details, ScopeDetailLevel::Basic));
                }
                _ => panic!("Expected Scope Inspect command"),
            },
            _ => panic!("Expected Scope command"),
        }
    }

    #[test]
    fn test_parse_scope_list_with_search() {
        let cli = Cli::parse_from([
            "snow-cli",
            "scope",
            "list",
            "global",
            "--kind",
            "plugin",
            "--kind",
            "store-app",
        ]);

        match cli.command {
            Commands::Scope(args) => match args.command {
                ScopeCommands::List {
                    search,
                    kind,
                    show_source_table,
                    show_sys_id,
                } => {
                    assert_eq!(search, Some("global".to_string()));
                    assert_eq!(kind, vec![ScopeListKind::Plugin, ScopeListKind::StoreApp]);
                    assert!(!show_source_table);
                    assert!(!show_sys_id);
                }
                _ => panic!("Expected Scope List command"),
            },
            _ => panic!("Expected Scope command"),
        }
    }

    #[test]
    fn test_parse_scope_list_text_column_flags() {
        let cli = Cli::parse_from([
            "snow-cli",
            "scope",
            "list",
            "incident",
            "--show-source-table",
            "--show-sys-id",
        ]);

        match cli.command {
            Commands::Scope(args) => match args.command {
                ScopeCommands::List {
                    search,
                    show_source_table,
                    show_sys_id,
                    ..
                } => {
                    assert_eq!(search, Some("incident".to_string()));
                    assert!(show_source_table);
                    assert!(show_sys_id);
                }
                _ => panic!("Expected Scope List command"),
            },
            _ => panic!("Expected Scope command"),
        }
    }

    #[test]
    fn test_parse_scope_inspect_full_details() {
        let cli = Cli::parse_from([
            "snow-cli",
            "scope",
            "inspect",
            "x_my_app",
            "--details",
            "full",
        ]);

        match cli.command {
            Commands::Scope(args) => match args.command {
                ScopeCommands::Inspect { details, .. } => {
                    assert!(matches!(details, ScopeDetailLevel::Full));
                }
                _ => panic!("Expected Scope Inspect command"),
            },
            _ => panic!("Expected Scope command"),
        }
    }

    #[test]
    fn test_parse_scope_inventory() {
        let cli = Cli::parse_from(["snow-cli", "scope", "inventory", "x_my_app"]);

        match cli.command {
            Commands::Scope(args) => match args.command {
                ScopeCommands::Inventory { scope } => {
                    assert_eq!(scope, "x_my_app");
                }
                _ => panic!("Expected Scope Inventory command"),
            },
            _ => panic!("Expected Scope command"),
        }
    }

    #[test]
    fn test_parse_scope_move_file() {
        let cli = Cli::parse_from([
            "snow-cli",
            "scope",
            "move-file",
            "sys_script_include",
            "abc123",
            "--target-scope",
            "x_target_app",
            "--dry-run",
            "--yes",
        ]);

        match cli.command {
            Commands::Scope(args) => match args.command {
                ScopeCommands::MoveFile {
                    table,
                    sys_id,
                    target_scope,
                    dry_run,
                    yes,
                } => {
                    assert_eq!(table, "sys_script_include");
                    assert_eq!(sys_id, "abc123");
                    assert_eq!(target_scope, "x_target_app");
                    assert!(dry_run);
                    assert!(yes);
                }
                _ => panic!("Expected Scope MoveFile command"),
            },
            _ => panic!("Expected Scope command"),
        }
    }

    #[test]
    fn test_parse_data_export_with_output_path() {
        let cli = Cli::parse_from([
            "snow-cli",
            "data",
            "export",
            "incident",
            "--query",
            "active=true",
            "--fields",
            "sys_id,number",
            "--out",
            "incident.json",
        ]);

        match cli.command {
            Commands::Data(args) => match args.command {
                DataCommands::Export {
                    table,
                    query,
                    fields,
                    out_path,
                    ..
                } => {
                    assert_eq!(table, "incident");
                    assert_eq!(query, Some("active=true".to_string()));
                    assert_eq!(fields, Some("sys_id,number".to_string()));
                    assert_eq!(out_path, Some("incident.json".to_string()));
                }
                _ => panic!("Expected Data Export command"),
            },
            _ => panic!("Expected Data command"),
        }
    }

    #[test]
    fn test_parse_data_export_package() {
        let cli = Cli::parse_from([
            "snow-cli",
            "--timeout-secs",
            "180",
            "data",
            "export-package",
            "--file",
            "dataset-spec.json",
            "--out-dir",
            "exported-dataset",
        ]);
        assert_eq!(cli.timeout_secs, Some(180));

        match cli.command {
            Commands::Data(args) => match args.command {
                DataCommands::ExportPackage { file, out_dir } => {
                    assert_eq!(file, "dataset-spec.json");
                    assert_eq!(out_dir, "exported-dataset");
                }
                _ => panic!("Expected Data ExportPackage command"),
            },
            _ => panic!("Expected Data command"),
        }
    }

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
