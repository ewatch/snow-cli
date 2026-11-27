use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Generic ServiceNow table record.
///
/// ServiceNow records are dynamic — different tables have different fields.
/// This type represents a record as a map of field names to values. Fields are
/// kept sorted so output key order is stable across runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    #[serde(flatten)]
    pub fields: BTreeMap<String, serde_json::Value>,
}

/// ServiceNow Table API response wrapper.
#[derive(Debug, Deserialize)]
pub struct TableResponse {
    pub result: Vec<Record>,
}

/// ServiceNow single-record API response wrapper.
#[derive(Debug, Deserialize)]
pub struct SingleRecordResponse {
    pub result: Record,
}

impl AsRef<Record> for Record {
    fn as_ref(&self) -> &Record {
        self
    }
}

impl Record {
    /// Get a field value as a string.
    pub fn get_str(&self, field: &str) -> Option<&str> {
        self.fields.get(field).and_then(|v| v.as_str())
    }

    /// Get a reference field's sys_id, whether the Table API returned it as a
    /// plain string or as a `{"link": ..., "value": ...}` object. An empty
    /// reference is `None`.
    pub fn reference_value(&self, field: &str) -> Option<&str> {
        match self.fields.get(field)? {
            serde_json::Value::String(value) => Some(value.as_str()),
            serde_json::Value::Object(object) => {
                object.get("value").and_then(serde_json::Value::as_str)
            }
            _ => None,
        }
        .filter(|value| !value.is_empty())
    }

    /// Get the sys_id of this record.
    pub fn sys_id(&self) -> Option<&str> {
        self.get_str("sys_id")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_table_response() {
        let json = r#"{
            "result": [
                {"sys_id": "abc123", "number": "INC0010001", "short_description": "Test"},
                {"sys_id": "def456", "number": "INC0010002", "short_description": "Test 2"}
            ]
        }"#;

        let response: TableResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.result.len(), 2);
        assert_eq!(response.result[0].sys_id(), Some("abc123"));
        assert_eq!(response.result[0].get_str("number"), Some("INC0010001"));
    }

    #[test]
    fn reference_value_reads_plain_and_link_objects() {
        let record: Record = serde_json::from_value(serde_json::json!({
            "plain": "abc",
            "link": {"value": "def", "link": "https://x"},
            "empty": "",
            "empty_link": {"value": "", "link": "https://x"},
            "number": 7,
        }))
        .unwrap();
        assert_eq!(record.reference_value("plain"), Some("abc"));
        assert_eq!(record.reference_value("link"), Some("def"));
        assert_eq!(record.reference_value("empty"), None);
        assert_eq!(record.reference_value("empty_link"), None);
        assert_eq!(record.reference_value("number"), None);
        assert_eq!(record.reference_value("missing"), None);
    }

    #[test]
    fn test_record_serialization_is_flat() {
        let mut fields = BTreeMap::new();
        fields.insert("sys_id".to_string(), serde_json::json!("abc123"));
        fields.insert("number".to_string(), serde_json::json!("INC0010001"));

        let record = Record { fields };
        let json = serde_json::to_string(&record).unwrap();

        // Flat serialization — no "fields" wrapper
        assert!(json.contains("\"sys_id\":\"abc123\""));
        assert!(!json.contains("\"fields\""));
    }

    #[test]
    fn test_record_keys_serialize_in_stable_sorted_order() {
        let json = r#"{"sys_id": "a", "state": "1", "number": "INC1", "priority": "3"}"#;
        let record: Record = serde_json::from_str(json).unwrap();
        assert_eq!(
            serde_json::to_string(&record).unwrap(),
            r#"{"number":"INC1","priority":"3","state":"1","sys_id":"a"}"#
        );
    }
}
