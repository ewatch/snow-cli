use crate::cli::args::{OutputFormat, SkillArgs, SkillCommands, SkillTarget};
use crate::cli::output::print_output;
use crate::models::validation::ValidationError;
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

const MANIFEST_FILE: &str = "skill.toml";
const LOCK_FILE: &str = "snow-cli-skill.lock.toml";

include!(concat!(env!("OUT_DIR"), "/bundled_skill.rs"));

pub async fn handle(args: SkillArgs, output_format: &OutputFormat) -> anyhow::Result<()> {
    match args.command {
        SkillCommands::Install {
            source,
            bundled,
            dry_run,
            force,
            target_dir,
            target,
            name,
            pack,
            all_packs,
        } => {
            let result = install_skill(InstallOptions {
                source,
                bundled,
                dry_run,
                force,
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
    source: Option<String>,
    bundled: bool,
    dry_run: bool,
    force: bool,
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
    Bundled,
    Local { root: PathBuf },
    Remote { manifest_url: url::Url },
}

#[derive(Debug, Serialize)]
struct InstallResult {
    status: &'static str,
    dry_run: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    action: Option<&'static str>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
struct SkillLock {
    name: String,
    #[serde(default)]
    version: Option<String>,
    source: String,
    #[serde(default)]
    packs: Vec<String>,
    #[serde(default)]
    files: Vec<LockedFile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct LockedFile {
    path: String,
    sha256: String,
}

#[derive(Debug)]
struct StaleEntry {
    path: String,
    sha256: String,
    action: &'static str,
    delete: bool,
}

fn default_entrypoint() -> String {
    "SKILL.md".to_string()
}

async fn install_skill(options: InstallOptions) -> anyhow::Result<InstallResult> {
    if options.target_dir.is_some() && options.target.is_some() {
        return Err(ValidationError::new("Use either --target-dir or --target, not both.").into());
    }
    if options.target_dir.is_none() && options.target.is_none() {
        return Err(ValidationError::new(
            "Choose an install location with --target-dir or --target.",
        )
        .into());
    }
    if options.bundled && options.source.is_some() {
        return Err(
            ValidationError::new("Use either --bundled or a source path/URL, not both.").into(),
        );
    }

    let bundle = if options.bundled {
        load_bundled_bundle()?
    } else if let Some(source) = options.source.as_deref() {
        load_bundle(source).await?
    } else {
        return Err(ValidationError::new(
            "Choose a skill source path/URL or pass --bundled to install the embedded skill.",
        )
        .into());
    };
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

    ensure_no_symlink_components(&install_dir, None)?;
    ensure_no_symlink_components(&install_dir, Some(LOCK_FILE))?;

    let prev_locked = read_existing_lock(&install_dir)?;

    let selected_packs = select_packs(&bundle, &options.packs, options.all_packs)?;
    let selected_files = selected_files(&bundle, &selected_packs, options.all_packs)?;

    let mut staged: Vec<(String, Vec<u8>, String, &'static str)> = Vec::new();
    let mut modified_conflicts: Vec<String> = Vec::new();

    let manifest_bytes = bundle.manifest_text.as_bytes().to_vec();
    let manifest_digest = sha256_hex(&manifest_bytes);
    let manifest_action = planned_file_action(
        &install_dir,
        MANIFEST_FILE,
        &manifest_digest,
        prev_locked.get(MANIFEST_FILE).map(String::as_str),
        options.force,
    )?;
    if manifest_action == "conflict" {
        modified_conflicts.push(MANIFEST_FILE.to_string());
    }
    staged.push((
        MANIFEST_FILE.to_string(),
        manifest_bytes,
        manifest_digest,
        manifest_action,
    ));

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

        let action = planned_file_action(
            &install_dir,
            &safe_path,
            &digest,
            prev_locked.get(&safe_path).map(String::as_str),
            options.force,
        )?;
        if action == "conflict" {
            modified_conflicts.push(safe_path.clone());
        }
        staged.push((safe_path, bytes, digest, action));
    }

    staged.sort_by(|a, b| a.0.cmp(&b.0));

    let staged_paths: BTreeSet<&str> = staged.iter().map(|(path, _, _, _)| path.as_str()).collect();
    let stale_entries =
        plan_stale_removals(&install_dir, &prev_locked, &staged_paths, options.force)?;

    if !options.dry_run && !modified_conflicts.is_empty() {
        modified_conflicts.sort();
        return Err(ValidationError::new(format!(
            "Refusing to overwrite locally modified file(s): {}. Re-run with --force to overwrite.",
            modified_conflicts.join(", ")
        ))
        .into());
    }

    if !options.dry_run {
        fs::create_dir_all(&install_dir).with_context(|| {
            format!(
                "Failed to create install directory {}",
                install_dir.display()
            )
        })?;
        for (path, bytes, _, _) in &staged {
            write_install_file(&install_dir, path, bytes)?;
        }
        for stale in &stale_entries {
            if stale.delete {
                remove_install_file(&install_dir, &stale.path)?;
            }
        }
    }

    let locked_files: Vec<LockedFile> = staged
        .iter()
        .map(|(path, _, sha256, _)| LockedFile {
            path: path.clone(),
            sha256: sha256.clone(),
        })
        .collect();

    let mut reported_files: Vec<InstalledFile> = staged
        .iter()
        .map(|(path, _, sha256, action)| InstalledFile {
            path: path.clone(),
            sha256: sha256.clone(),
            action: Some(*action),
        })
        .collect();
    for stale in &stale_entries {
        reported_files.push(InstalledFile {
            path: stale.path.clone(),
            sha256: stale.sha256.clone(),
            action: Some(stale.action),
        });
    }
    reported_files.sort_by(|a, b| a.path.cmp(&b.path));

    if !options.dry_run {
        let lock = SkillLock {
            name: bundle.manifest.name.clone(),
            version: bundle.manifest.version.clone(),
            source: bundle.source.clone(),
            packs: selected_packs.iter().cloned().collect(),
            files: locked_files,
        };
        let lock_text = toml::to_string_pretty(&lock)?;
        write_install_file(&install_dir, LOCK_FILE, lock_text.as_bytes())?;
    }

    Ok(InstallResult {
        status: if options.dry_run {
            "dry_run"
        } else {
            "success"
        },
        dry_run: options.dry_run,
        name: bundle.manifest.name,
        version: bundle.manifest.version,
        source: bundle.source,
        install_dir: install_dir.to_string_lossy().into_owned(),
        packs: selected_packs.into_iter().collect(),
        files: reported_files,
        lockfile: install_dir.join(LOCK_FILE).to_string_lossy().into_owned(),
    })
}

fn read_existing_lock(install_dir: &Path) -> anyhow::Result<BTreeMap<String, String>> {
    let lock_path = install_dir.join(LOCK_FILE);
    match fs::symlink_metadata(&lock_path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(err) => {
            return Err(err).with_context(|| format!("Failed to inspect {}", lock_path.display()));
        }
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                bail!("Refusing to read symlink {}", lock_path.display());
            }
            if !meta.is_file() {
                bail!(
                    "Lockfile path {} is not a regular file.",
                    lock_path.display()
                );
            }
        }
    }

    let text = fs::read_to_string(&lock_path)
        .with_context(|| format!("Failed to read {}", lock_path.display()))?;
    let lock: SkillLock = toml::from_str(&text)
        .with_context(|| format!("Failed to parse existing lockfile {}", lock_path.display()))?;
    let mut map = BTreeMap::new();
    for entry in lock.files {
        let safe = validate_safe_path(&entry.path)?;
        map.insert(safe, entry.sha256);
    }
    Ok(map)
}

fn ensure_no_symlink_components(install_dir: &Path, relative: Option<&str>) -> anyhow::Result<()> {
    match fs::symlink_metadata(install_dir) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err)
                .with_context(|| format!("Failed to inspect {}", install_dir.display()));
        }
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                bail!(
                    "Refusing to install through symlink {}",
                    install_dir.display()
                );
            }
            if !meta.is_dir() {
                bail!(
                    "Install directory {} is not a directory.",
                    install_dir.display()
                );
            }
        }
    }

    let Some(rel) = relative else {
        return Ok(());
    };
    let safe_path = validate_safe_path(rel)?;
    let parts: Vec<&str> = safe_path.split('/').collect();
    let mut current = install_dir.to_path_buf();
    for (idx, part) in parts.iter().enumerate() {
        current.push(part);
        let is_leaf = idx + 1 == parts.len();
        match fs::symlink_metadata(&current) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => break,
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("Failed to inspect {}", current.display()));
            }
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    bail!("Refusing to access symlink {}", current.display());
                }
                if is_leaf {
                    if !meta.is_file() {
                        bail!(
                            "Install target {} is not a regular file.",
                            current.display()
                        );
                    }
                } else if !meta.is_dir() {
                    bail!(
                        "Install parent path {} is not a directory.",
                        current.display()
                    );
                }
            }
        }
    }
    Ok(())
}

