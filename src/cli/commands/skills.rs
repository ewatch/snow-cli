use crate::cli::args::{OutputFormat, SkillArgs, SkillCommands, SkillTarget};
use crate::cli::output::print_output;
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

const MANIFEST_FILE: &str = "skill.toml";
const LOCK_FILE: &str = "snow-cli-skill.lock.toml";

pub async fn handle(args: SkillArgs, output_format: &OutputFormat) -> anyhow::Result<()> {
    match args.command {
        SkillCommands::Install {
            source,
            target_dir,
            target,
            name,
            pack,
            all_packs,
        } => {
            let result = install_skill(InstallOptions {
                source,
                target_dir,
                target,
                name,
                packs: pack,
                all_packs,
            })
            .await?;
            print_output(&result, output_format)
        }
    }
}

#[derive(Debug)]
struct InstallOptions {
    source: String,
    target_dir: Option<PathBuf>,
    target: Option<SkillTarget>,
    name: Option<String>,
    packs: Vec<String>,
    all_packs: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
struct SkillManifest {
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default = "default_entrypoint")]
    entrypoint: String,
    #[serde(default)]
    packs: Vec<SkillPack>,
    #[serde(default)]
    files: Vec<SkillFile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
struct SkillPack {
    name: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
struct SkillFile {
    path: String,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    executable: bool,
}

#[derive(Debug)]
struct LoadedBundle {
    source: String,
    base: SourceBase,
    manifest_text: String,
    manifest: SkillManifest,
    files: Vec<SkillFile>,
}

#[derive(Debug, Clone)]
enum SourceBase {
    Local { root: PathBuf },
    Remote { manifest_url: reqwest::Url },
}

#[derive(Debug, Serialize)]
struct InstallResult {
    status: &'static str,
    name: String,
    version: Option<String>,
    source: String,
    install_dir: String,
    packs: Vec<String>,
    files: Vec<InstalledFile>,
    lockfile: String,
}

#[derive(Debug, Clone, Serialize)]
struct InstalledFile {
    path: String,
    sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct SkillLock {
    name: String,
    version: Option<String>,
    source: String,
    packs: Vec<String>,
    files: Vec<InstalledFile>,
}

fn default_entrypoint() -> String {
    "SKILL.md".to_string()
}

async fn install_skill(options: InstallOptions) -> anyhow::Result<InstallResult> {
    if options.target_dir.is_some() && options.target.is_some() {
        bail!("Use either --target-dir or --target, not both.");
    }
    if options.target_dir.is_none() && options.target.is_none() {
        bail!("Choose an install location with --target-dir or --target.");
    }

    let bundle = load_bundle(&options.source).await?;
    validate_safe_path(&bundle.manifest.entrypoint)?;

    let install_name = options
        .name
        .clone()
        .unwrap_or_else(|| default_install_name(&bundle.manifest.name));
    validate_install_name(&install_name)?;

    let target_root = match (&options.target_dir, &options.target) {
        (Some(path), None) => path.clone(),
        (None, Some(target)) => target_root(target)?,
        _ => unreachable!("target options were validated above"),
    };
    let install_dir = target_root.join(&install_name);

    let selected_packs = select_packs(&bundle, &options.packs, options.all_packs)?;
    let selected_files = selected_files(&bundle, &selected_packs, options.all_packs)?;
    let mut installed = Vec::new();

    fs::create_dir_all(&install_dir).with_context(|| {
        format!(
            "Failed to create install directory {}",
            install_dir.display()
        )
    })?;

    let manifest_digest = sha256_hex(bundle.manifest_text.as_bytes());
    write_install_file(&install_dir, MANIFEST_FILE, bundle.manifest_text.as_bytes())?;
    installed.push(InstalledFile {
        path: MANIFEST_FILE.to_string(),
        sha256: manifest_digest,
    });

    for file in selected_files {
        let safe_path = validate_safe_path(&file.path)?;
        let bytes = read_bundle_file(&bundle.base, &file).await?;
        let digest = sha256_hex(&bytes);
        if let Some(expected) = file.sha256.as_deref() {
            if !digest.eq_ignore_ascii_case(expected) {
                bail!(
                    "Digest mismatch for {}: expected {}, got {}.",
                    file.path,
                    expected,
                    digest
                );
            }
        } else if matches!(bundle.base, SourceBase::Remote { .. }) {
            bail!(
                "Remote file {} must declare sha256 in skill.toml.",
                file.path
            );
        }

        write_install_file(&install_dir, &safe_path, &bytes)?;
        installed.push(InstalledFile {
            path: safe_path,
            sha256: digest,
        });
    }

    installed.sort_by(|a, b| a.path.cmp(&b.path));

    let lock = SkillLock {
        name: bundle.manifest.name.clone(),
        version: bundle.manifest.version.clone(),
        source: bundle.source.clone(),
        packs: selected_packs.iter().cloned().collect(),
        files: installed.clone(),
    };
    let lock_text = toml::to_string_pretty(&lock)?;
    write_install_file(&install_dir, LOCK_FILE, lock_text.as_bytes())?;

    Ok(InstallResult {
        status: "success",
        name: bundle.manifest.name,
        version: bundle.manifest.version,
        source: bundle.source,
        install_dir: install_dir.to_string_lossy().into_owned(),
        packs: selected_packs.into_iter().collect(),
        files: installed,
        lockfile: install_dir.join(LOCK_FILE).to_string_lossy().into_owned(),
    })
}

async fn load_bundle(source: &str) -> anyhow::Result<LoadedBundle> {
    if let Ok(url) = reqwest::Url::parse(source) {
        return match url.scheme() {
            "http" | "https" => load_remote_bundle(source, url).await,
            "file" => {
                let path = url
                    .to_file_path()
                    .map_err(|_| anyhow::anyhow!("Invalid file URL: {source}"))?;
                load_local_bundle(source.to_string(), path)
            }
            scheme => bail!("Unsupported skill source URL scheme: {scheme}"),
        };
    }

    load_local_bundle(source.to_string(), PathBuf::from(source))
}

async fn load_remote_bundle(
    source: &str,
    manifest_url: reqwest::Url,
) -> anyhow::Result<LoadedBundle> {
    let response = reqwest::get(manifest_url.clone())
        .await
        .with_context(|| format!("Failed to fetch skill manifest {manifest_url}"))?;
    if !response.status().is_success() {
        bail!(
            "Failed to fetch skill manifest {}: HTTP {}.",
            manifest_url,
            response.status()
        );
    }
    let manifest_text = response.text().await?;
    let manifest: SkillManifest =
        toml::from_str(&manifest_text).context("Failed to parse skill.toml")?;
    if manifest.files.is_empty() {
        bail!("Remote skill manifests must declare [[files]] entries.");
    }
    validate_manifest(&manifest)?;
    Ok(LoadedBundle {
        source: source.to_string(),
        base: SourceBase::Remote { manifest_url },
        manifest_text,
        files: manifest.files.clone(),
        manifest,
    })
}

fn load_local_bundle(source: String, source_path: PathBuf) -> anyhow::Result<LoadedBundle> {
    let manifest_path = if source_path.is_dir() {
        source_path.join(MANIFEST_FILE)
    } else {
        source_path
    };
    let root = manifest_path
        .parent()
        .context("Local skill manifest path has no parent directory")?
        .to_path_buf();
    let manifest_text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
    let manifest: SkillManifest =
        toml::from_str(&manifest_text).context("Failed to parse skill.toml")?;
    validate_manifest(&manifest)?;

    let files = if manifest.files.is_empty() {
        enumerate_local_files(&root)?
            .into_iter()
            .filter(|path| path != MANIFEST_FILE)
            .map(|path| SkillFile {
                path,
                sha256: None,
                executable: false,
            })
            .collect()
    } else {
        manifest.files.clone()
    };

    Ok(LoadedBundle {
        source,
        base: SourceBase::Local { root },
        manifest_text,
        files,
        manifest,
    })
}

fn validate_manifest(manifest: &SkillManifest) -> anyhow::Result<()> {
    validate_safe_path(&manifest.entrypoint)?;
    for file in &manifest.files {
        validate_safe_path(&file.path)?;
    }
    for pack in &manifest.packs {
        validate_install_name(&pack.name)?;
    }
    Ok(())
}

fn enumerate_local_files(root: &Path) -> anyhow::Result<Vec<String>> {
    fn visit(root: &Path, current: &Path, out: &mut Vec<String>) -> anyhow::Result<()> {
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            let meta = fs::symlink_metadata(&path)?;
            if meta.file_type().is_symlink() {
                bail!("Refusing to install symlink {}", path.display());
            }
            if meta.is_dir() {
                visit(root, &path, out)?;
            } else if meta.is_file() {
                let relative = path
                    .strip_prefix(root)?
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                validate_safe_path(&relative)?;
                out.push(relative);
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn select_packs(
    bundle: &LoadedBundle,
    requested: &[String],
    all_packs: bool,
) -> anyhow::Result<BTreeSet<String>> {
    if all_packs && !requested.is_empty() {
        bail!("Use either --all-packs or --pack, not both.");
    }

    let available = available_packs(bundle);
    let selected: BTreeSet<String> = if all_packs {
        available.clone()
    } else {
        requested.iter().cloned().collect()
    };

    for pack in &selected {
        if !available.contains(pack) {
            bail!("Unknown skill pack '{pack}'.");
        }
    }

    Ok(selected)
}

fn available_packs(bundle: &LoadedBundle) -> BTreeSet<String> {
    let mut packs: BTreeSet<String> = bundle
        .manifest
        .packs
        .iter()
        .map(|p| p.name.clone())
        .collect();
    for file in &bundle.files {
        if let Some(pack) = pack_name_for_path(&file.path) {
            packs.insert(pack);
        }
    }
    packs
}

fn selected_files(
    bundle: &LoadedBundle,
    selected_packs: &BTreeSet<String>,
    all_packs: bool,
) -> anyhow::Result<Vec<SkillFile>> {
    let mut by_path = BTreeMap::new();
    for file in &bundle.files {
        let safe_path = validate_safe_path(&file.path)?;
        if safe_path == MANIFEST_FILE {
            continue;
        }
        let pack = pack_name_for_path(&safe_path);
        if pack.is_none()
            || all_packs
            || pack
                .as_ref()
                .is_some_and(|pack_name| selected_packs.contains(pack_name))
        {
            by_path.insert(safe_path, file.clone());
        }
    }

    let selected = by_path.into_values().collect::<Vec<_>>();
    if !selected
        .iter()
        .any(|file| file.path == bundle.manifest.entrypoint)
    {
        bail!(
            "Selected files do not include entrypoint '{}'.",
            bundle.manifest.entrypoint
        );
    }
    Ok(selected)
}

async fn read_bundle_file(base: &SourceBase, file: &SkillFile) -> anyhow::Result<Vec<u8>> {
    match base {
        SourceBase::Local { root } => read_local_file(root, file),
        SourceBase::Remote { manifest_url } => read_remote_file(manifest_url, file).await,
    }
}

fn read_local_file(root: &Path, file: &SkillFile) -> anyhow::Result<Vec<u8>> {
    let safe_path = validate_safe_path(&file.path)?;
    let path = root.join(&safe_path);
    let meta = fs::symlink_metadata(&path)
        .with_context(|| format!("Failed to inspect {}", path.display()))?;
    if meta.file_type().is_symlink() {
        bail!("Refusing to install symlink {}", path.display());
    }
    if !meta.is_file() {
        bail!("Skill file {} is not a regular file.", path.display());
    }
    if is_executable(&meta) && !file.executable {
        bail!(
            "Executable skill file {} must be declared with executable = true.",
            file.path
        );
    }
    fs::read(&path).with_context(|| format!("Failed to read {}", path.display()))
}

async fn read_remote_file(
    manifest_url: &reqwest::Url,
    file: &SkillFile,
) -> anyhow::Result<Vec<u8>> {
    let safe_path = validate_safe_path(&file.path)?;
    let url = manifest_url
        .join(&safe_path)
        .with_context(|| format!("Invalid remote skill file path {}", file.path))?;
    let response = reqwest::get(url.clone())
        .await
        .with_context(|| format!("Failed to fetch skill file {url}"))?;
    if !response.status().is_success() {
        bail!(
            "Failed to fetch skill file {}: HTTP {}.",
            url,
            response.status()
        );
    }
    Ok(response.bytes().await?.to_vec())
}

fn write_install_file(install_dir: &Path, relative: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let safe_path = validate_safe_path(relative)?;
    let out_path = install_dir.join(&safe_path);
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&out_path, bytes).with_context(|| format!("Failed to write {}", out_path.display()))
}

fn validate_safe_path(path: &str) -> anyhow::Result<String> {
    let candidate = Path::new(path);
    let mut parts = Vec::new();
    for component in candidate.components() {
        match component {
            Component::Normal(value) => {
                let part = value.to_string_lossy();
                if part.is_empty() {
                    bail!("Invalid empty path component in {path}");
                }
                parts.push(part.into_owned());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("Unsafe skill file path: {path}");
            }
        }
    }
    if parts.is_empty() {
        bail!("Skill file path cannot be empty.");
    }
    Ok(parts.join("/"))
}

fn validate_install_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        bail!("Invalid skill name: {name}");
    }
    Ok(())
}

fn default_install_name(name: &str) -> String {
    name.rsplit('/').next().unwrap_or(name).to_string()
}

fn pack_name_for_path(path: &str) -> Option<String> {
    let mut parts = path.split('/');
    if parts.next()? != "packs" {
        return None;
    }
    parts.next().map(ToString::to_string)
}

fn target_root(target: &SkillTarget) -> anyhow::Result<PathBuf> {
    let home = home_dir()?;
    Ok(match target {
        SkillTarget::Codex => home.join(".codex").join("skills"),
        SkillTarget::Claude => home.join(".claude").join("skills"),
        SkillTarget::Agents => home.join(".agents").join("skills"),
    })
}

fn home_dir() -> anyhow::Result<PathBuf> {
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home));
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        return Ok(PathBuf::from(home));
    }
    bail!("Could not determine home directory for --target. Use --target-dir instead.");
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

#[cfg(unix)]
fn is_executable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_paths_reject_absolute_and_parent_paths() {
        assert!(validate_safe_path("SKILL.md").is_ok());
        assert!(validate_safe_path("packs/table-api/SKILL.md").is_ok());
        assert!(validate_safe_path("../SKILL.md").is_err());
        assert!(validate_safe_path("/tmp/SKILL.md").is_err());
    }

    #[test]
    fn pack_name_is_derived_from_pack_paths() {
        assert_eq!(
            pack_name_for_path("packs/table-api/SKILL.md"),
            Some("table-api".to_string())
        );
        assert_eq!(pack_name_for_path("SKILL.md"), None);
    }

    #[test]
    fn default_name_uses_last_path_segment() {
        assert_eq!(default_install_name("snow/snow-cli"), "snow-cli");
        assert_eq!(default_install_name("snow-cli"), "snow-cli");
    }
}
