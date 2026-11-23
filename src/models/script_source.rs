//! Pure helpers for reading ServiceNow script sources (script includes).
//!
//! These never execute or evaluate scripts. They give an agent cheap
//! orientation in a long source: the parent class a script include extends
//! (parsed from `Object.extendsObject(...)`) and grep-style line slices, so
//! the whole script need not be re-downloaded for each window. The member
//! outline lives in [`super::script_outline`].

/// Upper bound on matched-or-context lines returned by [`grep_lines`], so one
/// broad pattern cannot reproduce a whole multi-thousand-line script.
pub const MAX_GREP_LINES: usize = 400;

/// The base class named by `Object.extendsObject(<Base>, {...})`, if any.
///
/// `Class.create()` without `extendsObject` yields `None`. Commented-out
/// code is ignored. The result is a parse of the source, not a dictionary
/// field, and can miss unusual inheritance patterns.
pub fn parse_extends(script: &str) -> Option<String> {
    const MARKER: &str = "Object.extendsObject(";
    let code = strip_comments(script);
    let start = code.find(MARKER)? + MARKER.len();
    let rest = code[start..].trim_start();
    let end = rest
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' || ch == '.'))
        .unwrap_or(rest.len());
    let name = rest[..end].trim_end_matches('.');
    name.starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '_' || ch == '$')
        .then(|| name.to_string())
}

/// Remove `/* ... */` blocks and whole-line `//` comments, keeping line
/// structure. Trailing `//` comments are kept because `//` also appears in
/// string literals such as URLs; they never contain the patterns parsed here
/// in practice.
fn strip_comments(script: &str) -> String {
    let mut out = String::with_capacity(script.len());
    let mut rest = script;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => {
                // Keep newlines so line-oriented parsing stays aligned.
                let block = &rest[start..start + 2 + end + 2];
                out.extend(block.chars().filter(|ch| *ch == '\n'));
                rest = &rest[start + 2 + end + 2..];
            }
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.lines()
        .map(|line| {
            if line.trim_start().starts_with("//") {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Result of slicing a script by pattern.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GrepSlice {
    /// Number of lines that matched the pattern.
    pub match_count: usize,
    /// `N:text` for matching lines and `N-text` for context lines (1-based
    /// line numbers, grep convention); `--` separates non-adjacent blocks.
    pub lines: Vec<String>,
    /// Whether output stopped at the line budget.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

/// Lines of `script` matching `is_match`, each with `context` lines around it,
/// at most `max_lines` (callers pass [`MAX_GREP_LINES`] or what remains of a
/// budget shared by several scripts).
pub fn grep_lines(
    script: &str,
    is_match: impl Fn(&str) -> bool,
    context: usize,
    max_lines: usize,
) -> GrepSlice {
    let lines: Vec<&str> = script.lines().collect();
    let matched: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| is_match(line))
        .map(|(index, _)| index)
        .collect();

    let mut out = Vec::new();
    let mut truncated = false;
    let mut last_emitted: Option<usize> = None;
    'outer: for &hit in &matched {
        let from = hit.saturating_sub(context);
        let to = (hit + context).min(lines.len().saturating_sub(1));
        let from = match last_emitted {
            // Overlapping or directly adjacent windows form one block.
            Some(last) if last + 1 >= from => last + 1,
            Some(_) => {
                out.push("--".to_string());
                from
            }
            None => from,
        };
        for (index, line) in lines.iter().enumerate().take(to + 1).skip(from) {
            if out.len() >= max_lines {
                truncated = true;
                break 'outer;
            }
            let separator = if matched.binary_search(&index).is_ok() {
                ':'
            } else {
                '-'
            };
            out.push(format!("{}{separator}{line}", index + 1));
            last_emitted = Some(index);
        }
    }

    GrepSlice {
        match_count: matched.len(),
        lines: out,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HANDLER: &str = "var ChangeRequestStateHandler = Class.create();\n\
/* Customize the base class here.\n   Object.extendsObject(NotThis, {}) */\n\
ChangeRequestStateHandler.prototype = Object.extendsObject(ChangeRequestStateHandlerSNC, {\n\
    initialize: function(changeRequestGr) {\n\
        ChangeRequestStateHandlerSNC.prototype.initialize.call(this, changeRequestGr);\n\
        var helper = { inner: function() {} };\n\
    },\n\
    canMoveTo : function(state) {\n\
        return true;\n\
    },\n\
    type: 'ChangeRequestStateHandler'\n\
});";

    #[test]
    fn parses_extends_from_oob_header_ignoring_comments() {
        assert_eq!(
            parse_extends(HANDLER).as_deref(),
            Some("ChangeRequestStateHandlerSNC")
        );
        assert_eq!(
            parse_extends("var X = Class.create();\nX.prototype = Object.extendsObject( global.AbstractAjaxProcessor ,{").as_deref(),
            Some("global.AbstractAjaxProcessor")
        );
    }

    #[test]
    fn class_create_without_extends_has_no_parent() {
        let script = "var Util = Class.create();\n// Object.extendsObject(Old, {\nUtil.prototype = {\n    initialize: function() {},\n    type: 'Util'\n};";
        assert_eq!(parse_extends(script), None);
    }

    #[test]
    fn grep_numbers_lines_with_context_and_separators() {
        let script = "a\nb\nmatch1\nc\nd\ne\nf\nmatch2\ng";
        let slice = grep_lines(script, |line| line.starts_with("match"), 1, MAX_GREP_LINES);
        assert_eq!(slice.match_count, 2);
        assert_eq!(
            slice.lines,
            ["2-b", "3:match1", "4-c", "--", "7-f", "8:match2", "9-g"]
        );
        assert!(!slice.truncated);
    }

    #[test]
    fn grep_merges_overlapping_windows() {
        let script = "x1\nx2\ny\nx3";
        let slice = grep_lines(script, |line| line.starts_with('x'), 1, MAX_GREP_LINES);
        assert_eq!(slice.lines, ["1:x1", "2:x2", "3-y", "4:x3"]);
    }

    #[test]
    fn grep_joins_adjacent_windows_without_separator() {
        let slice = grep_lines("a\nb\nc", |line| line != "b", 0, MAX_GREP_LINES);
        assert_eq!(slice.lines, ["1:a", "--", "3:c"]);
        let slice = grep_lines("a\nb\nc", |line| line != "c", 0, MAX_GREP_LINES);
        assert_eq!(slice.lines, ["1:a", "2:b"]);
    }

    #[test]
    fn grep_is_bounded() {
        let script = "hit\n".repeat(MAX_GREP_LINES + 10);
        let slice = grep_lines(&script, |_| true, 0, MAX_GREP_LINES);
        assert_eq!(slice.lines.len(), MAX_GREP_LINES);
        assert!(slice.truncated);
        assert_eq!(slice.match_count, MAX_GREP_LINES + 10);
    }
}
