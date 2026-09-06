use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::model::{BackupInfo, FileContent, FileEntry};
use crate::util;

const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_UPLOAD_BYTES: u64 = 512 * 1024 * 1024;

pub fn resolve_in_workdir(workdir: &str, relative: &str) -> anyhow::Result<PathBuf> {
    let root = fs::canonicalize(workdir).unwrap_or_else(|_| PathBuf::from(workdir));
    if !root.exists() {
        fs::create_dir_all(&root)?;
    }
    let root = fs::canonicalize(&root)?;

    let rel = Path::new(relative.trim_start_matches(['/', '\\']));
    for c in rel.components() {
        match c {
            Component::Normal(_) | Component::CurDir => {}
            _ => anyhow::bail!("invalid path"),
        }
    }

    let candidate = if relative.is_empty() || relative == "." {
        root.clone()
    } else {
        root.join(rel)
    };

    if candidate.exists() {
        let canon = fs::canonicalize(&candidate)?;
        if !canon.starts_with(&root) {
            anyhow::bail!("path escapes workdir");
        }
        Ok(canon)
    } else {
        let parent = candidate.parent().unwrap_or(&root);
        fs::create_dir_all(parent)?;
        let parent = fs::canonicalize(parent)?;
        if !parent.starts_with(&root) {
            anyhow::bail!("path escapes workdir");
        }
        Ok(parent.join(candidate.file_name().unwrap_or_default()))
    }
}

pub fn list_files(workdir: &str, relative: &str) -> anyhow::Result<Vec<FileEntry>> {
    let dir = resolve_in_workdir(workdir, relative)?;
    if !dir.is_dir() {
        anyhow::bail!("not a directory");
    }
    let root = fs::canonicalize(workdir)?;
    let mut entries = Vec::new();
    for ent in fs::read_dir(&dir)? {
        let ent = ent?;
        let meta = ent.metadata()?;
        let full = ent.path();
        let rel = full
            .strip_prefix(&root)
            .unwrap_or(&full)
            .to_string_lossy()
            .replace('\\', "/");
        entries.push(FileEntry {
            name: ent.file_name().to_string_lossy().into_owned(),
            path: rel,
            is_dir: meta.is_dir(),
            size: if meta.is_file() { meta.len() } else { 0 },
        });
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(entries)
}

pub fn read_file(workdir: &str, relative: &str) -> anyhow::Result<FileContent> {
    let path = resolve_in_workdir(workdir, relative)?;
    if !path.is_file() {
        anyhow::bail!("not a file");
    }
    let meta = fs::metadata(&path)?;
    if meta.len() > MAX_TEXT_BYTES {
        anyhow::bail!("file too large for text edit (>2MiB); use download");
    }
    let content = fs::read_to_string(&path)?;
    Ok(FileContent {
        path: rel_path(workdir, &path)?,
        content,
    })
}

pub fn read_bytes(workdir: &str, relative: &str) -> anyhow::Result<(String, Vec<u8>)> {
    let path = resolve_in_workdir(workdir, relative)?;
    if !path.is_file() {
        anyhow::bail!("not a file");
    }
    let meta = fs::metadata(&path)?;
    if meta.len() > MAX_UPLOAD_BYTES {
        anyhow::bail!("file too large");
    }
    Ok((rel_path(workdir, &path)?, fs::read(&path)?))
}

pub fn write_file(workdir: &str, relative: &str, content: &str) -> anyhow::Result<FileContent> {
    if content.len() as u64 > MAX_TEXT_BYTES {
        anyhow::bail!("content too large (>2MiB)");
    }
    let path = resolve_in_workdir(workdir, relative)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, content)?;
    read_file(workdir, relative)
}

pub fn write_bytes(workdir: &str, relative: &str, bytes: &[u8]) -> anyhow::Result<FileEntry> {
    if bytes.len() as u64 > MAX_UPLOAD_BYTES {
        anyhow::bail!("upload too large (>512MiB)");
    }
    let path = resolve_in_workdir(workdir, relative)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, bytes)?;
    let meta = fs::metadata(&path)?;
    Ok(FileEntry {
        name: path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: rel_path(workdir, &path)?,
        is_dir: false,
        size: meta.len(),
    })
}

pub fn delete_path(workdir: &str, relative: &str) -> anyhow::Result<()> {
    if relative.is_empty() || relative == "." {
        anyhow::bail!("cannot delete workdir root");
    }
    let path = resolve_in_workdir(workdir, relative)?;
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else if path.is_file() {
        fs::remove_file(path)?;
    } else {
        anyhow::bail!("path not found");
    }
    Ok(())
}

pub fn mkdir(workdir: &str, relative: &str) -> anyhow::Result<FileEntry> {
    let rel = relative.trim().trim_matches(['/', '\\']);
    if rel.is_empty() {
        anyhow::bail!("directory name is required");
    }
    let path = resolve_in_workdir(workdir, rel)?;
    fs::create_dir_all(&path)?;
    Ok(FileEntry {
        name: path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: rel_path(workdir, &path)?,
        is_dir: true,
        size: 0,
    })
}

/// True if a relative path looks like a server/plugin jar under workdir.
pub fn jar_exists(workdir: &str, relative: &str) -> bool {
    resolve_in_workdir(workdir, relative)
        .map(|p| p.is_file())
        .unwrap_or(false)
}

