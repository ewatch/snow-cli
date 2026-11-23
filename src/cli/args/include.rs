use clap::{Args, Subcommand};

use crate::models::identifiers::RecordKey;

const INCLUDE_SHOW_AFTER_HELP: &str = "Examples:
  snow-cli include show ChangeRequestStateHandler --chain --source-of canMoveTo,_resetModel
  snow-cli include show ChangeRequestStateHandler ChangeRequestStateModel_normal --chain
  snow-cli include show ChangeRequestStateHandler --chain --grep 'canMoveTo|_resetModel' --context 10
  snow-cli include show global.AbstractAjaxProcessor   # one include with its full source

Output:
  --chain (or several names) returns {includes:[...],references:[...]}: each named include, then
  the classes it extends (up to 5), with name, api_name, scope, extends, and an outline of its
  prototype members: \"152-187 canMoveTo(toState)\" (lines, name, params),
  \"27-69 assess{nextState=[authorize],draft,authorize}\" (object keys), \"38 MODEL='x'\" (literal).
  No source unless --source-of, --grep, or --script; a single name without --chain includes the
  full source (--no-script omits it). references are other classes the sources name
  (`new X(`, `X.y`), unverified: add them as extra names to read them next.

Notes:
  - Looks up sys_script_include by name (api_name when the value contains '.', or a sys_id).
  - extends is parsed from Object.extendsObject(...) (\"extends_source\":\"parsed\").
  - --source-of finds members in every listed class; --grep prints \"N:text\" matches and \"N-text\"
    context. Each is bounded to 400 lines per command (\"truncated\":true).
  - One request per class; nothing is re-downloaded per --grep window.";

/// Script include source and inheritance.
#[derive(Args, Debug)]
pub struct IncludeArgs {
    #[command(subcommand)]
    pub command: IncludeCommands,
}

#[derive(Subcommand, Debug)]
pub enum IncludeCommands {
    /// Show a script include by name: scope, extends, methods, and source
    #[command(after_help = INCLUDE_SHOW_AFTER_HELP)]
    Show {
        /// Script include name, api_name (scope.Name), or sys_id; up to 5
        /// names are shown together
        #[arg(value_name = "NAME", required = true, num_args = 1..=5)]
        names: Vec<RecordKey>,

        /// Show the whole extends chain, handler first (up to 5 levels), as an
        /// outline without source
        #[arg(long)]
        chain: bool,

        /// Include only these members' source (comma-separated, e.g.
        /// canMoveTo,_resetModel), searched in every class of the chain
        #[arg(long, value_name = "MEMBER", value_delimiter = ',', num_args = 1..)]
        source_of: Vec<String>,

        /// Print only source lines matching this regex (with line numbers)
        /// instead of the whole script
        #[arg(long, value_name = "REGEX", allow_hyphen_values = true)]
        grep: Option<String>,

        /// Lines of context around each --grep match
        #[arg(long, value_name = "N", default_value_t = 3, requires = "grep")]
        context: usize,

        /// Include the full source of every class (the default without --chain,
        /// --grep, or --source-of)
        #[arg(long, conflicts_with = "no_script")]
        script: bool,

        /// Omit the script source (metadata, extends, and outline only)
        #[arg(long, conflicts_with_all = ["grep", "source_of"])]
        no_script: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{Cli, Commands};
    use clap::Parser;

    #[test]
    fn parses_include_show_with_chain_and_grep() {
        let cli = Cli::parse_from([
            "snow-cli",
            "include",
            "show",
            "ChangeRequestStateHandler",
            "--chain",
            "--source-of",
            "canMoveTo,_resetModel",
            "--grep",
            "canMoveTo|_resetModel",
            "--context",
            "15",
        ]);
        match cli.command {
            Commands::Include(args) => match args.command {
                IncludeCommands::Show {
                    names,
                    chain,
                    source_of,
                    grep,
                    context,
                    script,
                    no_script,
                } => {
                    assert!(matches!(names.as_slice(), [RecordKey::Name(_)]));
                    assert!(chain);
                    assert_eq!(source_of, ["canMoveTo", "_resetModel"]);
                    assert_eq!(grep.as_deref(), Some("canMoveTo|_resetModel"));
                    assert_eq!(context, 15);
                    assert!(!script && !no_script);
                }
            },
            _ => panic!("Expected Include command"),
        }
    }

    #[test]
    fn script_flags_conflict() {
        assert!(
            Cli::try_parse_from([
                "snow-cli",
                "include",
                "show",
                "X",
                "--script",
                "--no-script"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "snow-cli",
                "include",
                "show",
                "X",
                "--no-script",
                "--source-of",
                "a"
            ])
            .is_err()
        );
    }

    #[test]
    fn context_requires_grep() {
        assert!(
            Cli::try_parse_from(["snow-cli", "include", "show", "X", "--context", "2"]).is_err()
        );
    }
}
