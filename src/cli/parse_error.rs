//! Actionable command-line parse errors.
//!
//! Clap's defaults do not tell a caller what to do next: `snow-cli table
//! incident` reports only "unrecognized subcommand", and an unknown option
//! gets the tip "to pass '--x' as a value, use '-- --x'", which sends agents
//! the wrong way. This module rewrites those two error kinds to list what the
//! command accepts and suggest the likely call. Other errors, `--help`, and
//! `--version` keep clap's rendering. Usage errors still exit with code 2.

use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{Command, CommandFactory, FromArgMatches};

/// Parse `std::env::args_os()` into `P`, exiting with an actionable message
/// on a usage error.
pub fn parse_or_exit<P: CommandFactory + FromArgMatches>() -> P {
    // Clap gets the original OS strings, so non-UTF-8 paths stay intact; the
    // lossy text is only used to locate the command in a diagnostic.
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let mut command = P::command();
    let error = match command
        .try_get_matches_from_mut(&args)
        .and_then(|mut matches| P::from_arg_matches_mut(&mut matches))
    {
        Ok(parsed) => return parsed,
        Err(error) => error,
    };
    command.build();
    let display_args: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    match actionable_message(&error, &command, &display_args) {
        Some(message) => clap::Error::raw(error.kind(), message).exit(),
        None => error.with_cmd(&command).exit(),
    }
}

/// The rewritten message for `error`, or `None` to keep clap's.
pub(crate) fn actionable_message(
    error: &clap::Error,
    root: &Command,
    args: &[String],
) -> Option<String> {
    let path = command_path(root, args);
    let current = *path.last()?;
    let display_path = path
        .iter()
        .map(|command| command.get_name())
        .collect::<Vec<_>>()
        .join(" ");

    let mut message = match error.kind() {
        ErrorKind::InvalidSubcommand => {
            let bad = context_string(error, ContextKind::InvalidSubcommand)?;
            if is_write_only_command(&path, &bad) {
                read_only_rejection(current, &display_path, &bad)
            } else {
                invalid_subcommand(
                    current,
                    &display_path,
                    &bad,
                    &suggestions(error, ContextKind::SuggestedSubcommand),
                )
            }
        }
        ErrorKind::UnknownArgument => {
            let bad = context_string(error, ContextKind::InvalidArg)?;
            if !bad.starts_with('-') {
                return None;
            }
            unknown_option(
                current,
                path.len() == 1,
                &display_path,
                &bad,
                &suggestions(error, ContextKind::SuggestedArg),
            )
        }
        _ => return None,
    };

    let mut usage_command = current.clone();
    message.push_str(&format!(
        "\n{}\n\nFor more information, try '{display_path} --help'.\n",
        usage_command.render_usage()
    ));
    Some(message)
}

fn invalid_subcommand(
    current: &Command,
    display_path: &str,
    bad: &str,
    suggested: &[String],
) -> String {
    let valid = visible_subcommands(current);
    let mut message =
        format!("'{bad}' is not a `{display_path}` subcommand\n\n  valid subcommands: {valid}\n");
    let looks_like_name = !bad.is_empty()
        && bad
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    // Clap's fuzzy match proposes `find` for `incident`; only near misses help.
    let near_miss = suggested
        .iter()
        .find(|suggestion| edit_distance(bad, suggestion) <= MAX_SUGGESTION_EDITS);
    if let Some(suggestion) = near_miss {
        message.push_str(&format!(
            "  tip: did you mean `{display_path} {suggestion}`?\n"
        ));
    } else if current.get_name() == "table" && looks_like_name {
        message.push_str(&format!(
            "  tip: if '{bad}' is a table, try `{display_path} list {bad}` or `{display_path} schema {bad}`\n"
        ));
    } else if current.get_name() == "codesearch" {
        message.push_str(&format!(
            "  tip: to search code for '{bad}', use `{display_path} search '{bad}'`\n"
        ));
    }
    message
}

