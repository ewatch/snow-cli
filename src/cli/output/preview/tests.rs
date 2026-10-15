use super::super::write_toon;
use super::*;
use std::collections::BTreeMap;

#[test]
fn ordinary_envelopes_use_canonical_serialization() {
    let result = sample_list_result(Some(10), true);
    let preview = PreviewReport::default();
    let envelope = TableListEnvelope {
        meta: TableListMeta::from_result(&result, &preview),
        preview: &preview,
        records: &result.records,
    };
    for format in [
        OutputFormat::Json,
        OutputFormat::Toon,
        OutputFormat::Text,
        OutputFormat::Auto,
    ] {
        let mut framed = Vec::new();
        let mut canonical = Vec::new();
        write_table_list_with_preview(&result, &format, &preview, &mut framed).unwrap();
        write_output(&envelope, &format, &mut canonical).unwrap();
        assert_eq!(framed, canonical, "{}", format.as_str());
    }
}

#[test]
fn preview_compatibility_errors_are_typed() {
    let error = ensure_preview_supported(true, &OutputFormat::Csv).unwrap_err();
    assert!(
        error
            .downcast_ref::<crate::models::validation::ValidationError>()
            .is_some()
    );
    ensure_preview_supported(false, &OutputFormat::Csv).unwrap();
    for format in [
        OutputFormat::Json,
        OutputFormat::Jsonl,
        OutputFormat::Toon,
        OutputFormat::Text,
        OutputFormat::Auto,
    ] {
        ensure_preview_supported(true, &format).unwrap();
    }
}
fn sample_list_result(total: Option<usize>, truncated: bool) -> TableListResult {
    TableListResult {
        records: vec![
            Record {
                fields: BTreeMap::from([
                    ("sys_id".to_string(), serde_json::json!("abc123")),
                    ("number".to_string(), serde_json::json!("INC001")),
                ]),
            },
            Record {
                fields: BTreeMap::from([
                    ("sys_id".to_string(), serde_json::json!("def456")),
                    ("number".to_string(), serde_json::json!("INC002")),
                ]),
            },
        ],
        total,
        offset: 0,
        truncated,
        fields_truncated: false,
    }
}

