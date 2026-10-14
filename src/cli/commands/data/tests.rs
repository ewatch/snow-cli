use super::*;

#[test]
fn import_ready_preparation_reports_omissions_and_updates_declared_fields() {
    let mut records: Vec<Record> = serde_json::from_value(serde_json::json!([
        {"name": "first", "sys_id": "id", "unknown": "private"},
        {"name": "second", "sys_id": "other-id"}
    ]))
    .unwrap();
    let schema = vec![SchemaField {
        name: "name".to_string(),
        internal_type: "string".to_string(),
        mandatory: false,
        read_only: false,
        default_value: None,
    }];
    let mut fields = Some(vec![
        "*".to_string(),
        "name".to_string(),
        "sys_id".to_string(),
    ]);
    let preparation = prepare_import_ready_export(&mut records, &mut fields, &schema).unwrap();
    assert_eq!(fields, Some(vec!["name".to_string()]));
    assert_eq!(preparation.omitted_fields[0].field, "sys_id");
    assert_eq!(preparation.omitted_fields[0].affected_records, 2);
    assert_eq!(preparation.omitted_fields[1].affected_records, 1);
    assert_eq!(preparation.omitted_fields[1].reason, "unknown_field");
    assert_eq!(
        serde_json::to_value(records).unwrap(),
        serde_json::json!([{"name": "first"}, {"name": "second"}])
    );
    assert!(
        !serde_json::to_string(&preparation)
            .unwrap()
            .contains("private")
    );
}

#[test]
fn import_ready_preparation_requires_readable_schema_even_for_empty_exports() {
    let mut records = Vec::new();
    let mut fields = None;
    let error = prepare_import_ready_export(&mut records, &mut fields, &[]).unwrap_err();
    assert!(
        error
            .downcast_ref::<crate::models::validation::ValidationError>()
            .is_some()
    );
    assert!(fields.is_none());
}

#[test]
fn test_validate_package_file_name_rejects_traversal_and_absolute_paths() {
    assert!(validate_package_file_name("incident.json").is_ok());
    assert!(validate_package_file_name("../secret.json").is_err());
    assert!(validate_package_file_name("nested/incident.json").is_err());
    assert!(validate_package_file_name("/tmp/incident.json").is_err());
    assert!(validate_package_file_name("").is_err());
}

#[test]
fn test_split_csv_fields_none() {
    assert_eq!(split_csv_fields(None), None);
}

#[test]
fn test_split_csv_fields_trims_values() {
    assert_eq!(
        split_csv_fields(Some("sys_id, number, short_description")),
        Some(vec![
            "sys_id".to_string(),
            "number".to_string(),
            "short_description".to_string(),
        ])
    );
}

#[test]
fn test_json_value_as_text_from_reference_object() {
    let value = serde_json::json!({
        "link": "https://example.com/api/now/table/sys_glide_object?name=integer",
        "value": "integer"
    });

    assert_eq!(json_value_as_text(&value), Some("integer".to_string()));
}

#[test]
fn test_json_value_as_bool_from_string() {
    assert!(json_value_as_bool(&serde_json::json!("true")));
    assert!(!json_value_as_bool(&serde_json::json!("false")));
}
