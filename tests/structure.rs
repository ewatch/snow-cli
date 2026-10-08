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

// No raw-response logging/error exceptions are currently needed. Add a narrow
// path + rationale here only for an explicitly gated sensitive diagnostic.
const RAW_RESPONSE_ALLOW_LIST: &[&str] = &[];

fn raw_response_violations(text: &str) -> Vec<&str> {
    let mut violations = Vec::new();
    for macro_name in ["bail!", "anyhow!", "error!", "warn!"] {
        // Inspect multiline invocations as well as single-line ones. The scan
        // is deliberately conservative; fixtures test its coverage.
        for (start, _) in text.match_indices(macro_name) {
            let tail = &text[start..];
            let mut depth = 0;
            let mut quoted = false;
            let mut escaped = false;
            let mut end = tail.len();
            for (index, ch) in tail.char_indices() {
                if quoted {
                    if escaped {
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == '"' {
                        quoted = false;
                    }
                } else if ch == '"' {
                    quoted = true;
                } else if ch == '(' {
                    depth += 1;
                } else if ch == ')' {
                    depth -= 1;
                    if depth == 0 {
                        end = index + 1;
                        break;
                    }
                }
            }
            let invocation = &tail[..end];
            // Ignore prose inside literals, but retain Rust format captures
            // such as `{response_body}` as well as identifiers outside literals.
            let mut identifiers = String::new();
            let mut quoted = false;
            let mut escaped = false;
            let mut capture = false;
            for ch in invocation.chars() {
                if quoted {
                    if escaped {
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == '"' {
                        quoted = false;
                        identifiers.push(' ');
                    } else if ch == '{' {
                        capture = true;
                        identifiers.push(' ');
                    } else if ch == '}' {
                        capture = false;
                        identifiers.push(' ');
                    } else if capture {
                        identifiers.push(ch);
                    }
                } else if ch == '"' {
                    quoted = true;
                } else {
                    identifiers.push(ch);
                }
            }
            for metric in [
                "body.len()",
                "text.len()",
                "response_body.len()",
                "error_body.len()",
                "resp_body.len()",
            ] {
                identifiers = identifiers.replace(metric, "payload_length");
            }
            if identifiers
                .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                .any(|word| {
                    matches!(
                        word,
                        "body" | "text" | "response_body" | "error_body" | "resp_body"
                    )
                })
            {
                violations.push(invocation);
            }
        }
    }
    violations
}

#[test]
fn default_errors_and_warnings_never_interpolate_raw_responses() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();
    for source in rust_sources(&root).unwrap() {
        let relative = source
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if RAW_RESPONSE_ALLOW_LIST.contains(&relative.as_str()) {
            continue;
        }
        let text = fs::read_to_string(&source).unwrap();
        for invocation in raw_response_violations(&text) {
            violations.push(format!("{relative}: {invocation}"));
        }
    }
    assert!(
        violations.is_empty(),
        "raw response in diagnostic; use ApiError or safe_error_detail:\n{}",
        violations.join("\n")
    );
}

#[test]
fn raw_response_guard_covers_multiline_and_format_capture() {
    for source in [
        "anyhow::bail!(\"failed: {}\", body);",
        "anyhow::anyhow!(\"Raw output: {response_body}\");",
        "tracing::error!(\n status = status,\n body = %error_body,\n \"failed\"\n);",
        "tracing::warn!(%error, %text, \"invalid JSON\");",
    ] {
        assert_eq!(raw_response_violations(source).len(), 1, "{source}");
    }
    for source in [
        "tracing::warn!(body_len = size, \"invalid JSON\");",
        "tracing::warn!(body_len = text.len(), \"invalid JSON\");",
        "tracing::error!(body_len = response.body.len(), \"failed\");",
        "anyhow::bail!(\"pipe a request body or use text output\");",
    ] {
        assert!(raw_response_violations(source).is_empty(), "{source}");
    }
    assert_eq!(
        raw_response_violations("tracing::warn!(body_len = text.len(), %text, \"failed\");").len(),
        1
    );
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
