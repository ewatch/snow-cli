use clap::Args;

const GRAPHQL_AFTER_HELP: &str = "Examples:\n  snow-cli graphql '{ incident { number } }'\n  snow-cli graphql --query 'query Incident($number: String!) { incident(number: $number) { number } }' --variables '{\"number\":\"INC0010001\"}'\n  snow-cli graphql --query-file incident.graphql --variables '{\"number\":\"INC0010001\"}'\n  cat incident.graphql | snow-cli graphql\n\nNotes:\n  - This command has an implicit query action and posts to /api/now/graphql.\n  - Now GraphQL must be enabled by an administrator on the target instance.\n  - The supplied document must match that instance's GraphQL schema.\n  - GraphQL is unavailable in read-only mode because documents may contain mutations.";
// --- GraphQL ---

#[derive(Args, Debug)]
#[command(after_help = GRAPHQL_AFTER_HELP)]
pub struct GraphqlArgs {
    /// Inline GraphQL document
    #[arg(value_name = "DOCUMENT", group = "graphql_source")]
    pub document: Option<String>,

    /// Inline GraphQL document (equivalent to the positional document)
    #[arg(long, value_name = "DOCUMENT", group = "graphql_source")]
    pub query: Option<String>,

    /// Read the GraphQL document from a file
    #[arg(long, value_name = "PATH", group = "graphql_source")]
    pub query_file: Option<std::path::PathBuf>,

    /// GraphQL variables as a JSON object
    #[arg(long, value_name = "JSON")]
    pub variables: Option<String>,
}

#[cfg(test)]
mod tests {
    use crate::cli::args::{Cli, Commands};
    use clap::{CommandFactory, Parser};

    #[test]
    fn graphql_help_documents_sources_and_enablement() {
        let help = Cli::command()
            .find_subcommand_mut("graphql")
            .unwrap()
            .render_long_help()
            .to_string();
        assert!(help.contains("DOCUMENT"));
        assert!(help.contains("--query"));
        assert!(help.contains("--query-file"));
        assert!(help.contains("--variables"));
        assert!(help.contains("enabled by an administrator"));
        assert!(help.contains("implicit query action"));
    }

    #[test]
    fn parses_each_graphql_document_source() {
        let positional = Cli::parse_from(["snow-cli", "graphql", "{ incident { number } }"]);
        match positional.command {
            Commands::Graphql(args) => {
                assert_eq!(args.document.as_deref(), Some("{ incident { number } }"));
                assert!(args.query.is_none());
                assert!(args.query_file.is_none());
            }
            _ => panic!("Expected GraphQL command"),
        }

        let query = Cli::parse_from([
            "snow-cli",
            "graphql",
            "--query",
            "{ incident { number } }",
            "--variables",
            r#"{"limit":1}"#,
        ]);
        match query.command {
            Commands::Graphql(args) => {
                assert_eq!(args.query.as_deref(), Some("{ incident { number } }"));
                assert_eq!(args.variables.as_deref(), Some(r#"{"limit":1}"#));
            }
            _ => panic!("Expected GraphQL command"),
        }

        let file = Cli::parse_from(["snow-cli", "graphql", "--query-file", "incident.graphql"]);
        match file.command {
            Commands::Graphql(args) => {
                assert_eq!(
                    args.query_file.as_deref(),
                    Some(std::path::Path::new("incident.graphql"))
                );
            }
            _ => panic!("Expected GraphQL command"),
        }
    }

    #[test]
    fn rejects_multiple_explicit_graphql_sources() {
        let error = Cli::try_parse_from([
            "snow-cli",
            "graphql",
            "{ incident { number } }",
            "--query",
            "{ user { name } }",
        ])
        .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
}
