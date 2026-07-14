use super::*;

pub(super) fn save_screenshot_response(
    response: SnuMessage,
    out_path: Option<&str>,
) -> anyhow::Result<Value> {
    let image_data = response
        .extra
        .get("imageData")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("SN-Utils screenshot response did not contain imageData"))?;
    let file_name = response
        .extra
        .get("fileName")
        .and_then(Value::as_str)
        .unwrap_or("screenshot.png");
    let path = out_path
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(file_name));
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(image_data)
        .context("failed to decode SN-Utils screenshot imageData")?;
    std::fs::write(&path, bytes)
        .with_context(|| format!("failed to write screenshot: {}", path.display()))?;

    Ok(json!({
        "saved": true,
        "file": path,
        "url": response.extra.get("url").cloned().or_else(|| response.extra.get("tabUrl").cloned()),
        "tabTitle": response.extra.get("tabTitle").cloned(),
    }))
}
pub(super) fn guess_content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "json" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "zip" => "application/zip",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        _ => "application/octet-stream",
    }
}
