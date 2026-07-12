use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use reqwest::multipart::{Form, Part};

use crate::cli::args::{AttachmentArgs, AttachmentCommands, OutputFormat};
use crate::cli::output;
use crate::models::identifiers::EncodedQueryValue;

const MAX_ATTACHMENT_UPLOAD_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
struct AttachmentRecord {
    #[serde(default)]
    sys_id: String,
    #[serde(default)]
    file_name: String,
    #[serde(default)]
    content_type: String,
    #[serde(default)]
    size_bytes: String,
    #[serde(default)]
    table_name: String,
    #[serde(default)]
    table_sys_id: String,
    #[serde(default)]
    download_link: String,
}

#[derive(Debug, serde::Deserialize)]
struct AttachmentListResponse {
    result: Vec<AttachmentRecord>,
}

#[derive(Debug, serde::Deserialize)]
struct AttachmentSingleResponse {
    result: AttachmentRecord,
}

pub async fn handle(
    args: AttachmentArgs,
    profile: &str,
    format: &OutputFormat,
    instance: Option<&str>,
    timeout_secs: Option<u64>,
) -> anyhow::Result<()> {
    match args.command {
        AttachmentCommands::List { table, sys_id } => {
            tracing::info!("Listing attachments for {}/{}", table, sys_id);
            // sys_id is also embedded in an encoded query below, so it must
            // additionally satisfy the (stricter, operator-character-free)
            // encoded-query rules on top of the path-segment rules already
            // enforced by the `SysId` clap type.
            let _: EncodedQueryValue = sys_id.as_str().parse()?;

            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
            let query = format!("table_name={table}^table_sys_id={sys_id}");
            let response: AttachmentListResponse = client
                .get_json_with_params(
                    "/api/now/attachment",
                    &[
                        ("sysparm_query", query.as_str()),
                        (
                            "sysparm_fields",
                            "sys_id,file_name,content_type,size_bytes,table_name,table_sys_id,download_link",
                        ),
                    ],
                )
                .await?;

            output::print_list(&response.result, format)?;
            Ok(())
        }
        AttachmentCommands::Download { sys_id, out_path } => {
            tracing::info!("Downloading attachment: {}", sys_id);

            let mut client =
                crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
            let meta_path = format!("/api/now/attachment/{sys_id}");
            let metadata: AttachmentSingleResponse = client.get_json(&meta_path).await?;

            let destination = resolve_download_path(&metadata.result, out_path);
            let download_path = if metadata.result.download_link.is_empty() {
                format!("/api/now/attachment/{sys_id}/file")
            } else {
                metadata.result.download_link.clone()
            };

            download_attachment_file(&client, &download_path, &destination).await?;

            output::print_status(
                &format!(
                    "Downloaded attachment {} to {}",
                    metadata.result.file_name,
                    destination.display()
                ),
                format,
            )?;
            Ok(())
        }
        AttachmentCommands::Upload {
            table,
            sys_id,
            file,
        } => {
            if crate::policy::active_policy().mode() == crate::policy::PolicyMode::ReadOnly {
                return Err(crate::policy::PolicyError {
                    operation: "attachment upload".to_string(),
                    mode: crate::policy::PolicyMode::ReadOnly,
                    capability: crate::policy::CommandCapability::RemoteWrite,
                    reason: "read-only policy does not allow attachment uploads".to_string(),
                }
                .into());
            }

            tracing::info!("Uploading {} to {}/{}", file, table, sys_id);

            let path = PathBuf::from(&file);
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| {
                    anyhow::anyhow!("Unable to determine filename from path '{}'.", file)
                })?
                .to_string();

            let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if bytes > MAX_ATTACHMENT_UPLOAD_BYTES {
                anyhow::bail!(
                    "Attachment '{}' is {} bytes, exceeding the maximum upload size of {} bytes.",
                    file,
                    bytes,
                    MAX_ATTACHMENT_UPLOAD_BYTES
                );
            }

            if std::io::stderr().is_terminal() {
                eprintln!("Uploading '{}' ({} bytes)...", file_name, bytes);
            }

            let file_bytes = tokio::fs::read(&path)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to read file '{}': {}", file, e))?;

            let client = crate::client::build_client_with_timeout(profile, instance, timeout_secs)?;
            let auth_headers = client.authenticator().authenticate().await?;
            let url = format!("{}/api/now/attachment/upload", client.base_url());

            let file_part = Part::bytes(file_bytes).file_name(file_name.clone());
            let form = Form::new()
                .text("table_name", table.to_string())
                .text("table_sys_id", sys_id.to_string())
                .text("file_name", file_name.clone())
                .part("file", file_part);

            let response = client
                .http()
                .post(&url)
                .query(&[
                    ("table_name", table.as_str()),
                    ("table_sys_id", sys_id.as_str()),
                    ("file_name", file_name.as_str()),
                ])
                .header("Accept", "application/json")
                .headers(auth_headers)
                .multipart(form)
                .send()
                .await?;

            let status = response.status();
            let body = response.text().await?;
            if !status.is_success() {
                anyhow::bail!(
                    "Attachment upload failed with status {}: {}",
                    status.as_u16(),
                    body
                );
            }

            let created: AttachmentSingleResponse = serde_json::from_str(&body)
                .map_err(|e| anyhow::anyhow!("Failed to parse upload response JSON: {e}"))?;

            output::print_output(&created.result, format)?;
            Ok(())
        }
    }
}

