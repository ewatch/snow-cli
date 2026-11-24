use clap::{Args, Subcommand};

const API_AFTER_HELP: &str = "Examples:\n  snow-cli api get /api/now/table/incident?sysparm_limit=1\n  snow-cli api post /api/x_myapp/action --data '{\"dry_run\":true}'\n  snow-cli api get /api/x_myapp/status -H 'X-Trace-Id:abc123'";
pub(crate) const READ_ONLY_API_AFTER_HELP: &str = "Examples:\n  snow-cli-ro api get /api/now/table/incident?sysparm_limit=1\n  snow-cli-ro api get /api/x_myapp/status -H 'X-Trace-Id:abc123'";
// --- API (raw) ---

#[derive(Args, Debug)]
#[command(after_help = API_AFTER_HELP)]
pub struct ApiArgs {
    #[command(subcommand)]
    pub command: ApiCommands,
}

#[derive(Args, Debug)]
pub struct ApiGetArgs {
    /// API path (e.g., /api/x_myapp/my_endpoint)
    pub path: String,

    /// Custom headers (key:value)
    #[arg(long, short = 'H')]
    pub header: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum ApiCommands {
    /// Send a GET request
    Get(ApiGetArgs),

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
