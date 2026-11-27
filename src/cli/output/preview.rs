use super::{print_output, print_record, write_output, write_records_csv};
use crate::cli::{args::OutputFormat, truncation::PreviewReport};
use crate::models::field_warning::FieldWarning;
use crate::models::record::Record;
use serde::Serialize;
use std::io::Write;

#[cfg(test)]
mod tests;
use crate::client::pagination::TableListResult;

/// Records per domain name on one printed page (`table list --with-domain`).
pub(crate) type DomainSummary = std::collections::BTreeMap<String, usize>;

#[derive(Serialize)]
struct TableListMeta<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    total: Option<usize>,
    returned: usize,
    offset: usize,
    next_offset: Option<usize>,
    truncated: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    fields_truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    domains_in_page: Option<&'a DomainSummary>,
    /// Non-fatal warnings (e.g. unknown fields); omitted when there are none.
    #[serde(skip_serializing_if = "<[FieldWarning]>::is_empty")]
    warnings: &'a [FieldWarning],
}

impl<'a> TableListMeta<'a> {
    #[cfg(test)]
    fn from_result(
        result: &TableListResult,
        preview: &PreviewReport,
        domains_in_page: Option<&'a DomainSummary>,
    ) -> Self {
        Self::with_extras(
            result,
            preview,
            &TableListExtras {
                domains_in_page,
                warnings: &[],
            },
        )
    }

    fn with_extras(
        result: &TableListResult,
        preview: &PreviewReport,
        extras: &TableListExtras<'a>,
    ) -> Self {
        Self {
            total: result.total,
            returned: result.returned(),
            offset: result.offset,
            next_offset: result.next_offset(),
            truncated: result.truncated,
            fields_truncated: result.fields_truncated || !preview.is_empty(),
            domains_in_page: extras.domains_in_page,
            warnings: extras.warnings,
        }
    }
}

/// Optional `table list` metadata beyond the paging fields.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct TableListExtras<'a> {
    /// `--with-domain` per-domain summary, reported as `domains_in_page`.
    pub domains_in_page: Option<&'a DomainSummary>,
    /// Structured warnings, reported as `warnings` when non-empty.
    pub warnings: &'a [FieldWarning],
}

#[derive(Serialize)]
struct TableListEnvelope<'a> {
    #[serde(flatten)]
    meta: TableListMeta<'a>,
    #[serde(skip_serializing_if = "PreviewReport::is_empty")]
    preview: &'a PreviewReport,
    records: &'a [Record],
}

#[derive(Serialize)]
struct RecordPreviewEnvelope<'a> {
    preview: &'a PreviewReport,
    record: &'a Record,
}

pub(crate) fn ensure_preview_supported(preview: bool, format: &OutputFormat) -> anyhow::Result<()> {
    if preview && matches!(format, OutputFormat::Csv) {
        return Err(crate::models::validation::ValidationError::new(format!(
            "--preview is not supported with --output {}; use json, jsonl, toon, text, or auto, or omit --preview for lossless CSV",
            format.as_str()
        )).into());
    }
    Ok(())
}

pub fn print_table_list(result: &TableListResult, format: &OutputFormat) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut writer = std::io::BufWriter::new(stdout.lock());
    write_table_list_with_preview(result, format, &PreviewReport::default(), None, &mut writer)?;
    writer.flush()?;
    Ok(())
}

/// Print a `table list` page with its preview report and extra metadata
/// (`domains_in_page`, `warnings`).
pub(crate) fn print_table_list_preview(
    result: &TableListResult,
    format: &OutputFormat,
    preview: &PreviewReport,
    extras: &TableListExtras<'_>,
) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut writer = std::io::BufWriter::new(stdout.lock());
    write_table_list_page(result, format, preview, extras, &mut writer)?;
    writer.flush()?;
    Ok(())
}

