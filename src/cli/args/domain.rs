use crate::models::identifiers::TableName;
use clap::{Args, Subcommand, ValueEnum};

#[derive(Args, Debug)]
#[command(
    after_help = "Examples:\n  snow-cli domain status\n  snow-cli domain list\n  snow-cli domain table incident\n  snow-cli domain table incident --counts\n  snow-cli domain table incident --counts all   # admin: all domains\n  snow-cli domain inspect <domain_sys_id>\n\nDomain separation is independent of application scope. Home domain is not proof of session domain; empty results can reflect restricted visibility. These commands read configuration and never calculate effective access or switch session domains."
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
        /// Count records per domain. Without a value, covers the domains visible
        /// to this session and, for admin or query_no_domain_table_api, reports
        /// hidden records; `all` counts across all domains in one background
        /// script (admin only; not available in snow-cli-ro)
        #[arg(
            long,
            value_enum,
            value_name = "SCOPE",
            num_args = 0..=1,
            default_missing_value = "visible"
        )]
        counts: Option<DomainCountScope>,
    },
}

/// Which domains `domain table --counts` covers.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainCountScope {
    /// Domains visible to this session, from the Aggregate API (the default)
    Visible,
    /// Every domain, from one read-only background script (admin only)
    All,
}

/// Domain-visibility options shared by the Table API record reads
/// (`table list` and `table get`) in both binaries.
#[derive(Args, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DomainReadArgs {
    /// Request cross-domain Table API reads (admin or query_no_domain_table_api required)
    #[arg(long)]
    pub query_no_domain: bool,

    /// Add each record's domain (sys_domain, sys_domain_path, and a resolved
    /// sys_domain_name) and, for lists, a per-domain summary of the page
    #[arg(long)]
    pub with_domain: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Cli, Commands};
    use clap::Parser;

    fn parse_counts(extra: &[&str]) -> Result<Option<DomainCountScope>, clap::Error> {
        let argv = ["snow-cli", "domain", "table", "incident"]
            .into_iter()
            .chain(extra.iter().copied());
        match Cli::try_parse_from(argv)?.command {
            Commands::Domain(DomainArgs {
                command: DomainCommands::Table { counts, .. },
            }) => Ok(counts),
            _ => panic!("Expected Domain Table command"),
        }
    }

    #[test]
    fn counts_value_selects_the_scope() {
        assert_eq!(parse_counts(&[]).unwrap(), None);
        assert_eq!(
            parse_counts(&["--counts"]).unwrap(),
            Some(DomainCountScope::Visible)
        );
        assert_eq!(
            parse_counts(&["--counts", "visible"]).unwrap(),
            Some(DomainCountScope::Visible)
        );
        assert_eq!(
            parse_counts(&["--counts", "all"]).unwrap(),
            Some(DomainCountScope::All)
        );
        assert_eq!(
            parse_counts(&["--counts=all"]).unwrap(),
            Some(DomainCountScope::All)
        );
        assert_eq!(
            parse_counts(&["--counts", "everything"])
                .unwrap_err()
                .kind(),
            clap::error::ErrorKind::InvalidValue
        );
    }
}
