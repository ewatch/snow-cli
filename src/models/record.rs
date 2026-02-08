use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Generic ServiceNow table record.
///
/// ServiceNow records are dynamic — different tables have different fields.
/// This type represents a record as a map of field names to values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    #[serde(flatten)]
    pub fields: HashMap<String, serde_json::Value>,
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

impl Record {
    /// Get a field value as a string.
    pub fn get_str(&self, field: &str) -> Option<&str> {
        self.fields.get(field).and_then(|v| v.as_str())
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
    fn test_record_serialization_is_flat() {
        let mut fields = HashMap::new();
        fields.insert("sys_id".to_string(), serde_json::json!("abc123"));
        fields.insert("number".to_string(), serde_json::json!("INC0010001"));

        let record = Record { fields };
        let json = serde_json::to_string(&record).unwrap();

        // Flat serialization — no "fields" wrapper
        assert!(json.contains("\"sys_id\":\"abc123\""));
        assert!(!json.contains("\"fields\""));
    }
}