/// Name of the read-only binary, whose command tree omits write commands.
const READ_ONLY_BINARY: &str = "snow-cli-ro";

/// Whether `bad` under `path` is rejected only because the read-only binary
/// omits it: the full `snow-cli` command tree has it at the same place.
fn is_write_only_command(path: &[&Command], bad: &str) -> bool {
    if path.first().map(|root| root.get_name()) != Some(READ_ONLY_BINARY) {
        return false;
    }
    let full = crate::cli::args::Cli::command();
    let mut current = &full;
    for command in path.iter().skip(1) {
        match current.find_subcommand(command.get_name()) {
            Some(next) => current = next,
            None => return false,
        }
    }
    current.find_subcommand(bad).is_some()
}

fn read_only_rejection(current: &Command, display_path: &str, bad: &str) -> String {
    let valid = visible_subcommands(current);
    let full_path = display_path.replacen(READ_ONLY_BINARY, "snow-cli", 1);
    format!(
        "'{bad}' is not a `{display_path}` subcommand: `{full_path} {bad}` can change \
         remote data, so the read-only binary does not include it\n\n  valid subcommands: {valid}\n"
    )
}

fn visible_subcommands(current: &Command) -> String {
    current
        .get_subcommands()
        .filter(|command| !command.is_hide_set())
        .map(Command::get_name)
        .filter(|name| *name != "help")
        .collect::<Vec<_>>()
        .join(", ")
}

/// Largest edit distance at which a suggested subcommand is shown.
const MAX_SUGGESTION_EDITS: usize = 2;

/// Levenshtein distance between two short command names.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, left) in a.chars().enumerate() {
        let mut current = vec![i + 1];
        for (j, right) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(left != *right);
            current.push(substitution.min(previous[j + 1] + 1).min(current[j] + 1));
        }
        previous = current;
    }
    previous[b.len()]
}

fn unknown_option(
    current: &Command,
    is_root: bool,
    display_path: &str,
    bad: &str,
    suggested: &[String],
) -> String {
    let supported = current
        .get_arguments()
        // At the root the global options are the command's own options.
        .filter(|arg| !arg.is_hide_set() && (is_root || !arg.is_global_set()))
        .filter_map(|arg| arg.get_long().map(|long| format!("--{long}")))
        .filter(|long| long != "--help")
        .collect::<Vec<_>>();
    let name = bad.split_once('=').map_or(bad, |(name, _)| name);
    let mut message = format!("unknown option '{name}' for `{display_path}`\n\n");
    if supported.is_empty() {
        message.push_str("  this command takes no command-specific options\n");
    } else {
        message.push_str(&format!("  supported options: {}\n", supported.join(", ")));
    }
    if let Some(suggestion) = suggested.first() {
        message.push_str(&format!("  tip: did you mean '{suggestion}'?\n"));
    }
    if !is_root {
        message.push_str("  global options such as --profile and --output are listed in --help\n");
    }
    message
}

/// The chain of commands `args` selects, from the root to the deepest
/// subcommand named before the first positional or invalid token.
fn command_path<'a>(root: &'a Command, args: &[String]) -> Vec<&'a Command> {
    let mut path = vec![root];
    let mut tokens = args.iter().skip(1);
    while let Some(token) = tokens.next() {
        let current = *path.last().unwrap_or(&root);
        if token == "--" {
            break;
        }
        if let Some(long) = token.strip_prefix("--") {
            let (name, inline_value) = match long.split_once('=') {
                Some((name, _)) => (name, true),
                None => (long, false),
            };
            let takes_value = current
                .get_arguments()
                .find(|arg| arg.get_long() == Some(name))
                .is_some_and(|arg| arg.get_action().takes_values());
            if takes_value && !inline_value {
                tokens.next();
            }
            continue;
        }
        if let Some(short) = token.strip_prefix('-').filter(|rest| !rest.is_empty()) {
            let takes_value = short.chars().next().is_some_and(|flag| {
                current
                    .get_arguments()
                    .find(|arg| arg.get_short() == Some(flag))
                    .is_some_and(|arg| arg.get_action().takes_values())
            });
            if takes_value && short.len() == 1 {
                tokens.next();
            }
            continue;
        }
        match current.find_subcommand(token) {
            Some(subcommand) => path.push(subcommand),
            None => break,
        }
    }
    path
}