#[test]
fn test_write_table_list_json_envelope() {
    let result = sample_list_result(Some(4381), true);

    let mut output = Vec::new();
    write_table_list_with_preview(
        &result,
        &OutputFormat::Json,
        &PreviewReport::default(),
        &mut output,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(value["total"], 4381);
    assert_eq!(value["returned"], 2);
    assert_eq!(value["truncated"], true);
    assert_eq!(value["records"].as_array().unwrap().len(), 2);
    assert_eq!(value["records"][0]["number"], "INC001");
}

#[test]
fn test_write_table_list_json_omits_unknown_total() {
    let result = sample_list_result(None, false);

    let mut output = Vec::new();
    write_table_list_with_preview(
        &result,
        &OutputFormat::Json,
        &PreviewReport::default(),
        &mut output,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();

    assert!(value.get("total").is_none());
    assert_eq!(value["returned"], 2);
    assert_eq!(value["truncated"], false);
}

#[test]
fn test_write_table_list_fields_truncated_flag() {
    // Omitted entirely when false…
    let result = sample_list_result(Some(10), false);
    let mut output = Vec::new();
    write_table_list_with_preview(
        &result,
        &OutputFormat::Json,
        &PreviewReport::default(),
        &mut output,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(value.get("fields_truncated").is_none());

    // …and surfaced when the content cap shortened field values.
    let mut result = sample_list_result(Some(10), false);
    result.fields_truncated = true;
    let mut output = Vec::new();
    write_table_list_with_preview(
        &result,
        &OutputFormat::Json,
        &PreviewReport::default(),
        &mut output,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["fields_truncated"], true);
}

#[test]
fn test_write_table_list_preview_metadata_is_outside_records() {
    let result = sample_list_result(Some(10), false);
    let preview = PreviewReport {
        truncated_fields: vec![crate::cli::truncation::PreviewField {
            path: "/records/0/description".to_string(),
            original_chars: 2001,
            shown_chars: 2000,
            escape_hatch: "rerun this command with --full".to_string(),
        }],
    };
    let mut output = Vec::new();
    write_table_list_with_preview(&result, &OutputFormat::Json, &preview, &mut output).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(value["fields_truncated"], true);
    assert_eq!(
        value["preview"]["truncated_fields"][0]["path"],
        "/records/0/description"
    );
    assert!(value["records"][0].get("preview").is_none());
}

#[test]
fn test_write_table_list_jsonl_preview_uses_single_envelope_line() {
    let result = sample_list_result(Some(10), true);
    let preview = PreviewReport {
        truncated_fields: vec![crate::cli::truncation::PreviewField {
            path: "/records/1/script".to_string(),
            original_chars: 3000,
            shown_chars: 2000,
            escape_hatch: "rerun this command with --full".to_string(),
        }],
    };
    let mut output = Vec::new();
    write_table_list_with_preview(&result, &OutputFormat::Jsonl, &preview, &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    let lines: Vec<&str> = text.trim().split('\n').collect();
    let envelope: serde_json::Value = serde_json::from_str(lines[0]).unwrap();

    assert_eq!(lines.len(), 1);
    assert_eq!(
        envelope["preview"]["truncated_fields"][0]["shown_chars"],
        2000
    );
    assert_eq!(envelope["records"][0]["number"], "INC001");
}

#[test]
fn test_write_record_preview_toon_includes_metadata_and_record() {
    let record = Record {
        fields: BTreeMap::from([(
            "description".to_string(),
            serde_json::json!("x".repeat(2000)),
        )]),
    };
    let preview = PreviewReport {
        truncated_fields: vec![crate::cli::truncation::PreviewField {
            path: "/record/description".to_string(),
            original_chars: 2001,
            shown_chars: 2000,
            escape_hatch: "rerun this command with --full".to_string(),
        }],
    };
    let envelope = RecordPreviewEnvelope {
        preview: &preview,
        record: &record,
    };
    let mut output = Vec::new();
    write_toon(&envelope, &mut output).unwrap();
    let toon = String::from_utf8(output).unwrap();

    assert!(toon.contains("preview"));
    assert!(toon.contains("record"));
    assert!(toon.contains("/record/description"));
}

#[test]
fn test_write_table_list_jsonl_meta_line_first() {
    let result = sample_list_result(Some(10), true);

    let mut output = Vec::new();
    write_table_list_with_preview(
        &result,
        &OutputFormat::Jsonl,
        &PreviewReport::default(),
        &mut output,
    )
    .unwrap();
    let text = String::from_utf8(output).unwrap();
    let lines: Vec<&str> = text.trim().split('\n').collect();

    assert_eq!(lines.len(), 3);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(lines[0]).unwrap(),
        serde_json::json!({"meta": {"total": 10, "returned": 2, "offset": 0, "next_offset": 2, "truncated": true}})
    );
    let first: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(first["number"], "INC001");
}

#[test]
fn test_write_table_list_csv_stays_plain_records() {
    let result = sample_list_result(Some(10), true);

    let mut output = Vec::new();
    write_table_list_with_preview(
        &result,
        &OutputFormat::Csv,
        &PreviewReport::default(),
        &mut output,
    )
    .unwrap();
    let csv_str = String::from_utf8(output).unwrap();

    assert!(csv_str.starts_with("number,sys_id\n"));
    assert!(!csv_str.contains("truncated"));
    assert!(!csv_str.contains("total"));
}

#[test]
fn test_write_table_list_toon_includes_meta() {
    let result = sample_list_result(Some(10), true);

    let mut output = Vec::new();
    write_table_list_with_preview(
        &result,
        &OutputFormat::Toon,
        &PreviewReport::default(),
        &mut output,
    )
    .unwrap();
    let toon = String::from_utf8(output).unwrap();

    assert!(toon.contains("total"));
    assert!(toon.contains("returned"));
    assert!(toon.contains("truncated"));
    assert!(toon.contains("INC001"));
}

#[test]
fn test_write_table_list_auto_keeps_meta() {
    let result = sample_list_result(Some(10), true);

    let mut output = Vec::new();
    write_table_list_with_preview(
        &result,
        &OutputFormat::Auto,
        &PreviewReport::default(),
        &mut output,
    )
    .unwrap();
    let text = String::from_utf8(output).unwrap();

    // Whichever encoding wins, the metadata must survive.
    assert!(text.contains("total"));
    assert!(text.contains("returned"));
    assert!(text.contains("truncated"));
    assert!(text.contains("INC001"));
}

#[test]
fn test_write_table_list_empty_is_definitive() {
    let result = TableListResult {
        records: Vec::new(),
        total: Some(0),
        offset: 0,
        truncated: false,
        fields_truncated: false,
    };

    let mut output = Vec::new();
    write_table_list_with_preview(
        &result,
        &OutputFormat::Json,
        &PreviewReport::default(),
        &mut output,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(value["total"], 0);
    assert_eq!(value["returned"], 0);
    assert_eq!(value["truncated"], false);
    assert_eq!(value["records"], serde_json::json!([]));
}
