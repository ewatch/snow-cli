use clap::{Args, Subcommand};

const AUTH_AFTER_HELP: &str = "Examples:\n  snow-cli auth login\n  snow-cli auth login --no-verify   # store only, e.g. offline setup\n  printf '%s' \"$SNOW_PASSWORD\" | snow-cli auth login --password-stdin\n  snow-cli auth status --verify\n  snow-cli auth token\n  snow-cli auth logout";

const AUTH_LOGIN_AFTER_HELP: &str = "Examples:\n  snow-cli auth login\n  printf '%s' \"$SNOW_PASSWORD\" | snow-cli auth login --password-stdin\n  printf '%s' \"$SNOW_API_TOKEN\" | snow-cli auth login --token-stdin\n  printf '%s' \"$SNOW_CLIENT_SECRET\" | snow-cli auth login --client-secret-stdin\n  snow-cli auth login --no-browser\n  snow-cli auth login --profile sdk --sdk-oauth\n  snow-cli auth login --session-cookie 'JSESSIONID=...; glide_user_route=...'\n\nTip:\n  Prefer interactive prompts or --*-stdin flags over command-line secret flags, which can leak through shell history and process listings.\n  If a required secret flag is omitted and stdin is a TTY, you will be prompted securely.\n  For OAuth2 authorization-code profiles, snow-cli opens the authorization URL and waits for a local redirect callback. Public PKCE clients can omit --client-secret.\n  With --sdk-oauth, snow-cli instead uses the ServiceNow SDK OAuth app's /sdk-oauth.do redirect: approve access in the browser, then paste the code the page displays at the hidden prompt (or pipe it with --code-stdin).\n  For browser-session profiles, provide the full Cookie header value from your authenticated browser session via --session-cookie or the SNOW_SESSION_COOKIE environment variable. The token is not stored.";
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

        /// Use the ServiceNow SDK OAuth app's manual-code callback (redirect URI
        /// /sdk-oauth.do) instead of a localhost listener; paste the code the page displays.
        /// Requires an OAuth2 authorization-code profile
        #[arg(long)]
        sdk_oauth: bool,

        /// Read the authorization code for --sdk-oauth from stdin instead of prompting
        #[arg(
            long,
            requires = "sdk_oauth",
            conflicts_with_all = ["password_stdin", "token_stdin", "client_secret_stdin", "session_cookie_stdin"]
        )]
        code_stdin: bool,

        /// Also write the successful basic login into now-sdk
        #[arg(long)]
        also_now_sdk: bool,

        /// Destination alias name for now-sdk
        #[arg(long, requires = "also_now_sdk")]
        now_sdk_alias: Option<String>,

        /// Mark the now-sdk alias as default
        #[arg(long, requires = "also_now_sdk")]
        set_now_sdk_default: bool,

        /// Store the credentials without making a verification request (offline setup)
        #[arg(long)]
        no_verify: bool,
    },

    /// Clear stored credentials for the active profile
    Logout,

    /// Show current authentication status
    Status {
        /// Make one authenticated request: confirms the credentials and reports the session user, latency, and build
        #[arg(long)]
        verify: bool,
    },

    /// Print the current access token to stdout (for piping to other tools)
    Token,
}