fn resolve_download_path(record: &AttachmentRecord, output: Option<String>) -> PathBuf {
    if let Some(path) = output {
        return PathBuf::from(path);
    }
    if let Some(file_name) = safe_default_download_file_name(&record.file_name) {
        return PathBuf::from(file_name);
    }
    PathBuf::from(format!("{}.bin", record.sys_id))
}

fn safe_default_download_file_name(file_name: &str) -> Option<String> {
    let base_name = Path::new(file_name).file_name()?.to_str()?.trim();
    if base_name.is_empty() || base_name == "." || base_name == ".." {
        return None;
    }

    let sanitized = base_name
        .chars()
        .map(|ch| {
            if ch.is_control() || ch == '/' || ch == '\\' {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();

    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

async fn download_attachment_file(
    client: &crate::client::SnowClient,
    path: &str,
    destination: &Path,
) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;

    let auth_headers = client.authenticator().authenticate().await?;
    let url = client.authenticated_url(path)?;

    let mut response = client
        .http()
        .get(url)
        .header("Accept", "*/*")
        .headers(auth_headers)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "Attachment download failed with status {}: {}",
            status.as_u16(),
            body
        );
    }

    let mut file = tokio::fs::File::create(destination).await.map_err(|e| {
        anyhow::anyhow!(
            "Failed to create output file '{}': {}",
            destination.display(),
            e
        )
    })?;

    let total = response.content_length();
    let mut downloaded: u64 = 0;
    let show_progress = std::io::stderr().is_terminal() && total.unwrap_or(0) >= 1_048_576;

    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        if show_progress {
            if let Some(size) = total {
                let pct = (downloaded as f64 / size as f64) * 100.0;
                eprint!("\rDownloading... {:>5.1}% ({}/{})", pct, downloaded, size);
            } else {
                eprint!("\rDownloading... {} bytes", downloaded);
            }
        }
    }

    file.flush().await?;

    if show_progress {
        eprintln!();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_default_download_file_name_uses_basename() {
        assert_eq!(
            safe_default_download_file_name("../../secret.txt"),
            Some("secret.txt".to_string())
        );
        assert_eq!(
            safe_default_download_file_name("nested/report.pdf"),
            Some("report.pdf".to_string())
        );
        assert_eq!(safe_default_download_file_name(".."), None);
    }

    #[tokio::test]
    async fn attachment_upload_rejects_read_only_policy() {
        crate::policy::set_active_policy(crate::policy::ExecutionPolicy::read_only());
        let args = AttachmentArgs {
            command: AttachmentCommands::Upload {
                table: "incident".parse().unwrap(),
                sys_id: "abc123".parse().unwrap(),
                file: "/dev/null".to_string(),
            },
        };
        let result = handle(args, "dummy", &OutputFormat::Json, None, None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        let policy_err = err.downcast_ref::<crate::policy::PolicyError>();
        assert!(policy_err.is_some(), "Expected PolicyError, got: {err}");
        assert_eq!(policy_err.unwrap().code(), "POLICY_DENIED");
        crate::policy::set_active_policy(crate::policy::ExecutionPolicy::full_access());
    }
}
