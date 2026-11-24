#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use clap::{CommandFactory, Parser, error::ErrorKind};
use snow_cli::cli::{args::Cli, readonly_args::ReadOnlyCli};
use std::path::Path;

/// Syntax-only checks: never execute examples, load profiles, or call an instance.
fn check_examples(path: &Path) -> usize {
    let source = std::fs::read_to_string(path).unwrap();
    let mut in_bash = false;
    let mut checked = 0;
    for (index, line) in source.lines().enumerate() {
        let line = line.trim();
        if line.starts_with("```") {
            in_bash = line == "```bash";
            continue;
        }
        if !in_bash || !(line.starts_with("snow-cli ") || line.starts_with("snow-cli-ro ")) {
            continue;
        }
        // This slice covers complete literal one-line commands. Shell workflows,
        // continuation lines, metavariables, and variable expansion are excluded.
        if line.ends_with('\\')
            || line.contains('<')
            || line.contains('>')
            || line.contains("[options]")
            || line.contains('$')
        {
            continue;
        }
        let argv = shlex::split(line)
            .unwrap_or_else(|| panic!("{}:{}: malformed shell quoting", path.display(), index + 1));
        if argv
            .iter()
            .any(|arg| matches!(arg.as_str(), "|" | "&&" | ";" | "||"))
        {
            continue;
        }
        let result = if argv[0] == "snow-cli-ro" {
            ReadOnlyCli::try_parse_from(&argv).map(|_| ())
        } else {
            Cli::try_parse_from(&argv).map(|_| ())
        };
        if let Err(error) = result {
            assert!(
                matches!(
                    error.kind(),
                    ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
                ),
                "{}:{}: {line}\n{error}",
                path.display(),
                index + 1
            );
        }
        checked += 1;
    }
    checked
}

#[test]
fn literal_documented_examples_match_the_parsers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut checked = check_examples(&root.join("README.md"));
    for entry in std::fs::read_dir(root.join("docs/site/src/content/docs/commands")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "md") {
            checked += check_examples(&path);
        }
    }
    assert!(
        checked >= 100,
        "example coverage unexpectedly dropped: {checked}"
    );
}

#[test]
fn table_full_help_states_the_lossless_default_in_both_binaries() {
    for binary in ["snow-cli", "snow-cli-ro"] {
        for command in ["list", "get"] {
            let argv = [binary, "table", command, "--help"];
            let error = if binary == "snow-cli" {
                Cli::try_parse_from(argv).unwrap_err()
            } else {
                ReadOnlyCli::try_parse_from(argv).unwrap_err()
            };
            assert_eq!(error.kind(), ErrorKind::DisplayHelp);
            assert!(
                error
                    .to_string()
                    .contains("Explicit lossless option (the default)")
            );
            assert!(!error.to_string().contains("instead of capping"));
        }
    }
}

/// Replace angle-bracket metavariables in help examples with syntactically
/// valid dummy values so `Cli::try_parse_from` / `ReadOnlyCli::try_parse_from`
/// validates the entire flag/subcommand structure of every `after_help` line.
fn substitute_help_placeholders(line: &str) -> String {
    let mut out = line.to_string();
    for (placeholder, sample) in [
        ("<sys_id>", "6816f79cc0a8016401c5a33be04be441"),
        ("<domain_sys_id>", "6816f79cc0a8016401c5a33be04be441"),
        ("<run-id>", "run-123"),
        ("<name>", "ChangeRequestStateHandler"),
        ("<flow name>", "ChangeFlow"),
        ("<password>", "secret"),
        ("<url-or-host>", "https://dev12345.service-now.com"),
        ("<path>", "script.js"),
        ("<id>", "abc123"),
        ("<user>", "admin"),
        ("<profile>", "dev"),
    ] {
        out = out.replace(placeholder, sample);
    }
    out
}

/// Extract a runnable `snow-cli ...` or `snow-cli-ro ...` command fragment from
/// a single `after_help` line (handling recipe labels, piped stdin prefixes,
/// trailing pipes, and trailing `# comments`).
fn extract_help_example(raw_line: &str) -> Option<String> {
    let trimmed = raw_line.trim();
    if trimmed.starts_with('-') {
        return None;
    }
    let start = trimmed
        .find("snow-cli-ro ")
        .or_else(|| trimmed.find("snow-cli "))?;
    let prefix = &trimmed[..start];
    // Accept only command lines: bare `snow-cli ...`, piped stdin (`... | snow-cli ...`),
    // or labeled recipes (`Labels, not codes:   snow-cli ...`).
    let valid_prefix = prefix.is_empty()
        || prefix.ends_with("| ")
        || (prefix.trim_end().ends_with(':') && prefix.ends_with("  "));
    if !valid_prefix {
        return None;
    }
    let mut cmd = trimmed[start..].to_string();
    // Strip trailing shell comments (`   # ...`) outside quotes.
    if let Some(comment_idx) = cmd.find("   #") {
        cmd.truncate(comment_idx);
    }
    // Strip trailing shell pipe (e.g. `| grep -n canMoveTo`).
    if let Some(pipe_idx) = cmd.find(" | ") {
        cmd.truncate(pipe_idx);
    }
    let cmd = substitute_help_placeholders(cmd.trim());
    if cmd.contains('<') || cmd.contains('>') {
        return None;
    }
    Some(cmd)
}

fn check_command_after_help_examples(cmd: &clap::Command, path: &str, checked: &mut usize) {
    if let Some(after_help) = cmd.get_after_help() {
        let text = after_help.to_string();
        let mut in_prose_section = false;
        for (line_idx, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if matches!(trimmed, "Notes:" | "Tip:") {
                in_prose_section = true;
                continue;
            }
            if !line.starts_with(' ') && trimmed.ends_with(':') {
                in_prose_section = false;
                continue;
            }
            if in_prose_section {
                continue;
            }
            let Some(example) = extract_help_example(line) else {
                continue;
            };
            let argv = shlex::split(&example).unwrap_or_else(|| {
                panic!(
                    "after_help on `{path}` line {}: malformed shell quoting in `{example}`",
                    line_idx + 1
                )
            });
            if argv.is_empty() {
                continue;
            }
            let result = if argv[0] == "snow-cli-ro" {
                ReadOnlyCli::try_parse_from(&argv).map(|_| ())
            } else {
                Cli::try_parse_from(&argv).map(|_| ())
            };
            if let Err(error) = result {
                assert!(
                    matches!(
                        error.kind(),
                        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
                    ),
                    "after_help on `{path}` line {}: `{example}` failed to parse:\n{error}",
                    line_idx + 1
                );
            }
            *checked += 1;
        }
    }
    for sub in cmd.get_subcommands() {
        if sub.get_name() == "help" {
            continue;
        }
        let next = format!("{path} {}", sub.get_name());
        check_command_after_help_examples(sub, &next, checked);
    }
}

#[test]
fn all_after_help_examples_parse_against_cli_and_read_only_cli() {
    let mut checked = 0;
    let full = Cli::command();
    check_command_after_help_examples(&full, "snow-cli", &mut checked);
    let ro = ReadOnlyCli::command();
    check_command_after_help_examples(&ro, "snow-cli-ro", &mut checked);
    assert!(
        checked >= 60,
        "after_help example coverage unexpectedly low: {checked}"
    );
}
