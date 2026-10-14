#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use clap::{Parser, error::ErrorKind};
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
