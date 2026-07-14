use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct TableExportArtifact {
    pub(super) version: u8,
    pub(super) kind: String,
    #[serde(default = "default_export_command")]
    pub(super) command: String,
    pub(super) instance: String,
    pub(super) table: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) fields: Option<Vec<String>>,
    #[serde(default)]
    pub(super) exported_at_unix_s: u64,
    #[serde(default)]
    pub(super) record_count: usize,
    pub(super) records: Vec<Record>,
}

#[derive(Debug, Serialize)]
pub(super) struct ExportSummary {
    pub(super) kind: &'static str,
    pub(super) command: &'static str,
    pub(super) output_format: &'static str,
    pub(super) instance: String,
    pub(super) table: String,
    pub(super) record_count: usize,
    pub(super) out_path: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ValidationReport {
    pub(super) kind: &'static str,
    pub(super) command: &'static str,
    pub(super) dataset_kind: String,
    pub(super) table: String,
    pub(super) ready: bool,
    pub(super) record_count: usize,
    pub(super) field_count: usize,
    pub(super) errors: Vec<ValidationIssue>,
    pub(super) warnings: Vec<ValidationIssue>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ValidationIssue {
    pub(super) kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) record_index: Option<usize>,
    pub(super) message: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ImportReport {
    pub(super) kind: &'static str,
    pub(super) command: &'static str,
    pub(super) strategy: &'static str,
    pub(super) strategy_reason: &'static str,
    pub(super) table: String,
    pub(super) record_count: usize,
    pub(super) created: usize,
    pub(super) failed: usize,
    pub(super) skipped: usize,
    pub(super) validation: ImportValidationSummary,
    pub(super) failures: Vec<ImportFailure>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ImportSetApiResponse {
    #[serde(default)]
    pub(super) result: Vec<ImportSetApiResult>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ImportSetApiResult {
    #[serde(default)]
    pub(super) status: Option<String>,
    #[serde(default)]
    pub(super) error_message: Option<String>,
    #[serde(default)]
    pub(super) status_message: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ImportExecutionOptions<'a> {
    pub(super) dry_run: bool,
    pub(super) import_set_table: Option<&'a TableName>,
    pub(super) fail_on_error: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct ImportValidationSummary {
    pub(super) ready: bool,
    pub(super) error_count: usize,
    pub(super) warning_count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct ImportFailure {
    pub(super) record_index: usize,
    pub(super) message: String,
}

#[derive(Debug, Clone)]
pub(super) struct SchemaField {
    pub(super) name: String,
    pub(super) internal_type: String,
    pub(super) mandatory: bool,
    pub(super) read_only: bool,
    pub(super) default_value: Option<String>,
}

#[derive(Debug)]
pub(super) struct TableDefinition {
    pub(super) name: String,
    pub(super) super_class_sys_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct DatasetExportSpec {
    pub(super) version: u8,
    pub(super) kind: String,
    pub(super) tables: Vec<DatasetTableSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DatasetTableSpec {
    pub(super) name: String,
    #[serde(default)]
    pub(super) file: Option<String>,
    #[serde(default)]
    pub(super) query: Option<String>,
    #[serde(default)]
    pub(super) fields: Option<Vec<String>>,
    #[serde(default)]
    pub(super) limit: Option<usize>,
    #[serde(default)]
    pub(super) order_by: Option<String>,
    #[serde(default)]
    pub(super) depends_on: Vec<String>,
    #[serde(default)]
    pub(super) references: Vec<DatasetReferenceSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DatasetReferenceSpec {
    pub(super) field: String,
    pub(super) target_table: String,
    pub(super) source_key: String,
    pub(super) target_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct DatasetManifest {
    pub(super) version: u8,
    pub(super) kind: String,
    pub(super) command: String,
    pub(super) instance: String,
    pub(super) exported_at_unix_s: u64,
    pub(super) tables: Vec<DatasetManifestTable>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DatasetManifestTable {
    pub(super) name: String,
    pub(super) file: String,
    #[serde(default)]
    pub(super) query: Option<String>,
    #[serde(default)]
    pub(super) fields: Option<Vec<String>>,
    pub(super) record_count: usize,
    #[serde(default)]
    pub(super) depends_on: Vec<String>,
    #[serde(default)]
    pub(super) references: Vec<DatasetReferenceSpec>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct DatasetTableArtifact {
    pub(super) version: u8,
    pub(super) kind: String,
    pub(super) table: String,
    #[serde(default)]
    pub(super) source_key_fields: Vec<String>,
    pub(super) records: Vec<DatasetTableRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DatasetTableRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) source_sys_id: Option<String>,
    pub(super) data: Record,
}

#[derive(Debug, Serialize)]
pub(super) struct DatasetExportSummary {
    pub(super) kind: &'static str,
    pub(super) command: &'static str,
    pub(super) instance: String,
    pub(super) table_count: usize,
    pub(super) tables: Vec<DatasetTableSummary>,
    pub(super) out_dir: String,
    pub(super) manifest_path: String,
}

#[derive(Debug, Serialize)]
pub(super) struct DatasetTableSummary {
    pub(super) table: String,
    pub(super) record_count: usize,
    pub(super) file: String,
}

#[derive(Debug, Serialize)]
pub(super) struct DatasetValidationReport {
    pub(super) kind: &'static str,
    pub(super) command: &'static str,
    pub(super) dataset_kind: String,
    pub(super) ready: bool,
    pub(super) table_count: usize,
    pub(super) import_order: Vec<String>,
    pub(super) tables: Vec<ValidationReport>,
    pub(super) errors: Vec<ValidationIssue>,
    pub(super) warnings: Vec<ValidationIssue>,
}

#[derive(Debug, Serialize)]
pub(super) struct DatasetImportReport {
    pub(super) kind: &'static str,
    pub(super) command: &'static str,
    pub(super) strategy: &'static str,
    pub(super) strategy_reason: &'static str,
    pub(super) table_count: usize,
    pub(super) import_order: Vec<String>,
    pub(super) created: usize,
    pub(super) failed: usize,
    pub(super) skipped: usize,
    pub(super) tables: Vec<TableImportResult>,
}

#[derive(Debug, Serialize)]
pub(super) struct TableImportResult {
    pub(super) table: String,
    pub(super) created: usize,
    pub(super) failed: usize,
    pub(super) skipped: usize,
    pub(super) failures: Vec<ImportFailure>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ReferencePlaceholder {
    #[serde(rename = "__reference")]
    pub(super) reference: ReferenceMarker,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct ReferenceMarker {
    pub(super) target_table: String,
    pub(super) source_key: String,
    pub(super) target_key: String,
    pub(super) source_value: String,
}

#[derive(Debug)]
pub(super) struct ExportRequest {
    pub(super) table: TableName,
    pub(super) query: Option<String>,
    pub(super) fields: Option<String>,
    pub(super) limit: Option<usize>,
    pub(super) order_by: Option<String>,
    pub(super) out_path: Option<String>,
}

pub(super) enum DatasetInput {
    Flat(TableExportArtifact),
    Package(DatasetManifest),
}
