//! Field references inside a ServiceNow encoded query.
//!
//! ServiceNow silently ignores an encoded-query term whose field does not
//! exist (or is not readable), so `assigned_two.name=Abel Tuter` returns the
//! unfiltered result. [`field_paths`] extracts the field path of each term so
//! callers can verify them. It is a pure, best-effort parser: it never
//! rejects a query, and it skips anything it does not recognize.

/// Operator-free keywords that may start a term and never name a field.
const KEYWORD_PREFIXES: [&str; 2] = ["EQ", "ENDRLQUERY"];

/// Clauses whose entire suffix after the keyword is a field path (no operator).
const FIELD_KEYWORD_PREFIXES: [&str; 3] = ["ORDERBYDESC", "ORDERBY", "GROUPBY"];

/// Split an encoded query on unescaped `^` separators.
///
/// ServiceNow escapes a literal `^` inside a query value as `^^`, so consecutive
/// pairs stay inside the current term while a single `^` starts a new term.
fn split_encoded_terms(query: &str) -> Vec<&str> {
    let bytes = query.as_bytes();
    let mut terms = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'^' {
            if bytes.get(i + 1) == Some(&b'^') {
                i += 2;
                continue;
            }
            terms.push(&query[start..i]);
            i += 1;
            start = i;
        } else {
            i += 1;
        }
    }
    terms.push(&query[start..]);
    terms
}

/// Whether `path` is a valid checkable field path on the primary table.
///
/// Rejects empty segments and extended-table casts (`ref_<table>.<field>`)
/// while allowing top-level or terminal columns whose own name starts with
/// `ref_` (such as `ref_number`).
fn is_checkable_field_path(path: &str) -> bool {
    if !path.starts_with(|ch: char| ch.is_ascii_lowercase()) {
        return false;
    }
    let segments: Vec<&str> = path.split('.').collect();
    if segments.iter().any(|segment| segment.is_empty()) {
        return false;
    }
    !segments[..segments.len().saturating_sub(1)]
        .iter()
        .any(|segment| segment.starts_with("ref_"))
}

