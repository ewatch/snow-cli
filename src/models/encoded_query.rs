//! Field references inside a ServiceNow encoded query.
//!
//! ServiceNow silently ignores an encoded-query term whose field does not
//! exist (or is not readable), so `assigned_two.name=Abel Tuter` returns the
//! unfiltered result. [`field_paths`] extracts the field path of each term so
//! callers can verify them. It is a pure, best-effort parser: it never
//! rejects a query, and it skips anything it does not recognize.

/// Operator-free keywords that may start a term and never name a field.
const KEYWORD_PREFIXES: [&str; 5] = ["ORDERBYDESC", "ORDERBY", "GROUPBY", "EQ", "ENDRLQUERY"];

/// Distinct field paths referenced by an encoded query, in first-seen order.
///
/// Terms are split on `^` (which also covers `^OR` and `^NQ`); the leading
/// `OR`/`NQ` connector is removed. Sort/group clauses, `EQ`, and related-list
/// (`RLQUERY ... ENDRLQUERY`) blocks are skipped. The field path is the
/// leading run of `[a-z0-9_.]`, because ServiceNow field names are lowercase
/// and operators uppercase (`assignment_groupISNOTEMPTY`, `sys_idIN...`).
/// Terms that do not start with a lowercase letter (such as keyword search
/// `123TEXTQUERY321`) and extended-table dot-walks (`ref_<table>.field`) are
/// skipped rather than guessed at.
pub fn field_paths(query: &str) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    let mut in_related_list = false;
    for raw in query.split('^') {
        let term = raw.trim_start();
        let term = term
            .strip_prefix("NQ")
            .or_else(|| term.strip_prefix("OR"))
            .filter(|rest| rest.starts_with(|ch: char| ch.is_ascii_lowercase()))
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
        let end = term
            .find(|ch: char| {
                !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '.')
            })
            .unwrap_or(term.len());
        let path = term[..end].trim_end_matches('.');
        if !path.starts_with(|ch: char| ch.is_ascii_lowercase())
            || path
                .split('.')
                .any(|segment| segment.is_empty() || segment.starts_with("ref_"))
        {
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
    fn skips_sort_group_and_keyword_clauses() {
        assert_eq!(
            field_paths("active=true^ORDERBYDESCsys_created_on^ORDERBYnumber^GROUPBYstate^EQ"),
            ["active"]
        );
        assert_eq!(field_paths("123TEXTQUERY321=disk^active=true"), ["active"]);
        assert!(field_paths("").is_empty());
        assert!(field_paths("^^").is_empty());
    }

    #[test]
    fn skips_related_list_blocks_and_extended_dot_walks() {
        assert_eq!(
            field_paths("active=true^RLQUERYtask_sla.task,>=1^stage=breached^ENDRLQUERY^state=1"),
            ["active", "state"]
        );
        assert_eq!(
            field_paths("ref_incident.caller_id=abc^number=1"),
            ["number"]
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
        // A field that genuinely starts with "or"/"nq" in lowercase is kept intact.
        assert_eq!(field_paths("order=1^nqueue=2"), ["order", "nqueue"]);
    }
}
