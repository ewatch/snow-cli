use super::*;

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
