use clap::{Args, Subcommand};

use crate::models::identifiers::TableName;

const DATA_AFTER_HELP: &str = "Examples:\n  snow-cli data export incident --query 'active=true'\n  snow-cli data export sys_user --fields sys_id,user_name,email --out users.json\n  snow-cli data export-package --file dataset-spec.json --out-dir exported-dataset\n  snow-cli data validate --file export.json\n  snow-cli data import --file export.json\n  snow-cli data import --file users.json --import-set-table imp_user";

const DATA_EXPORT_AFTER_HELP: &str = "Examples:\n  snow-cli data export incident --query 'active=true'\n  snow-cli data export incident --fields sys_id,number,short_description --limit 50\n  snow-cli --output csv data export sys_user --fields sys_id,user_name,email --out users.csv";

const DATA_EXPORT_PACKAGE_AFTER_HELP: &str = "Examples:\n  snow-cli data export-package --file dataset-spec.json --out-dir exported-dataset\n  snow-cli data validate --file exported-dataset/manifest.json\n  snow-cli data import --file exported-dataset/manifest.json";
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
        table: TableName,

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
        import_set_table: Option<TableName>,

        /// Exit non-zero when Import Set API responses contain row-level errors
        #[arg(long)]
        fail_on_error: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Cli, Commands};
    use clap::Parser;

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
                    assert_eq!(table.as_str(), "incident");
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
}
