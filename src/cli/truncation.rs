use crate::models::record::Record;
use serde::Serialize;

/// Per-field character cap used only when `--preview` is explicitly requested.
///
/// ServiceNow text fields (descriptions, work notes, scripts, widget bodies)
/// can be tens of kilobytes. Preview bounds agent-facing content without
/// changing lossless default output. The cap leaves ordinary short fields intact.
pub(crate) const DEFAULT_FIELD_CHAR_LIMIT: usize = 2000;

/// Machine-readable metadata for one shortened field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PreviewField {
    /// JSON Pointer to the shortened field in the emitted data shape.
    pub path: String,
    pub original_chars: usize,
    pub shown_chars: usize,
    pub escape_hatch: String,
}

/// Metadata for all fields shortened by a preview policy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(crate) struct PreviewReport {
    pub truncated_fields: Vec<PreviewField>,
}

impl PreviewReport {
    pub(crate) fn is_empty(&self) -> bool {
        self.truncated_fields.is_empty()
    }

    fn push(
        &mut self,
        path: String,
        original_chars: usize,
        shown_chars: usize,
        escape_hatch: &str,
    ) {
        self.truncated_fields.push(PreviewField {
            path,
            original_chars,
            shown_chars,
            escape_hatch: escape_hatch.to_string(),
        });
    }

