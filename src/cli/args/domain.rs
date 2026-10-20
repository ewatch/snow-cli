use crate::models::identifiers::TableName;
use clap::{Args, Subcommand};

#[derive(Args, Debug)]
#[command(
    after_help = "Examples:\n  snow-cli domain status\n  snow-cli domain list\n  snow-cli domain table incident\n  snow-cli domain table incident --counts\n  snow-cli domain table incident --counts --via-script   # admin: all domains\n  snow-cli domain inspect <domain_sys_id>\n\nDomain separation is independent of application scope. Home domain is not proof of session domain; empty results can reflect restricted visibility. These commands read configuration and never calculate effective access or switch session domains."
)]
pub struct DomainArgs {
    #[command(subcommand)]
    pub command: DomainCommands,
}

/// These commands only read instance configuration; they never switch domains.
#[derive(Subcommand, Debug)]
pub enum DomainCommands {
    /// Read domain settings and the authenticated user's home domain
    Status,
    /// List visible domain records and parent relationships (global is a special value)
    List {
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// Request cross-domain reads (admin or query_no_domain_table_api required)
        #[arg(long)]
        query_no_domain: bool,
    },
    /// Read a domain's direct children, Contains relationships, and visibility grants
    Inspect {
        /// Domain sys_id or the special global value
        domain: crate::models::domain::DomainId,
        /// Maximum rows per relationship table
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// Request cross-domain reads (admin or query_no_domain_table_api required)
        #[arg(long)]
        query_no_domain: bool,
    },
    /// Inspect inherited domain fields on a table
    Table {
        table: TableName,
        /// Count records per domain. Covers the domains visible to this session
        /// and, for admin or query_no_domain_table_api, reports hidden records
        #[arg(long)]
        counts: bool,
        /// With --counts: count across all domains in one background script
        /// (admin only; not available in snow-cli-ro)
        #[arg(long, requires = "counts")]
        via_script: bool,
    },
}
