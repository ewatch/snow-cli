#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
struct Rule {
    name: &'static str,
    forms: &'static [&'static str],
    allowed: &'static [&'static str],
    remediation: &'static str,
}

const RULES: &[Rule] = &[
    Rule {
        name: "HTTP transport seam",
        forms: &["reqwest"],
        allowed: &["client/"],
        remediation: "use the focused interface in src/client/; reqwest is allowed only under src/client/",
    },
    Rule {
        name: "SnowClient transport escape hatch",
        forms: &[".http()", "SnowClient::http"],
        allowed: &[],
        remediation: "add or use a focused SnowClient operation instead of exposing its transport",
    },
    Rule {
        name: "active-policy initialization seam",
        forms: &["set_active_policy("],
        allowed: &["lib.rs", "policy.rs"],
        remediation: "initialize active policy only in src/lib.rs; src/policy.rs owns the setter implementation",
    },
    Rule {
        name: "process termination seam",
        forms: &["std::process::exit", "process::exit"],
        allowed: &["main.rs", "bin/snow-cli-ro.rs"],
        remediation: "return an exit code or error; only binary roots may terminate the process",
    },
];

fn rust_sources(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    fn visit(current: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in fs::read_dir(current)? {
            let path = entry?.path();
            if path.is_dir() {
                visit(&path, files)?;
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn path_is_allowed(relative: &str, allowed: &[&str]) -> bool {
    allowed.iter().any(|entry| {
        if entry.ends_with('/') {
            relative.starts_with(entry)
        } else {
            relative == *entry
        }
    })
}

fn boundary_violations(root: &Path) -> std::io::Result<Vec<String>> {
    let mut violations = Vec::new();
    for source in rust_sources(root)? {
        let relative = source
            .strip_prefix(root)
            .expect("walked source must remain below root")
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        let text = fs::read_to_string(&source)?;

        for (line_index, line) in text.lines().enumerate() {
            for rule in RULES {
                if path_is_allowed(&relative, rule.allowed) {
                    continue;
                }
                for forbidden in rule.forms {
                    if line.contains(forbidden) {
                        let allowed = if rule.allowed.is_empty() {
                            "nowhere under src/".to_string()
                        } else {
                            rule.allowed.join(", ")
                        };
                        violations.push(format!(
                            "{relative}:{}: {} forbids `{forbidden}` (allowed: {allowed}). {}",
                            line_index + 1,
                            rule.name,
                            rule.remediation
                        ));
                    }
                }
            }
        }
    }
    Ok(violations)
}

#[test]
fn source_tree_respects_module_boundaries() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let violations = boundary_violations(&root).expect("structural source scan should succeed");
    assert!(
        violations.is_empty(),
        "module boundary violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn rules_accept_only_their_explicit_allowed_paths() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    fs::create_dir_all(root.join("client")).unwrap();
    fs::create_dir_all(root.join("cli/commands")).unwrap();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(root.join("client/transport.rs"), "use reqwest::Client;\n").unwrap();
    fs::write(root.join("lib.rs"), "policy::set_active_policy(policy);\n").unwrap();
    fs::write(
        root.join("policy.rs"),
        "pub fn set_active_policy(policy: Policy) {}\n",
    )
    .unwrap();
    fs::write(root.join("main.rs"), "std::process::exit(1);\n").unwrap();
    fs::write(root.join("bin/snow-cli-ro.rs"), "std::process::exit(1);\n").unwrap();

    assert!(boundary_violations(root).unwrap().is_empty());
}

#[test]
fn rules_report_path_line_form_and_remediation() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    fs::create_dir_all(root.join("cli/commands")).unwrap();
    fs::write(
        root.join("cli/commands/bad.rs"),
        "\nuse reqwest::Client;\nclient.http();\nset_active_policy(policy);\nstd::process::exit(1);\n",
    )
    .unwrap();

    let diagnostics = boundary_violations(root).unwrap().join("\n");
    assert!(diagnostics.contains("cli/commands/bad.rs:2"));
    assert!(diagnostics.contains("`reqwest`"));
    assert!(diagnostics.contains("src/client/"));
    assert!(diagnostics.contains("cli/commands/bad.rs:3"));
    assert!(diagnostics.contains("`.http()`"));
    assert!(diagnostics.contains("cli/commands/bad.rs:4"));
    assert!(diagnostics.contains("`set_active_policy(`"));
    assert!(diagnostics.contains("cli/commands/bad.rs:5"));
    assert!(diagnostics.contains("`std::process::exit`"));
}