fn planned_file_action(
    install_dir: &Path,
    relative: &str,
    new_sha256: &str,
    locked_sha256: Option<&str>,
    force: bool,
) -> anyhow::Result<&'static str> {
    let safe_path = validate_safe_path(relative)?;
    ensure_no_symlink_components(install_dir, Some(&safe_path))?;
    let target = install_dir.join(&safe_path);
    match fs::symlink_metadata(&target) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok("create"),
        Err(err) => Err(err).with_context(|| format!("Failed to inspect {}", target.display())),
        Ok(_) => {
            let existing = fs::read(&target)
                .with_context(|| format!("Failed to read existing {}", target.display()))?;
            let existing_sha = sha256_hex(&existing);
            if existing_sha.eq_ignore_ascii_case(new_sha256) {
                Ok("unchanged")
            } else {
                let locally_modified = match locked_sha256 {
                    Some(expected) => !existing_sha.eq_ignore_ascii_case(expected),
                    None => true,
                };
                if locally_modified && !force {
                    Ok("conflict")
                } else {
                    Ok("overwrite")
                }
            }
        }
    }
}

fn plan_stale_removals(
    install_dir: &Path,
    prev_locked: &BTreeMap<String, String>,
    staged_paths: &BTreeSet<&str>,
    force: bool,
) -> anyhow::Result<Vec<StaleEntry>> {
    let mut entries = Vec::new();
    for (path, locked_sha) in prev_locked {
        if staged_paths.contains(path.as_str()) {
            continue;
        }
        let safe_path = validate_safe_path(path)?;
        ensure_no_symlink_components(install_dir, Some(&safe_path))?;
        let target = install_dir.join(&safe_path);
        match fs::symlink_metadata(&target) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err).with_context(|| format!("Failed to inspect {}", target.display()));
            }
            Ok(_) => {
                let existing = fs::read(&target)
                    .with_context(|| format!("Failed to read existing {}", target.display()))?;
                let existing_sha = sha256_hex(&existing);
                let unmodified = existing_sha.eq_ignore_ascii_case(locked_sha);
                if unmodified || force {
                    entries.push(StaleEntry {
                        path: safe_path,
                        sha256: existing_sha,
                        action: "remove",
                        delete: true,
                    });
                } else {
                    entries.push(StaleEntry {
                        path: safe_path,
                        sha256: existing_sha,
                        action: "keep_modified",
                        delete: false,
                    });
                }
            }
        }
    }
    Ok(entries)
}

