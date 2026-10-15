use super::*;

pub(super) struct ArtifactDefinition {
    pub(super) artifact_type: &'static str,
    pub(super) category: &'static str,
    pub(super) table: &'static str,
    pub(super) fields: &'static str,
    pub(super) name_field: &'static str,
}

pub(super) struct MoveFileRequest<'a> {
    pub(super) table: &'a TableName,
    pub(super) sys_id: &'a SysId,
    pub(super) target_scope: &'a str,
    pub(super) dry_run: bool,
    pub(super) yes: bool,
}
#[derive(Debug, Clone, Copy)]
pub(super) struct ScopeListTextOptions {
    pub(super) show_source_table: bool,
    pub(super) show_sys_id: bool,
}

/// How `scope list` presents matching rows: which page, and text columns.
#[derive(Debug, Clone, Copy)]
pub(super) struct ScopeListView {
    pub(super) window: ScopeListWindow,
    pub(super) text: ScopeListTextOptions,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct MoveFileOutput {
    pub(super) ok: bool,
    pub(super) dry_run: bool,
    pub(super) table: String,
    pub(super) sys_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) source_scope: Option<ScopeInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) target_scope: Option<ScopeInfo>,
    #[serde(default)]
    pub(super) before: MoveFileState,
    #[serde(default)]
    pub(super) after: MoveFileState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) changed_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) warnings: Vec<String>,
    #[serde(default)]
    pub(super) requires_confirmation: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct MoveFileState {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(super) sys_scope: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(super) sys_package: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(super) sys_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(super) sys_update_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(super) api_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(super) path: String,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct ScopeInspectOutput {
    pub(super) scope: ScopeInfo,
    pub(super) details: String,
    pub(super) summary: ScopeSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) artifacts: Option<Vec<ScopeInventoryRow>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) warnings: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct ScopeInfo {
    pub(super) sys_id: String,
    pub(super) scope: String,
    pub(super) name: String,
    pub(super) version: String,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct ScopeListOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) search: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) kind_filter: Vec<String>,
    pub(super) counts: ScopeListCounts,
    pub(super) offset: usize,
    pub(super) returned: usize,
    pub(super) truncated: bool,
    pub(super) next_offset: Option<usize>,
    pub(super) rows: Vec<ScopeListRow>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) warnings: Vec<String>,
}

/// Default number of `scope list` rows. A PDI has over 2,000 scopes, store
/// apps, and plugins (about 400 KB of JSON), too much for an agent's context.
const DEFAULT_SCOPE_LIST_LIMIT: usize = 20;

/// Which slice of the merged, sorted scope rows to return.
#[derive(Debug, Clone, Copy)]
pub(super) struct ScopeListWindow {
    offset: usize,
    limit: Option<usize>,
}

impl ScopeListWindow {
    pub(super) fn new(limit: Option<usize>, offset: usize, all: bool) -> Self {
        let limit = if all {
            None
        } else {
            Some(limit.unwrap_or(DEFAULT_SCOPE_LIST_LIMIT))
        };
        Self { offset, limit }
    }
}

impl ScopeListOutput {
    /// Keeps only the requested window of rows. `counts` still describe every
    /// matching row, so `counts.total` is the paging total.
    pub(super) fn apply_window(&mut self, window: ScopeListWindow) {
        let total = self.rows.len();
        let start = window.offset.min(total);
        let end = window
            .limit
            .map_or(total, |limit| start.saturating_add(limit).min(total));
        self.rows.truncate(end);
        self.rows.drain(..start);
        self.offset = window.offset;
        self.returned = self.rows.len();
        self.truncated = end < total;
        self.next_offset = self.truncated.then_some(end);
    }
}

#[derive(Debug, serde::Serialize)]
pub(super) struct ScopeListCounts {
    pub(super) total: usize,
    pub(super) by_kind: BTreeMap<String, usize>,
}

impl ScopeListCounts {
    pub(super) fn from_rows(rows: &[ScopeListRow]) -> Self {
        let mut by_kind = BTreeMap::new();
        for row in rows {
            *by_kind.entry(row.kind.clone()).or_insert(0) += 1;
        }

        Self {
            total: rows.len(),
            by_kind,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct ScopeListRow {
    pub(super) kind: String,
    pub(super) scope: String,
    pub(super) name: String,
    pub(super) version: String,
    pub(super) identifier: String,
    pub(super) source_table: String,
    pub(super) sys_id: String,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct ScopeSummary {
    pub(super) total_artifacts: usize,
    pub(super) artifact_counts: BTreeMap<String, usize>,
    pub(super) category_counts: BTreeMap<String, usize>,
}

impl ScopeSummary {
    pub(super) fn from_rows(rows: &[ScopeInventoryRow]) -> Self {
        let mut artifact_counts = BTreeMap::new();
        let mut category_counts = BTreeMap::new();

        for row in rows {
            *artifact_counts
                .entry(row.artifact_type.clone())
                .or_insert(0) += 1;
            *category_counts.entry(row.category.clone()).or_insert(0) += 1;
        }

        Self {
            total_artifacts: rows.len(),
            artifact_counts,
            category_counts,
        }
    }

    pub(super) fn to_csv_rows(&self, scope: &str, scope_sys_id: &str) -> Vec<ScopeInspectCsvRow> {
        self.artifact_counts
            .iter()
            .map(|(artifact, count)| ScopeInspectCsvRow {
                scope: scope.to_string(),
                scope_sys_id: scope_sys_id.to_string(),
                artifact: artifact.clone(),
                count: *count,
            })
            .collect()
    }
}

#[derive(Debug, serde::Serialize)]
pub(super) struct ScopeInspectCsvRow {
    pub(super) scope: String,
    pub(super) scope_sys_id: String,
    pub(super) artifact: String,
    pub(super) count: usize,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct ScopeInventoryOutput {
    pub(super) scope: ScopeInfo,
    pub(super) summary: ScopeSummary,
    pub(super) rows: Vec<ScopeInventoryRow>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) warnings: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct ScopeInventoryRow {
    pub(super) scope: String,
    pub(super) scope_sys_id: String,
    pub(super) category: String,
    pub(super) artifact_type: String,
    pub(super) source_table: String,
    pub(super) sys_id: String,
    pub(super) name: String,
}

pub(super) struct CollectedArtifactSet {
    pub(super) category: String,
    pub(super) artifact_type: String,
    pub(super) source_table: String,
    pub(super) name_field: String,
    pub(super) records: Vec<Record>,
}

pub(super) struct CollectedScopeData {
    pub(super) scope: ScopeInfo,
    pub(super) summary: ScopeSummary,
    pub(super) artifact_sets: Vec<CollectedArtifactSet>,
    pub(super) other_rows: Vec<ScopeInventoryRow>,
    pub(super) warnings: Vec<String>,
}

impl CollectedScopeData {
    pub(super) fn to_inventory_rows(&self) -> Vec<ScopeInventoryRow> {
        let mut rows = Vec::new();
        for set in &self.artifact_sets {
            rows.extend(map_inventory_rows(
                &self.scope,
                &set.category,
                &set.artifact_type,
                &set.source_table,
                &set.records,
                &set.name_field,
            ));
        }
        rows.extend(self.other_rows.clone());
        rows
    }
}