/// Write the JSONL leading `{"meta": ...}` line for a streamed `table list --all` result.
pub(crate) fn write_table_list_jsonl_meta<W: Write>(
    summary: crate::client::TablePageSummary,
    warnings: &[FieldWarning],
    writer: &mut W,
) -> anyhow::Result<()> {
    let next_offset = if summary.truncated {
        Some(summary.offset.saturating_add(summary.returned))
    } else {
        None
    };
    let meta_line = JsonlMetaLine {
        meta: JsonlMetaBody {
            meta: TableListMeta {
                total: summary.total,
                returned: summary.returned,
                offset: summary.offset,
                next_offset,
                truncated: summary.truncated,
                fields_truncated: false,
                domains_in_page: None,
                warnings,
            },
            preview: &PreviewReport::default(),
        },
    };
    serde_json::to_writer(&mut *writer, &meta_line)?;
    writer.write_all(b"\n")?;
    Ok(())
}

/// Print a single record, wrapping in `RecordPreviewEnvelope` whenever
/// `--preview` was explicitly requested (even if no field exceeded the limit)
/// so `--preview` consumers always receive a stable schema.
pub(crate) fn print_record_preview(
    record: &Record,
    format: &OutputFormat,
    preview_enabled: bool,
    preview: &PreviewReport,
) -> anyhow::Result<()> {
    if !preview_enabled && preview.is_empty() {
        return print_record(record, format);
    }
    let envelope = RecordPreviewEnvelope { preview, record };
    if matches!(format, OutputFormat::Csv) {
        return write_records_csv(std::slice::from_ref(record), &mut std::io::stdout());
    }
    print_output(&envelope, format)
}

pub(crate) fn write_table_list_with_preview<W: Write>(
    result: &TableListResult,
    format: &OutputFormat,
    preview: &PreviewReport,
    domains_in_page: Option<&DomainSummary>,
    writer: &mut W,
) -> anyhow::Result<()> {
    let extras = TableListExtras {
        domains_in_page,
        warnings: &[],
    };
    write_table_list_page(result, format, preview, &extras, writer)
}

#[derive(Serialize)]
struct JsonlMetaBody<'a> {
    #[serde(flatten)]
    meta: TableListMeta<'a>,
    #[serde(skip_serializing_if = "PreviewReport::is_empty")]
    preview: &'a PreviewReport,
}

#[derive(Serialize)]
struct JsonlMetaLine<'a> {
    meta: JsonlMetaBody<'a>,
}

pub(crate) fn write_table_list_page<W: Write>(
    result: &TableListResult,
    format: &OutputFormat,
    preview: &PreviewReport,
    extras: &TableListExtras<'_>,
    writer: &mut W,
) -> anyhow::Result<()> {
    let envelope = TableListEnvelope {
        meta: TableListMeta::with_extras(result, preview, extras),
        preview,
        records: &result.records,
    };
    match format {
        OutputFormat::Csv => write_records_csv(&result.records, writer)?,
        OutputFormat::Jsonl => {
            let meta_line = JsonlMetaLine {
                meta: JsonlMetaBody {
                    meta: TableListMeta::with_extras(result, preview, extras),
                    preview,
                },
            };
            serde_json::to_writer(&mut *writer, &meta_line)?;
            writer.write_all(b"\n")?;
            for record in &result.records {
                serde_json::to_writer(&mut *writer, record)?;
                writer.write_all(b"\n")?;
            }
        }
        _ => write_output(&envelope, format, writer)?,
    }
    Ok(())
}

/// Print one field value verbatim (`table get --raw`): strings unescaped,
/// other JSON values compact, always ending with a newline.
pub(crate) fn print_raw_value(value: &serde_json::Value) -> anyhow::Result<()> {
    let text = match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Null => String::new(),
        other => serde_json::to_string(other)?,
    };
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(text.as_bytes())?;
    if !text.ends_with('\n') {
        stdout.write_all(b"\n")?;
    }
    stdout.flush()?;
    Ok(())
}