fn remove_install_file(install_dir: &Path, relative: &str) -> anyhow::Result<()> {
    let safe_path = validate_safe_path(relative)?;
    ensure_no_symlink_components(install_dir, Some(&safe_path))?;
    let target = install_dir.join(&safe_path);
    fs::remove_file(&target)
        .with_context(|| format!("Failed to remove stale file {}", target.display()))?;

    let mut parent = target.parent();
    while let Some(dir) = parent {
        if dir == install_dir {
            break;
        }
        match fs::remove_dir(dir) {
            Ok(()) => parent = dir.parent(),
            Err(_) => break,
        }
    }
    Ok(())
}

fn load_bundled_bundle() -> anyhow::Result<LoadedBundle> {
    let manifest_bytes = BUNDLED_SKILL_FILES
        .iter()
        .find_map(|(path, bytes)| (*path == MANIFEST_FILE).then_some(*bytes))
        .context("Embedded skill bundle is missing skill.toml")?;
    let manifest_text = std::str::from_utf8(manifest_bytes)
        .context("Embedded skill.toml is not valid UTF-8")?
        .to_string();
    let manifest: SkillManifest =
        toml::from_str(&manifest_text).context("Failed to parse embedded skill.toml")?;
    validate_manifest(&manifest)?;

    let files = if manifest.files.is_empty() {
        BUNDLED_SKILL_FILES
            .iter()
            .filter(|(path, _)| *path != MANIFEST_FILE)
            .map(|(path, _)| SkillFile {
                path: (*path).to_string(),
                sha256: None,
                executable: false,
            })
            .collect()
    } else {
        manifest.files.clone()
    };

    Ok(LoadedBundle {
        source: "bundled".to_string(),
        base: SourceBase::Bundled,
        manifest_text,
        manifest,
        files,
    })
}