/// Distinct field paths referenced by an encoded query, in first-seen order.
///
/// Terms are split on unescaped `^` (preserving `^^` literal carets inside
/// values); leading `OR`/`NQ` connectors are removed before field names,
/// `RLQUERY`, and `GOTO`. Related-list (`RLQUERY ... ENDRLQUERY`) blocks and
/// `EQ` are skipped. Embedded `ORDERBY`/`ORDERBYDESC`/`GROUPBY` clauses and
/// `GOTO` predicates have their field paths extracted. For normal predicates,
/// the field path is the leading run of `[a-z0-9_.]` followed by a ServiceNow
/// operator (`=`, `!`, `<`, `>`, or an uppercase ASCII operator keyword such as
/// `LIKE`, `IN`, `ISNOTEMPTY`). Terms that do not start with a lowercase letter
/// (such as keyword search `123TEXTQUERY321`), operator-less fragments, and
/// extended-table dot-walks (`ref_<table>.<field>`) are skipped rather than
/// guessed at.
pub fn field_paths(query: &str) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    let mut in_related_list = false;
    for raw in split_encoded_terms(query) {
        let term = raw.trim_start();
        let term = term
            .strip_prefix("NQ")
            .or_else(|| term.strip_prefix("OR"))
            .filter(|rest| {
                rest.starts_with(|ch: char| ch.is_ascii_lowercase())
                    || rest.starts_with("RLQUERY")
                    || rest.starts_with("GOTO")
            })
            .unwrap_or(term);
        if term.starts_with("RLQUERY") {
            in_related_list = true;
            continue;
        }
        if term.starts_with("ENDRLQUERY") {
            in_related_list = false;
            continue;
        }
        if in_related_list
            || term.is_empty()
            || KEYWORD_PREFIXES
                .iter()
                .any(|prefix| term.starts_with(prefix))
        {
            continue;
        }
        if let Some(rest) = FIELD_KEYWORD_PREFIXES
            .iter()
            .find_map(|prefix| term.strip_prefix(prefix))
        {
            let path = rest.trim_end();
            if path
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '.')
                && is_checkable_field_path(path)
                && !paths.iter().any(|existing| existing == path)
            {
                paths.push(path.to_string());
            }
            continue;
        }
        let term = term
            .strip_prefix("GOTO")
            .filter(|rest| rest.starts_with(|ch: char| ch.is_ascii_lowercase()))
            .unwrap_or(term);
        let Some(end) = term.find(|ch: char| {
            !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '.')
        }) else {
            continue;
        };
        let rest = &term[end..];
        if !matches!(
            rest.as_bytes().first(),
            Some(b'=' | b'!' | b'<' | b'>' | b'A'..=b'Z')
        ) {
            continue;
        }
        let path = term[..end].trim_end_matches('.');
        if !is_checkable_field_path(path) {
            continue;
        }
        if !paths.iter().any(|existing| existing == path) {
            paths.push(path.to_string());
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_fields_across_operators_and_connectors() {
        assert_eq!(
            field_paths("active=true^assigned_two.name=Abel Tuter^ORpriority<=2^NQstate!=7"),
            ["active", "assigned_two.name", "priority", "state"]
        );
        assert_eq!(
            field_paths("assignment_groupISNOTEMPTY^sys_idINa,b^short_descriptionLIKEdisk"),
            ["assignment_group", "sys_id", "short_description"]
        );
    }

    #[test]
    fn extracts_sort_group_goto_and_skips_non_field_keywords() {
        assert_eq!(
            field_paths(
                "active=true^ORDERBYDESCsys_created_on^ORDERBYnumber^GROUPBYstate^GOTOpriority>=2^EQ"
            ),
            ["active", "sys_created_on", "number", "state", "priority"]
        );
        assert_eq!(
            field_paths("active=true^NQGOTOcaller_id.nameLIKEAbel^ORDERBYassignment_group.name"),
            ["active", "caller_id.name", "assignment_group.name"]
        );
        assert_eq!(field_paths("123TEXTQUERY321=disk^active=true"), ["active"]);
        assert!(field_paths("").is_empty());
        assert!(field_paths("^^").is_empty());
    }

    #[test]
    fn handles_escaped_carets_and_ignores_operatorless_fragments() {
        assert_eq!(
            field_paths("short_descriptionLIKEa^^b^active=true"),
            ["short_description", "active"]
        );
        assert_eq!(
            field_paths("short_description=foo^^^state=1"),
            ["short_description", "state"]
        );
        assert!(field_paths("just_a_word").is_empty());
        assert!(field_paths("foo-bar^baz qux").is_empty());
        assert_eq!(field_paths("foo^active=true^bar"), ["active"]);
    }

    #[test]
    fn skips_related_list_blocks_including_or_and_nq_connectors() {
        assert_eq!(
            field_paths("active=true^RLQUERYtask_sla.task,>=1^stage=breached^ENDRLQUERY^state=1"),
            ["active", "state"]
        );
        assert_eq!(
            field_paths(
                "active=true^ORRLQUERYtask_sla.task,>=1^stage=breached^ENDRLQUERY^NQRLQUERYincident_task.incident,>=1^short_descriptionLIKEx^ENDRLQUERY^priority=1"
            ),
            ["active", "priority"]
        );
    }

    #[test]
    fn skips_extended_ref_casts_but_keeps_top_level_and_terminal_ref_columns() {
        assert_eq!(
            field_paths(
                "ref_incident.caller_id=abc^parent.ref_incident.caller_id=def^ref_number=123^caller_id.ref_code=xyz^number=1"
            ),
            ["ref_number", "caller_id.ref_code", "number"]
        );
        assert_eq!(
            field_paths("ORDERBYref_incident.caller_id^ORDERBYref_number"),
            ["ref_number"]
        );
    }

    #[test]
    fn ignores_javascript_values_and_deduplicates() {
        assert_eq!(
            field_paths("assigned_to=javascript:gs.getUserID()^ORassigned_to=abc"),
            ["assigned_to"]
        );
    }

    #[test]
    fn keeps_lowercase_field_after_or_and_nq_prefixes_only() {
        // A field that genuinely starts with "or"/"nq"/"goto" in lowercase is kept intact.
        assert_eq!(
            field_paths("order=1^nqueue=2^goto_target=3"),
            ["order", "nqueue", "goto_target"]
        );
    }
}
