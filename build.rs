use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let skill_root = manifest_dir.join("skills").join("snow-cli");
    println!("cargo:rerun-if-changed={}", skill_root.display());

    let mut rel_files = Vec::new();
    collect_files(&skill_root, &skill_root, &mut rel_files)?;
    rel_files.sort();

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let dest = out_dir.join("bundled_skill.rs");
    let mut file = fs::File::create(&dest)?;

    writeln!(file, "pub const BUNDLED_SKILL_FILES: &[(&str, &[u8])] = &[")?;
    for rel in &rel_files {
        writeln!(
            file,
            "    ({rel:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/skills/snow-cli/\", {rel:?}))),"
        )?;
    }
    writeln!(file, "];")?;
    Ok(())
}

fn collect_files(
    root: &Path,
    current: &Path,
    out: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut entries: Vec<PathBuf> = fs::read_dir(current)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    entries.sort();

    for path in entries {
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'))
        {
            continue;
        }
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            return Err(format!("Refusing to embed symlink {}", path.display()).into());
        }
        if meta.is_dir() {
            println!("cargo:rerun-if-changed={}", path.display());
            collect_files(root, &path, out)?;
        } else if meta.is_file() {
            println!("cargo:rerun-if-changed={}", path.display());
            let rel = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            out.push(rel);
        }
    }
    Ok(())
}
