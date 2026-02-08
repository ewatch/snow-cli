use serde::{Deserialize, Serialize};

/// ServiceNow attachment metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sys_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_sys_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sys_created_on: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_attachment() {
        let json = r#"{
            "sys_id": "att001",
            "file_name": "report.pdf",
            "content_type": "application/pdf",
            "size_bytes": "102400",
            "table_name": "incident",
            "table_sys_id": "inc001"
        }"#;

        let attachment: Attachment = serde_json::from_str(json).unwrap();
        assert_eq!(attachment.sys_id, Some("att001".to_string()));
        assert_eq!(attachment.file_name, Some("report.pdf".to_string()));
    }
}
