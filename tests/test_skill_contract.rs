#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Static checks on the bundled agent skill (`skills/snow-cli/`): the facts
//! agents act on must match the CLI, and always-loaded text must stay small.
//!
//! Example syntax is covered by `test_skills.rs` and
//! `scripts/check-skill-examples.py`; the retry and exit-code contract in
//! `SKILL.md` §10 by the `skill_contract` unit tests in `src/error.rs`.

use regex::Regex;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// `SKILL.md` is loaded into the agent's context whenever the skill triggers,
/// so it has the tightest budget. Headroom over the current ~11 KiB is small
/// on purpose: growth should be a decision, not drift.
const SKILL_MD_MAX_BYTES: usize = 12 * 1024;

/// References and packs load on demand, one at a time; the budget keeps each
/// playbook focused enough to read in a single step.
const ON_DEMAND_MAX_BYTES: usize = 10 * 1024;

/// Agent skill frontmatter limits (name: lowercase letters, digits and
/// hyphens; description: plain text used for triggering).
const NAME_MAX_CHARS: usize = 64;
const DESCRIPTION_MAX_CHARS: usize = 1024;

/// Only the read-only binary is pre-approved; see SKILL.md §1.
const EXPECTED_ALLOWED_TOOLS: &str = "Bash(snow-cli-ro *)";

fn skill_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills/snow-cli")
}

fn markdown_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn rust_sources(root: &Path) -> String {
    let mut all = String::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                all.push_str(&fs::read_to_string(&path).unwrap());
            }
        }
    }
    all
}

/// Returns the frontmatter as `key -> value` for single-line keys.
fn frontmatter(skill_md: &str) -> Vec<(String, String)> {
    let body = skill_md
        .strip_prefix("---\n")
        .expect("SKILL.md starts with `---` frontmatter");
    let end = body
        .find("\n---\n")
        .expect("frontmatter is closed by `---`");
    body[..end]
        .lines()
        .filter_map(|line| line.split_once(": "))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

#[test]
fn documented_error_codes_exist_in_source() {
    let skill = skill_dir();
    let sources = rust_sources(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src"));

    // Multi-word codes anywhere (`ACL_DENIED`), JSON examples
    // (`"code":"UNKNOWN_FIELD"`), and the single-word codes in §10's
    // "Branch on `code`" list, which other uppercase words (query operators
    // such as `ISEMPTY`) would otherwise drown out.
    let multi_word = Regex::new(r"`([A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+)`").unwrap();
    let json_code = Regex::new(r#""code":"([A-Z][A-Z0-9_]*)""#).unwrap();
    let any_code = Regex::new(r"`([A-Z][A-Z0-9_]*)`").unwrap();

    let mut documented = BTreeSet::new();
    for file in markdown_files(&skill) {
        let text = fs::read_to_string(&file).unwrap();
        for regex in [&multi_word, &json_code] {
            for capture in regex.captures_iter(&text) {
                documented.insert(capture[1].to_string());
            }
        }
    }
    let skill_md = fs::read_to_string(skill.join("SKILL.md")).unwrap();
    let branch_list = skill_md
        .split_once("Branch on `code`:")
        .and_then(|(_, rest)| rest.split_once(". "))
        .map(|(list, _)| list)
        .expect("SKILL.md §10 has a `Branch on `code`:` list");
    for capture in any_code.captures_iter(branch_list) {
        documented.insert(capture[1].to_string());
    }

    assert!(
        documented.len() >= 10,
        "expected at least 10 documented error codes, found {documented:?}"
    );
    let missing: Vec<_> = documented
        .iter()
        .filter(|code| !sources.contains(&format!("\"{code}\"")))
        .collect();
    assert!(
        missing.is_empty(),
        "skill docs mention error codes the CLI never emits: {missing:?}"
    );
}

#[test]
fn skill_frontmatter_is_valid() {
    let skill_md = fs::read_to_string(skill_dir().join("SKILL.md")).unwrap();
    let fields = frontmatter(&skill_md);
    let get = |key: &str| {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .unwrap_or_else(|| panic!("SKILL.md frontmatter is missing `{key}`"))
    };

    // Unquoted values are YAML plain scalars. `: ` or ` #` inside one, or a
    // reserved first character, makes the frontmatter invalid YAML; strict
    // parsers (pi, for one) then drop the whole skill without a warning.
    for (key, value) in &fields {
        if value.starts_with('"') || value.starts_with('\'') {
            continue;
        }
        assert!(
            !value.contains(": ") && !value.contains(" #"),
            "frontmatter `{key}` contains `: ` or ` #`, which is invalid in an unquoted YAML value"
        );
        assert!(
            !value.starts_with(['[', '{', '&', '*', '!', '|', '>', '%', '@', '`']),
            "frontmatter `{key}` starts with a YAML indicator; quote the value"
        );
    }

    let name = get("name");
    assert_eq!(name, "snow-cli", "skill name must match its directory");
    assert!(
        name.len() <= NAME_MAX_CHARS
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
        "skill name `{name}` must be lowercase letters, digits and hyphens"
    );

    let description = get("description");
    let chars = description.chars().count();
    assert!(
        (1..=DESCRIPTION_MAX_CHARS).contains(&chars),
        "description is {chars} characters; limit is {DESCRIPTION_MAX_CHARS}"
    );
    assert!(
        !description.contains('<') && !description.contains('>'),
        "description must be plain text without angle brackets"
    );
    assert!(
        description.contains("snow-cli-ro"),
        "description should name the read-only binary agents are expected to use"
    );

    assert_eq!(get("allowed-tools"), EXPECTED_ALLOWED_TOOLS);
}

#[test]
fn skill_files_stay_within_size_budgets() {
    let skill = skill_dir();
    let mut over = Vec::new();
    for file in markdown_files(&skill) {
        let size = fs::metadata(&file).unwrap().len() as usize;
        let budget = if file == skill.join("SKILL.md") {
            SKILL_MD_MAX_BYTES
        } else {
            ON_DEMAND_MAX_BYTES
        };
        if size > budget {
            over.push(format!(
                "{} is {size} bytes (budget {budget})",
                file.strip_prefix(&skill).unwrap().display()
            ));
        }
    }
    assert!(over.is_empty(), "skill files over budget: {over:#?}");
}