fn context_string(error: &clap::Error, kind: ContextKind) -> Option<String> {
    match error.get(kind)? {
        ContextValue::String(value) => Some(value.clone()),
        _ => None,
    }
}

fn suggestions(error: &clap::Error, kind: ContextKind) -> Vec<String> {
    match error.get(kind) {
        Some(ContextValue::String(value)) => vec![value.clone()],
        Some(ContextValue::Strings(values)) => values.clone(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::Cli;

    fn message_for(argv: &[&str]) -> Option<String> {
        let args: Vec<String> = argv.iter().map(|arg| (*arg).to_owned()).collect();
        let mut root = Cli::command();
        let error = root.try_get_matches_from_mut(&args).unwrap_err();
        root.build();
        actionable_message(&error, &root, &args)
    }

    #[test]
    fn resource_first_table_call_lists_subcommands_and_suggests_list() {
        let message = message_for(&["snow-cli", "table", "incident"]).unwrap();
        assert!(
            message.contains("'incident' is not a `snow-cli table` subcommand"),
            "{message}"
        );
        assert!(
            message.contains("valid subcommands: list, get,"),
            "{message}"
        );
        assert!(
            message.contains("`snow-cli table list incident`"),
            "{message}"
        );
        assert!(message.contains("Usage: snow-cli table"), "{message}");
    }

    #[test]
    fn codesearch_without_subcommand_suggests_search() {
        let message = message_for(&["snow-cli", "--profile", "dev", "codesearch", "foo"]).unwrap();
        assert!(
            message.contains("`snow-cli codesearch search 'foo'`"),
            "{message}"
        );
    }

    #[test]
    fn near_miss_subcommand_keeps_clap_suggestion() {
        let message = message_for(&["snow-cli", "scope", "lst"]).unwrap();
        assert!(
            message.contains("did you mean `snow-cli scope list`?"),
            "{message}"
        );
    }

    #[test]
    fn unknown_option_lists_supported_options_without_dash_dash_tip() {
        let message = message_for(&[
            "snow-cli", "--output", "json", "table", "list", "incident", "--foo",
        ])
        .unwrap();
        assert!(
            message.contains("unknown option '--foo' for `snow-cli table list`"),
            "{message}"
        );
        assert!(message.contains("--query"), "{message}");
        assert!(message.contains("--limit"), "{message}");
        assert!(!message.contains("-- --foo"), "{message}");
        assert!(
            !message.contains("--profile,"),
            "globals are not listed: {message}"
        );
    }

    #[test]
    fn unknown_option_keeps_similar_option_suggestion() {
        let message =
            message_for(&["snow-cli", "table", "list", "incident", "--offest", "3"]).unwrap();
        assert!(message.contains("did you mean '--offset'?"), "{message}");
    }

    #[test]
    fn read_only_binary_explains_omitted_write_commands() {
        let args: Vec<String> = ["snow-cli-ro", "table", "delete", "incident", "abc"]
            .iter()
            .map(|arg| (*arg).to_owned())
            .collect();
        let mut root = crate::cli::readonly_args::ReadOnlyCli::command();
        let error = root.try_get_matches_from_mut(&args).unwrap_err();
        root.build();
        let message = actionable_message(&error, &root, &args).unwrap();
        assert!(
            message.contains("`snow-cli table delete` can change remote data"),
            "{message}"
        );
        assert!(
            message.contains("valid subcommands: list, get, schema"),
            "{message}"
        );
    }

    #[test]
    fn other_errors_keep_clap_rendering() {
        assert!(message_for(&["snow-cli", "table", "list"]).is_none());
        assert!(message_for(&["snow-cli", "table", "get", "incident", "a", "b", "c"]).is_none());
    }
}
