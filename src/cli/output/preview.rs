use super::{print_output, print_record, write_output, write_records_csv};
use crate::cli::{args::OutputFormat, truncation::PreviewReport};
use crate::models::record::Record;
use serde::Serialize;
use std::io::Write;

#[cfg(test)]
mod tests;
use crate::client::pagination::TableListResult;

#[derive(Serialize)]
struct TableListMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    total: Option<usize>,
    returned: usize,
    offset: usize,
    next_offset: Option<usize>,
    truncated: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    fields_truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    domains_in_page: Option<std::collections::BTreeMap<String, usize>>,
}

impl TableListMeta {
    fn from_result(result: &TableListResult, preview: &PreviewReport) -> Self {
        Self {
            total: result.total,
            returned: result.returned(),
            offset: result.offset,
            next_offset: result.next_offset(),
            truncated: result.truncated,
            fields_truncated: result.fields_truncated || !preview.is_empty(),
            domains_in_page: result.domains_in_page.clone(),
        }
    }
}

#[derive(Serialize)]
struct TableListEnvelope<'a> {
    #[serde(flatten)]
    meta: TableListMeta,
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
    write_table_list_with_preview(
        result,
        format,
        &PreviewReport::default(),
        &mut std::io::stdout(),
    )
}

pub(crate) fn print_table_list_preview(
    result: &TableListResult,
    format: &OutputFormat,
    preview: &PreviewReport,
) -> anyhow::Result<()> {
    write_table_list_with_preview(result, format, preview, &mut std::io::stdout())
}

pub(crate) fn print_record_preview(
    record: &Record,
    format: &OutputFormat,
    preview: &PreviewReport,
) -> anyhow::Result<()> {
    if preview.is_empty() {
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
    writer: &mut W,
) -> anyhow::Result<()> {
    let envelope = TableListEnvelope {
        meta: TableListMeta::from_result(result, preview),
        preview,
        records: &result.records,
    };
    match format {
        OutputFormat::Csv => write_records_csv(&result.records, writer)?,
        OutputFormat::Jsonl if preview.is_empty() => {
            let meta = serde_json::json!({ "meta": TableListMeta::from_result(result, preview) });
            writeln!(writer, "{}", serde_json::to_string(&meta)?)?;
            for record in &result.records {
                writeln!(writer, "{}", serde_json::to_string(record)?)?;
            }
        }
        _ => write_output(&envelope, format, writer)?,
    }
    Ok(())
}