pub fn ensure_seed_files(workdir: &str, port: u16, eula_accepted: bool) -> anyhow::Result<()> {
    fs::create_dir_all(workdir)?;
    fs::create_dir_all(Path::new(workdir).join("plugins"))?;
    fs::create_dir_all(Path::new(workdir).join("mods"))?;
    let props = PathBuf::from(workdir).join("server.properties");
    if !props.exists() {
        fs::write(
            &props,
            format!(
                "\
# Cocktail Manager seed
motd=A Cocktail Minecraft Server
server-port={port}
max-players=20
gamemode=survival
difficulty=easy
online-mode=true
white-list=false
"
            ),
        )?;
    } else {
        util::set_property_file(&props, "server-port", &port.to_string())?;
    }
    let eula = PathBuf::from(workdir).join("eula.txt");
    if !eula.exists() || eula_accepted {
        util::write_eula(workdir, eula_accepted)?;
    }
    Ok(())
}

pub fn sync_port(workdir: &str, port: u16) -> anyhow::Result<()> {
    let props = PathBuf::from(workdir).join("server.properties");
    util::set_property_file(&props, "server-port", &port.to_string())
}

pub fn create_backup(instance_id: &str, workdir: &str) -> anyhow::Result<BackupInfo> {
    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let dir = PathBuf::from("data").join("backups").join(instance_id);
    fs::create_dir_all(&dir)?;
    let dest = dir.join(format!("{stamp}.zip"));
    zip_dir(Path::new(workdir), &dest)?;
    let size = fs::metadata(&dest)?.len();
    let created_at = file_created_at(&dest).unwrap_or_else(Utc::now);
    Ok(BackupInfo {
        id: format!("{stamp}.zip"),
        created_at,
        path: dest.to_string_lossy().replace('\\', "/"),
        size_bytes: size,
    })
}

pub fn prune_backups(instance_id: &str, keep: u32) -> anyhow::Result<usize> {
    let mut list = list_backups(instance_id)?;
    if keep == 0 || list.len() <= keep as usize {
        return Ok(0);
    }
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let mut n = 0;
    for bak in list.into_iter().skip(keep as usize) {
        delete_backup(instance_id, &bak.id)?;
        n += 1;
    }
    Ok(n)
}

pub fn list_backups(instance_id: &str) -> anyhow::Result<Vec<BackupInfo>> {
    let root = PathBuf::from("data").join("backups").join(instance_id);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for ent in fs::read_dir(&root)? {
        let ent = ent?;
        let path = ent.path();
        let name = ent.file_name().to_string_lossy().into_owned();
        let meta = ent.metadata()?;
        let (is_backup, size) = if meta.is_file() && name.ends_with(".zip") {
            (true, meta.len())
        } else if meta.is_dir() {
            (true, dir_size(&path).unwrap_or(0))
        } else {
            (false, 0)
        };
        if !is_backup {
            continue;
        }
        out.push(BackupInfo {
            id: name,
            created_at: file_created_at(&path).unwrap_or_else(Utc::now),
            path: path.to_string_lossy().replace('\\', "/"),
            size_bytes: size,
        });
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

pub fn delete_backup(instance_id: &str, backup_id: &str) -> anyhow::Result<()> {
    let path = PathBuf::from("data")
        .join("backups")
        .join(instance_id)
        .join(backup_id);
    if !path.exists() {
        anyhow::bail!("backup not found");
    }
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn restore_backup(instance_id: &str, backup_id: &str, workdir: &str) -> anyhow::Result<()> {
    let src = PathBuf::from("data")
        .join("backups")
        .join(instance_id)
        .join(backup_id);
    if !src.exists() {
        anyhow::bail!("backup not found");
    }
    clear_dir_contents(workdir)?;
    if src.is_dir() {
        copy_dir_recursive(&src, Path::new(workdir))?;
    } else {
        unzip_to(&src, Path::new(workdir))?;
    }
    Ok(())
}

pub fn unzip_archive(zip_path: &Path, dest: &Path) -> anyhow::Result<()> {
    unzip_to(zip_path, dest)
}

fn clear_dir_contents(workdir: &str) -> anyhow::Result<()> {
    if Path::new(workdir).exists() {
        for ent in fs::read_dir(workdir)? {
            let ent = ent?;
            let p = ent.path();
            if p.is_dir() {
                fs::remove_dir_all(p)?;
            } else {
                fs::remove_file(p)?;
            }
        }
    } else {
        fs::create_dir_all(workdir)?;
    }
    Ok(())
}

fn zip_dir(src: &Path, dest: &Path) -> anyhow::Result<()> {
    let file = File::create(dest)?;
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    add_dir_to_zip(&mut zip, src, src, opts)?;
    zip.finish()?;
    Ok(())
}

fn add_dir_to_zip(
    zip: &mut ZipWriter<File>,
    root: &Path,
    current: &Path,
    opts: SimpleFileOptions,
) -> anyhow::Result<()> {
    for ent in fs::read_dir(current)? {
        let ent = ent?;
        let path = ent.path();
        let name = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            if !name.is_empty() {
                zip.add_directory(format!("{name}/"), opts)?;
            }
            add_dir_to_zip(zip, root, &path, opts)?;
        } else {
            zip.start_file(name, opts)?;
            let mut f = File::open(&path)?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            zip.write_all(&buf)?;
        }
    }
    Ok(())
}

fn unzip_to(zip_path: &Path, dest: &Path) -> anyhow::Result<()> {
    let file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;
    fs::create_dir_all(dest)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(p) => dest.join(p),
            None => continue,
        };
        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dst)?;
    for ent in fs::read_dir(src)? {
        let ent = ent?;
        let from = ent.path();
        let to = dst.join(ent.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
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

fn file_created_at(path: &Path) -> Option<DateTime<Utc>> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().ok().or_else(|| meta.created().ok())?;
    let duration = modified.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    DateTime::from_timestamp(duration.as_secs() as i64, duration.subsec_nanos())
}

fn rel_path(workdir: &str, path: &Path) -> anyhow::Result<String> {
    let root = fs::canonicalize(workdir)?;
    Ok(path
        .strip_prefix(&root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/"))
}
