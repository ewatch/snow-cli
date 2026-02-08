use serde::{Deserialize, Serialize};

/// Incident record with well-known fields.
///
/// This is a typed view over the generic Record for convenience.
/// The Table API commands use the generic Record type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sys_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub urgency: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_to: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment_group: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub subcategory: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub opened_at: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_notes: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_incident() {
        let json = r#"{
            "sys_id": "abc123",
            "number": "INC0010001",
            "short_description": "Cannot login",
            "state": "1",
            "priority": "2"
        }"#;

        let incident: Incident = serde_json::from_str(json).unwrap();
        assert_eq!(incident.sys_id, Some("abc123".to_string()));
        assert_eq!(incident.number, Some("INC0010001".to_string()));
        assert_eq!(incident.short_description, Some("Cannot login".to_string()));
    }

    #[test]
    fn test_serialize_skips_none_fields() {
        let incident = Incident {
            sys_id: Some("abc123".to_string()),
            number: None,
            short_description: Some("Test".to_string()),
            description: None,
            state: None,
            priority: None,
            urgency: None,
            impact: None,
            assigned_to: None,
            assignment_group: None,
            category: None,
            subcategory: None,
            caller_id: None,
            opened_at: None,
            resolved_at: None,
            close_notes: None,
        };
        let json = serde_json::to_string(&incident).unwrap();
        assert!(!json.contains("number"));
        assert!(json.contains("sys_id"));
        assert!(json.contains("short_description"));
    }
}