async fn load_bundle(source: &str) -> anyhow::Result<LoadedBundle> {
    if let Ok(url) = url::Url::parse(source) {
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

async fn load_remote_bundle(source: &str, manifest_url: url::Url) -> anyhow::Result<LoadedBundle> {
    let response = crate::client::fetch_skill_resource(&manifest_url)
        .await
        .with_context(|| format!("Failed to fetch skill manifest {manifest_url}"))?;
    if !(200..300).contains(&response.status) {
        bail!(
            "Failed to fetch skill manifest {}: HTTP {}.",
            manifest_url,
            response.status
        );
    }
    let manifest_text = response.text();
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
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('.'))
            {
                continue;
            }
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
        return Err(ValidationError::new("Use either --all-packs or --pack, not both.").into());
    }

    let available = available_packs(bundle);
    let selected: BTreeSet<String> = if all_packs {
        available.clone()
    } else {
        requested.iter().cloned().collect()
    };

    for pack in &selected {
        if !available.contains(pack) {
            let known = if available.is_empty() {
                "none".to_string()
            } else {
                available.iter().cloned().collect::<Vec<_>>().join(", ")
            };
            return Err(ValidationError::new(format!(
                "Unknown skill pack '{pack}'. Available packs: {known}."
            ))
            .into());
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
        SourceBase::Bundled => read_bundled_file(file),
        SourceBase::Local { root } => read_local_file(root, file),
        SourceBase::Remote { manifest_url } => read_remote_file(manifest_url, file).await,
    }
}

fn read_bundled_file(file: &SkillFile) -> anyhow::Result<Vec<u8>> {
    let safe_path = validate_safe_path(&file.path)?;
    let bytes = BUNDLED_SKILL_FILES
        .iter()
        .find_map(|(path, bytes)| (*path == safe_path).then_some(*bytes))
        .with_context(|| format!("Embedded skill bundle is missing '{safe_path}'."))?;
    Ok(bytes.to_vec())
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

fn resolve_remote_file_url(manifest_url: &url::Url, relative: &str) -> anyhow::Result<url::Url> {
    let safe_path = validate_safe_path(relative)?;
    let url = manifest_url
        .join(&safe_path)
        .with_context(|| format!("Invalid remote skill file path {relative}"))?;

    if url.origin() != manifest_url.origin() || url.scheme() != manifest_url.scheme() {
        bail!("Unsafe remote skill file URL escapes manifest origin: {relative}");
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!("Unsafe remote skill file URL contains query or fragment: {relative}");
    }
    let manifest_path = manifest_url.path();
    let base_dir = match manifest_path.rfind('/') {
        Some(idx) => &manifest_path[..=idx],
        None => "/",
    };
    if !url.path().starts_with(base_dir) {
        bail!("Unsafe remote skill file URL escapes manifest directory: {relative}");
    }
    Ok(url)
}

async fn read_remote_file(manifest_url: &url::Url, file: &SkillFile) -> anyhow::Result<Vec<u8>> {
    let url = resolve_remote_file_url(manifest_url, &file.path)?;
    let response = crate::client::fetch_skill_resource(&url)
        .await
        .with_context(|| format!("Failed to fetch skill file {url}"))?;
    if !(200..300).contains(&response.status) {
        bail!(
            "Failed to fetch skill file {}: HTTP {}.",
            url,
            response.status
        );
    }
    Ok(response.body)
}

fn write_install_file(install_dir: &Path, relative: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let safe_path = validate_safe_path(relative)?;
    ensure_no_symlink_components(install_dir, Some(&safe_path))?;
    let out_path = install_dir.join(&safe_path);
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    ensure_no_symlink_components(install_dir, Some(&safe_path))?;
    fs::write(&out_path, bytes).with_context(|| format!("Failed to write {}", out_path.display()))
}

fn validate_safe_path(path: &str) -> anyhow::Result<String> {
    if path.contains(['\\', ':', '%', '?', '#', '\0']) {
        bail!("Unsafe skill file path: {path}");
    }
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

fn is_windows_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn validate_install_name(name: &str) -> anyhow::Result<()> {
    let mut chars = name.chars();
    let valid_first = chars.next().is_some_and(|ch| ch.is_ascii_alphanumeric());
    let valid_rest = chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'));
    if !valid_first || !valid_rest || is_windows_reserved_name(name) {
        return Err(ValidationError::new(format!("Invalid skill name: {name}")).into());
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
    fn safe_paths_reject_absolute_parent_and_url_escape_paths() {
        assert!(validate_safe_path("SKILL.md").is_ok());
        assert!(validate_safe_path("packs/table-api/PACK.md").is_ok());
        assert!(validate_safe_path("../SKILL.md").is_err());
        assert!(validate_safe_path("/tmp/SKILL.md").is_err());
        assert!(validate_safe_path("%2e%2e/%2e%2e/secret").is_err());
        assert!(validate_safe_path(r"..\..\secret").is_err());
        assert!(validate_safe_path("http:/127.0.0.1:8080/x").is_err());
        assert!(validate_safe_path("SKILL.md?x=1").is_err());
        assert!(validate_safe_path("SKILL.md#frag").is_err());
    }

    #[test]
    fn remote_url_resolution_stays_under_manifest_origin_and_dir() {
        let base = url::Url::parse("https://ewatch.github.io/snow-cli/skills/snow-cli/skill.toml")
            .unwrap_or_else(|e| panic!("valid test url: {e}"));
        let ok = resolve_remote_file_url(&base, "references/acls.md")
            .unwrap_or_else(|e| panic!("valid relative path: {e}"));
        assert_eq!(
            ok.as_str(),
            "https://ewatch.github.io/snow-cli/skills/snow-cli/references/acls.md"
        );
        assert!(resolve_remote_file_url(&base, "%2e%2e/%2e%2e/secret").is_err());
        assert!(resolve_remote_file_url(&base, "http:/127.0.0.1:8080/x").is_err());
    }

    #[test]
    fn install_name_rejects_drive_letters_and_reserved_windows_names() {
        assert!(validate_install_name("snow-cli").is_ok());
        assert!(validate_install_name("snow_cli.v1").is_ok());
        assert!(validate_install_name("C:").is_err());
        assert!(validate_install_name("CON").is_err());
        assert!(validate_install_name("nul.txt").is_err());
        assert!(validate_install_name(".hidden").is_err());
        assert!(validate_install_name("a/b").is_err());
    }

    #[test]
    fn pack_name_is_derived_from_pack_paths() {
        assert_eq!(
            pack_name_for_path("packs/table-api/PACK.md"),
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
