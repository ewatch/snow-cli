use crate::models::record::Record;

/// Default per-field character cap applied to `table list` and `table get`
/// output when `--full` is omitted.
///
/// ServiceNow text fields (descriptions, work notes, scripts, widget bodies)
/// can be tens of kilobytes; a bounded default keeps record output safe to
/// read in agent contexts the same way the bounded list limit does for row
/// counts. The cap is generous enough that ordinary short_description-style
/// fields are never touched.
pub(crate) const DEFAULT_FIELD_CHAR_LIMIT: usize = 2000;

/// Marker appended to a truncated field value. Carries a size hint (shown vs
/// total characters) and the escape hatch, so a consumer can always tell the
/// value is incomplete and knows how to get the rest.
fn truncation_marker(shown: usize, total: usize) -> String {
    format!("… [truncated {shown} of {total} chars; use --full]")
}

/// Truncate a single string to `limit` characters, appending a size-hint
/// marker. Returns `None` when the value already fits.
///
/// The cut respects UTF-8 character boundaries: `limit` counts characters,
/// not bytes, so multi-byte content is never split mid-character.
fn truncate_str(value: &str, limit: usize) -> Option<String> {
    let cut = value.char_indices().nth(limit)?.0;
    let total = value.chars().count();
    let mut shortened = value[..cut].to_string();
    shortened.push_str(&truncation_marker(limit, total));
    Some(shortened)
}

/// Apply the per-field character cap to every string field of every record.
///
/// Returns `true` when at least one field value was shortened, so callers can
/// surface a `fields_truncated` signal in result metadata. Non-string values
/// (numbers, booleans, nested objects such as reference link pairs) are left
/// untouched: the oversized payloads in practice are always plain strings.
pub(crate) fn truncate_record_fields(records: &mut [Record], limit: usize) -> bool {
    let mut any_truncated = false;
    for record in records {
        for value in record.fields.values_mut() {
            if let serde_json::Value::String(text) = value
                && let Some(shortened) = truncate_str(text, limit)
            {
                *text = shortened;
                any_truncated = true;
            }
        }
    }
    any_truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn short_values_are_untouched() {
        assert_eq!(truncate_str("hello", 5), None);
        assert_eq!(truncate_str("", 5), None);
    }

    #[test]
    fn long_values_are_cut_with_size_hint() {
        let value = "a".repeat(12);
        let shortened = truncate_str(&value, 10).unwrap();
        assert!(shortened.starts_with(&"a".repeat(10)));
        assert!(shortened.contains("truncated 10 of 12 chars"));
        assert!(shortened.contains("--full"));
    }

    #[test]
    fn cut_respects_multibyte_char_boundaries() {
        let value = "é".repeat(8);
        let shortened = truncate_str(&value, 5).unwrap();
        assert!(shortened.starts_with(&"é".repeat(5)));
        assert!(!shortened.starts_with(&"é".repeat(6)));
        assert!(shortened.contains("truncated 5 of 8 chars"));
    }

    #[test]
    fn records_report_whether_anything_was_truncated() {
        let mut records = vec![
            Record {
                fields: HashMap::from([
                    ("number".to_string(), serde_json::json!("INC001")),
                    ("description".to_string(), serde_json::json!("x".repeat(30))),
                ]),
            },
            Record {
                fields: HashMap::from([("number".to_string(), serde_json::json!("INC002"))]),
            },
        ];

        assert!(truncate_record_fields(&mut records, 10));

        let description = records[0].fields["description"].as_str().unwrap();
        assert!(description.contains("truncated 10 of 30 chars"));
        assert_eq!(records[0].fields["number"], serde_json::json!("INC001"));
        assert_eq!(records[1].fields["number"], serde_json::json!("INC002"));
    }

    #[test]
    fn records_within_limit_are_not_flagged() {
        let mut records = vec![Record {
            fields: HashMap::from([
                ("number".to_string(), serde_json::json!("INC001")),
                ("count".to_string(), serde_json::json!(42)),
            ]),
        }];

        assert!(!truncate_record_fields(&mut records, 10));
        assert_eq!(records[0].fields["number"], serde_json::json!("INC001"));
    }

    #[test]
    fn non_string_values_are_ignored() {
        let mut records = vec![Record {
            fields: HashMap::from([(
                "assigned_to".to_string(),
                serde_json::json!({"link": "https://example.service-now.com/very/long/link/that/would/exceed/any/limit/if/it/were/a/string", "value": "abc"}),
            )]),
        }];

        assert!(!truncate_record_fields(&mut records, 10));
    }
}
