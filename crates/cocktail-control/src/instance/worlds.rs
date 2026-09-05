//! World folder helpers.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;

use super::files::{self, create_backup};
use super::model::BackupInfo;

#[derive(Debug, Serialize)]
pub struct WorldInfo {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
}

pub fn list_worlds(workdir: &str) -> anyhow::Result<Vec<WorldInfo>> {
    let root = PathBuf::from(workdir);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for ent in fs::read_dir(&root)? {
        let ent = ent?;
        if !ent.file_type()?.is_dir() {
            continue;
        }
        let name = ent.file_name().to_string_lossy().into_owned();
        // skip common non-world dirs
        if matches!(
            name.as_str(),
            "plugins" | "mods" | "libraries" | "versions" | "logs" | "cache" | "config"
        ) {
            continue;
        }
        let path = ent.path();
        let is_world = path.join("level.dat").exists()
            || path.join("region").exists()
            || path.join("DIM-1").exists()
            || name == "world"
            || name.starts_with("world_");
        if !is_world {
            continue;
        }
        out.push(WorldInfo {
            name: name.clone(),
            path: name,
            size_bytes: dir_size(&path).unwrap_or(0),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn reset_world(workdir: &str, world: &str) -> anyhow::Result<()> {
    if world.contains("..") || world.contains('/') || world.contains('\\') {
        anyhow::bail!("invalid world name");
    }
    let path = PathBuf::from(workdir).join(world);
    if !path.exists() {
        anyhow::bail!("world not found");
    }
    fs::remove_dir_all(&path)?;
    Ok(())
}

pub fn export_world(instance_id: &str, workdir: &str, world: &str) -> anyhow::Result<BackupInfo> {
    if world.contains("..") || world.contains('/') || world.contains('\\') {
        anyhow::bail!("invalid world name");
    }
    let src = PathBuf::from(workdir).join(world);
    if !src.is_dir() {
        anyhow::bail!("world not found");
    }
    // Reuse zip by temporarily creating a single-world backup directory layout.
    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let staging = PathBuf::from("data")
        .join("tmp")
        .join(format!("{instance_id}-world-{world}-{stamp}"));
    fs::create_dir_all(&staging)?;
    copy_dir(&src, &staging.join(world))?;
    let bak = create_backup(
        &format!("{instance_id}-world-{world}"),
        staging.to_str().unwrap_or("."),
    )?;
    let _ = fs::remove_dir_all(&staging);
    Ok(BackupInfo {
        id: format!("world-{world}-{}", bak.id),
        created_at: bak.created_at,
        path: bak.path,
        size_bytes: bak.size_bytes,
    })
}

pub fn import_world(workdir: &str, world: &str, zip_bytes: &[u8]) -> anyhow::Result<()> {
    if world.contains("..") || world.contains('/') || world.contains('\\') {
        anyhow::bail!("invalid world name");
    }
    let dest = PathBuf::from(workdir).join(world);
    if dest.exists() {
        fs::remove_dir_all(&dest)?;
    }
    let tmp_zip = PathBuf::from("data")
        .join("tmp")
        .join(format!("import-{world}.zip"));
    if let Some(p) = tmp_zip.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(&tmp_zip, zip_bytes)?;
    let extract_to = PathBuf::from(workdir).join(format!(".import-{world}"));
    files::unzip_archive(&tmp_zip, &extract_to)?;
    // If zip contains a single top-level dir, use it; else move extract_to -> dest
    let entries: Vec<_> = fs::read_dir(&extract_to)?.filter_map(|e| e.ok()).collect();
    if entries.len() == 1 && entries[0].path().is_dir() {
        fs::rename(entries[0].path(), &dest)?;
        let _ = fs::remove_dir_all(&extract_to);
    } else {
        fs::rename(&extract_to, &dest)?;
    }
    let _ = fs::remove_file(tmp_zip);
    Ok(())
}

fn copy_dir(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dst)?;
    for ent in fs::read_dir(src)? {
        let ent = ent?;
        let from = ent.path();
        let to = dst.join(ent.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn dir_size(path: &Path) -> anyhow::Result<u64> {
    let mut total = 0u64;
    if path.is_file() {
        return Ok(fs::metadata(path)?.len());
    }
    for ent in fs::read_dir(path)? {
        let ent = ent?;
        let p = ent.path();
        total += if p.is_dir() {
            dir_size(&p)?
        } else {
            ent.metadata()?.len()
        };
    }
    Ok(total)
}