    pub(crate) fn append(&mut self, mut other: Self) {
        self.truncated_fields.append(&mut other.truncated_fields);
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum RecordPreviewRoot {
    Records,
    Record,
}

fn escape_json_pointer_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

fn record_path(root: RecordPreviewRoot, record_index: usize, field: &str) -> String {
    let field = escape_json_pointer_segment(field);
    match root {
        RecordPreviewRoot::Records => format!("/records/{record_index}/{field}"),
        RecordPreviewRoot::Record => format!("/record/{field}"),
    }
}

fn preview_string(value: &str, limit: usize) -> Option<(String, usize)> {
    let total = value.chars().count();
    if total <= limit {
        return None;
    }
    let shortened = value.chars().take(limit).collect::<String>();
    Some((shortened, total))
}

fn preview_object_strings(
    object: &mut serde_json::Map<String, serde_json::Value>,
    path_prefix: &str,
    limit: usize,
    escape_hatch: &str,
) -> PreviewReport {
    let mut report = PreviewReport::default();
    let mut keys = object.keys().cloned().collect::<Vec<_>>();
    keys.sort();

    for key in keys {
        let Some(serde_json::Value::String(text)) = object.get_mut(&key) else {
            continue;
        };
        if let Some((shortened, total)) = preview_string(text, limit) {
            *text = shortened;
            report.push(
                format!("{path_prefix}/{}", escape_json_pointer_segment(&key)),
                total,
                limit,
                escape_hatch,
            );
        }
    }

    report
}

/// Apply the per-field character cap to top-level string fields of table
/// records. Non-string values and nested values stay lossless.
pub(crate) fn preview_record_fields(
    records: &mut [Record],
    limit: usize,
    root: RecordPreviewRoot,
    escape_hatch: &str,
) -> PreviewReport {
    let mut report = PreviewReport::default();

    for (record_index, record) in records.iter_mut().enumerate() {
        let mut keys = record.fields.keys().cloned().collect::<Vec<_>>();
        keys.sort();

        for key in keys {
            let Some(serde_json::Value::String(text)) = record.fields.get_mut(&key) else {
                continue;
            };
            if let Some((shortened, total)) = preview_string(text, limit) {
                *text = shortened;
                report.push(
                    record_path(root, record_index, &key),
                    total,
                    limit,
                    escape_hatch,
                );
            }
        }
    }

    report
}

/// Apply the preview cap to the known Code Search result shapes only:
/// `/result/<n>/<field>` and `/result/<group>/hits/<n>/<field>`.
/// Nested objects and arrays are left lossless.
pub(crate) fn preview_codesearch_fields(
    value: &mut serde_json::Value,
    limit: usize,
    escape_hatch: &str,
) -> PreviewReport {
    let mut report = PreviewReport::default();
    let Some(results) = value
        .get_mut("result")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return report;
    };

    for (result_index, result) in results.iter_mut().enumerate() {
        if let Some(hits) = result
            .get_mut("hits")
            .and_then(serde_json::Value::as_array_mut)
        {
            for (hit_index, hit) in hits.iter_mut().enumerate() {
                if let Some(hit_object) = hit.as_object_mut() {
                    report.append(preview_object_strings(
                        hit_object,
                        &format!("/result/{result_index}/hits/{hit_index}"),
                        limit,
                        escape_hatch,
                    ));
                }
            }
        } else if let Some(result_object) = result.as_object_mut() {
            report.append(preview_object_strings(
                result_object,
                &format!("/result/{result_index}"),
                limit,
                escape_hatch,
            ));
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn short_and_exact_limit_values_are_untouched() {
        assert_eq!(preview_string("hello", 5), None);
        assert_eq!(preview_string("", 5), None);
    }

    #[test]
    fn long_values_are_cut_without_inline_markers() {
        let (shortened, total) = preview_string(&"a".repeat(12), 10).unwrap();
        assert_eq!(shortened, "a".repeat(10));
        assert_eq!(total, 12);
        assert!(!shortened.contains("truncated"));
    }

    #[test]
    fn cut_respects_multibyte_char_boundaries() {
        let (shortened, total) = preview_string(&"é".repeat(8), 5).unwrap();
        assert_eq!(shortened, "é".repeat(5));
        assert_eq!(total, 8);
    }

    #[test]
    fn records_return_field_metadata_and_preserve_other_values() {
        let mut records = vec![
            Record {
                fields: HashMap::from([
                    ("number".to_string(), serde_json::json!("INC001")),
                    ("description".to_string(), serde_json::json!("x".repeat(30))),
                    ("tags".to_string(), serde_json::json!(["keep", "array"])),
                    (
                        "meta".to_string(),
                        serde_json::json!({"nested": "y".repeat(30)}),
                    ),
                    ("closed".to_string(), serde_json::json!(false)),
                    ("empty".to_string(), serde_json::Value::Null),
                ]),
            },
            Record {
                fields: HashMap::from([("number".to_string(), serde_json::json!("INC002"))]),
            },
        ];

        let report = preview_record_fields(&mut records, 10, RecordPreviewRoot::Records, "--full");

        assert_eq!(
            records[0].fields["description"],
            serde_json::json!("x".repeat(10))
        );
        assert_eq!(records[0].fields["number"], serde_json::json!("INC001"));
        assert_eq!(
            records[0].fields["tags"],
            serde_json::json!(["keep", "array"])
        );
        assert_eq!(
            records[0].fields["meta"],
            serde_json::json!({"nested": "y".repeat(30)})
        );
        assert_eq!(records[0].fields["closed"], serde_json::json!(false));
        assert_eq!(records[0].fields["empty"], serde_json::Value::Null);
        assert_eq!(
            report.truncated_fields,
            vec![PreviewField {
                path: "/records/0/description".to_string(),
                original_chars: 30,
                shown_chars: 10,
                escape_hatch: "--full".to_string(),
            }]
        );
    }

    #[test]
    fn single_record_paths_use_record_root() {
        let mut records = vec![Record {
            fields: HashMap::from([("work/notes".to_string(), serde_json::json!("x".repeat(12)))]),
        }];

        let report = preview_record_fields(&mut records, 3, RecordPreviewRoot::Record, "--full");

        assert_eq!(report.truncated_fields[0].path, "/record/work~1notes");
    }

    #[test]
    fn codesearch_preview_handles_flat_and_grouped_shapes_only() {
        let mut value = serde_json::json!({
            "result": [
                {"name": "Short", "match_text": "é".repeat(8), "nested": {"script": "x".repeat(20)}},
                {"recordType": "sys_script", "hits": [
                    {"name": "Hit", "script": "y".repeat(9), "lines": ["z".repeat(20)]}
                ]}
            ]
        });

        let report = preview_codesearch_fields(&mut value, 5, "--full");

        assert_eq!(
            value["result"][0]["match_text"],
            serde_json::json!("é".repeat(5))
        );
        assert_eq!(
            value["result"][0]["nested"]["script"],
            serde_json::json!("x".repeat(20))
        );
        assert_eq!(
            value["result"][1]["hits"][0]["script"],
            serde_json::json!("y".repeat(5))
        );
        assert_eq!(
            value["result"][1]["hits"][0]["lines"][0],
            serde_json::json!("z".repeat(20))
        );
        assert_eq!(report.truncated_fields.len(), 2);
        assert_eq!(report.truncated_fields[0].path, "/result/0/match_text");
        assert_eq!(report.truncated_fields[1].path, "/result/1/hits/0/script");
    }
}
