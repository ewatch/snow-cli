use super::*;

#[test]
fn test_build_dictionary_query() {
    let query = build_dictionary_query(&["x_app_table", "x_app_second"]);
    assert_eq!(
        query,
        "nameINx_app_table,x_app_second^elementISNOTEMPTY^element!=sys_tags"
    );
}

#[test]
fn test_build_scope_search_query() {
    let global: EncodedQueryValue = "global".parse().unwrap();
    assert_eq!(
        build_scope_search_query(Some(&global)).unwrap(),
        Some("scope=global^ORsys_id=global^ORscopeLIKEglobal^ORnameLIKEglobal".to_string())
    );
    assert_eq!(build_scope_search_query(None).unwrap(), None);
    // Invalid characters are now rejected at construction time, before
    // `build_scope_search_query` is ever called.
    assert!("global^ORactive=true".parse::<EncodedQueryValue>().is_err());
}

#[test]
fn test_build_scope_list_rows_classifies_scope_origins() {
    let scopes = vec![
        record(&[
            ("sys_id", "scope-store"),
            ("scope", "sn_store_app"),
            ("name", "Store App"),
            ("version", "1.0.0"),
        ]),
        record(&[
            ("sys_id", "scope-custom"),
            ("scope", "x_acme_ops"),
            ("name", "Acme Ops"),
            ("version", "1.0.0"),
        ]),
        record(&[
            ("sys_id", "scope-platform"),
            ("scope", "global"),
            ("name", "Global"),
            ("version", ""),
        ]),
        record(&[
            ("sys_id", "scope-oob"),
            ("scope", "sn_ot_incident_mgmt"),
            ("name", "OT Incident Management"),
            ("version", "2.0.0"),
        ]),
    ];
    let store_apps = vec![record(&[
        ("sys_id", "store-1"),
        ("scope", "sn_store_app"),
        ("name", "Store App"),
        ("version", "1.0.0"),
    ])];
    let plugins = vec![record(&[
        ("sys_id", "plugin-1"),
        ("id", "com.snc.example"),
        ("name", "Example Plugin"),
    ])];

    let rows = build_scope_list_rows(scopes, store_apps, plugins);

    assert!(
        rows.iter()
            .any(|row| row.scope == "sn_store_app" && row.kind == "store_app")
    );
    assert!(
        rows.iter()
            .any(|row| row.scope == "x_acme_ops" && row.kind == "custom_app")
    );
    assert!(
        rows.iter()
            .any(|row| row.scope == "global" && row.kind == "platform")
    );
    assert!(
        rows.iter()
            .any(|row| row.scope == "sn_ot_incident_mgmt" && row.kind == "platform_app")
    );
    assert!(
        rows.iter()
            .any(|row| row.identifier == "com.snc.example" && row.kind == "plugin")
    );
}

#[test]
fn test_filter_scope_list_rows_by_kind() {
    let rows = vec![
        ScopeListRow {
            kind: "store_app".to_string(),
            scope: "sn_store_app".to_string(),
            name: "Store App".to_string(),
            version: "1.0.0".to_string(),
            identifier: String::new(),
            source_table: "sys_scope".to_string(),
            sys_id: "1".to_string(),
        },
        ScopeListRow {
            kind: "plugin".to_string(),
            scope: String::new(),
            name: "Plugin".to_string(),
            version: String::new(),
            identifier: "com.snc.example".to_string(),
            source_table: "v_plugin".to_string(),
            sys_id: "2".to_string(),
        },
    ];

    let filtered = filter_scope_list_rows(rows, &[ScopeListKind::Plugin]);

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].kind, "plugin");
}

#[test]
fn test_group_scope_rows_by_kind_preserves_display_order() {
    let rows = vec![
        ScopeListRow {
            kind: "plugin".to_string(),
            scope: String::new(),
            name: "Plugin".to_string(),
            version: String::new(),
            identifier: "com.snc.example".to_string(),
            source_table: "v_plugin".to_string(),
            sys_id: "2".to_string(),
        },
        ScopeListRow {
            kind: "custom_app".to_string(),
            scope: "x_acme_app".to_string(),
            name: "Acme App".to_string(),
            version: "1.0.0".to_string(),
            identifier: String::new(),
            source_table: "sys_scope".to_string(),
            sys_id: "1".to_string(),
        },
    ];

    let grouped = group_scope_rows_by_kind(&rows);

    assert_eq!(grouped.len(), 2);
    assert_eq!(grouped[0].0, "custom_app");
    assert_eq!(grouped[1].0, "plugin");
}

fn record(fields: &[(&str, &str)]) -> Record {
    Record {
        fields: fields
            .iter()
            .map(|(key, value)| (key.to_string(), serde_json::json!(value)))
            .collect(),
    }
}

#[test]
fn test_value_as_text_from_reference_object() {
    let value = serde_json::json!({"link": "https://example", "value": "abc123"});
    assert_eq!(value_as_text(&value), Some("abc123".to_string()));
}

#[test]
fn test_scope_summary_counts_by_category_and_artifact_type() {
    let rows = vec![
        ScopeInventoryRow {
            scope: "x_app".to_string(),
            scope_sys_id: "id".to_string(),
            category: "server_logic".to_string(),
            artifact_type: "script_includes".to_string(),
            source_table: "sys_script_include".to_string(),
            sys_id: "1".to_string(),
            name: "SI1".to_string(),
        },
        ScopeInventoryRow {
            scope: "x_app".to_string(),
            scope_sys_id: "id".to_string(),
            category: "other".to_string(),
            artifact_type: "other".to_string(),
            source_table: "x_custom_meta".to_string(),
            sys_id: "2".to_string(),
            name: "Meta".to_string(),
        },
    ];

    let summary = ScopeSummary::from_rows(&rows);
    assert_eq!(summary.total_artifacts, 2);
    assert_eq!(summary.category_counts.get("server_logic"), Some(&1));
    assert_eq!(summary.category_counts.get("other"), Some(&1));
    assert_eq!(summary.artifact_counts.get("script_includes"), Some(&1));
    assert_eq!(summary.artifact_counts.get("other"), Some(&1));
}

#[test]
fn test_build_move_file_script_contains_inputs() {
    let script =
        build_move_file_script("sys_script_include", "abc123", "x_target_app", true, false)
            .unwrap();

    assert!(script.contains("sys_script_include"));
    assert!(script.contains("abc123"));
    assert!(script.contains("x_target_app"));
    assert!(script.contains("\"dryRun\":true"));
    assert!(script.contains("\"force\":false"));
    assert!(script.contains("sys_update_name"));
    assert!(script.contains("api_name"));
}

#[test]
fn test_build_move_file_script_limits_records_to_sys_metadata_tables() {
    let script =
        build_move_file_script("sys_script_include", "abc123", "x_target_app", true, false)
            .unwrap();

    assert!(script.contains("tableExtends(input.table, 'sys_metadata')"));
    assert!(script.contains("Unsupported record: table must extend sys_metadata"));
    assert!(script.contains("new GlideRecord('sys_db_object')"));
    assert!(script.contains("getValue('super_class')"));
}
