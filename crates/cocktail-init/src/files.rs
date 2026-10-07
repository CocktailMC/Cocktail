//! 实例工作区文件操作：阶段 2 起 cocktail-init 接管用户空间文件读写。
//!
//! control 端通过 IPC `files.*` 调用本模块。所有函数保持同步（与
//! `sevenz::extract` 一致直接在 RPC handler 中调用），重操作后续可改
//! `tokio::task::spawn_blocking`，当前 init 单线程串行处理即可。
//!
//! 安全要点：`resolve_in_workdir` 做了 zip-slip / 路径穿越防护，所有
//! 用户提供的相对路径都经它校验后再落盘。`backup_path`/`restore_backup`
//! 对备份名做 file_name 规范化，拒绝 `..` 等逃逸片段。

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use cocktail_shared::model::{BackupInfo, BackupScan, FileContent, FileEntry};

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

pub fn jar_exists(workdir: &str, relative: &str) -> bool {
    resolve_in_workdir(workdir, relative)
        .map(|p| p.is_file())
        .unwrap_or(false)
}

pub fn default_instance_root(id: &str) -> String {
    PathBuf::from("data")
        .join("instances")
        .join(id)
        .to_string_lossy()
        .replace('\\', "/")
}

fn comparable_workdir(s: &str) -> PathBuf {
    let p = Path::new(s.trim());
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    };
    lexical_normalize(&abs)
}

fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    #[cfg(windows)]
    {
        PathBuf::from(out.to_string_lossy().to_ascii_lowercase())
    }
    #[cfg(not(windows))]
    {
        out
    }
}

pub fn workdirs_conflict(a: &str, b: &str) -> bool {
    let a = comparable_workdir(a);
    let b = comparable_workdir(b);
    a == b || a.starts_with(&b) || b.starts_with(&a)
}

pub fn ensure_seed_files(workdir: &str, port: u16, eula_accepted: bool) -> anyhow::Result<()> {
    fs::create_dir_all(workdir)?;
    fs::create_dir_all(Path::new(workdir).join("plugins"))?;
    fs::create_dir_all(Path::new(workdir).join("mods"))?;
    fs::create_dir_all(Path::new(workdir).join("cache"))?;
    fs::create_dir_all(Path::new(workdir).join("libraries"))?;
    fs::create_dir_all(Path::new(workdir).join("logs"))?;
    fs::create_dir_all(Path::new(workdir).join("runtime"))?;
    fs::create_dir_all(Path::new(workdir).join(".cocktail").join("tmp"))?;
    fs::create_dir_all(Path::new(workdir).join(".cocktail").join("appdata"))?;
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
        set_property_file(&props, "server-port", &port.to_string())?;
    }
    let eula = PathBuf::from(workdir).join("eula.txt");
    if !eula.exists() || eula_accepted {
        write_eula(workdir, eula_accepted)?;
    }
    Ok(())
}

pub fn sync_port(workdir: &str, port: u16) -> anyhow::Result<()> {
    let props = PathBuf::from(workdir).join("server.properties");
    set_property_file(&props, "server-port", &port.to_string())
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

pub fn backup_path(instance_id: &str, backup_id: &str) -> anyhow::Result<PathBuf> {
    let safe = Path::new(backup_id)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .ok_or_else(|| anyhow::anyhow!("非法备份名"))?;
    if safe != backup_id || safe.contains("..") {
        anyhow::bail!("非法备份名");
    }
    let path = PathBuf::from("data")
        .join("backups")
        .join(instance_id)
        .join(&safe);
    if !path.exists() {
        anyhow::bail!("backup not found");
    }
    Ok(path)
}

pub fn backup_meta(instance_id: &str, backup_id: &str) -> anyhow::Result<BackupInfo> {
    let path = backup_path(instance_id, backup_id)?;
    let meta = fs::metadata(&path)?;
    let size = if meta.is_dir() {
        dir_size(&path).unwrap_or(0)
    } else {
        meta.len()
    };
    Ok(BackupInfo {
        id: backup_id.to_string(),
        created_at: file_created_at(&path).unwrap_or_else(Utc::now),
        path: path.to_string_lossy().replace('\\', "/"),
        size_bytes: size,
    })
}

pub fn inspect_backup_zip(path: &Path) -> anyhow::Result<BackupScan> {
    if path.is_dir() {
        let mut scan = BackupScan {
            entries: 0,
            size_bytes: dir_size(path).unwrap_or(0),
            world_bytes: 0,
            plugin_count: 0,
            has_server_properties: path.join("server.properties").is_file(),
            has_level_dat: path.join("world").join("level.dat").is_file(),
            ..Default::default()
        };
        let mut stack = vec![path.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for ent in entries.flatten() {
                let p = ent.path();
                if p.is_symlink() {
                    continue;
                }
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                scan.entries += 1;
                let rel = p
                    .strip_prefix(path)
                    .map(|r| r.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                if rel.starts_with("world/") {
                    scan.world_bytes += p.metadata().map(|m| m.len()).unwrap_or(0);
                }
                if rel.starts_with("plugins/") && rel.ends_with(".jar") {
                    scan.plugin_count += 1;
                }
            }
        }
        return Ok(scan);
    }
    let file = File::open(path)?;
    let mut zip = ZipArchive::new(file)?;
    let mut scan = BackupScan {
        entries: 0,
        size_bytes: fs::metadata(path)?.len(),
        world_bytes: 0,
        plugin_count: 0,
        has_server_properties: false,
        has_level_dat: false,
    };
    for i in 0..zip.len() {
        let Ok(entry) = zip.by_index(i) else {
            continue;
        };
        let name = entry.name().to_string();
        if name.ends_with('/') {
            continue;
        }
        scan.entries += 1;
        if name == "server.properties" {
            scan.has_server_properties = true;
        }
        if name == "world/level.dat" {
            scan.has_level_dat = true;
        }
        if name.starts_with("world/") {
            scan.world_bytes += entry.size();
        }
        if name.starts_with("plugins/") && name.ends_with(".jar") {
            scan.plugin_count += 1;
        }
    }
    Ok(scan)
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
        if skip_backup_rel(&name) {
            continue;
        }
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

fn skip_backup_rel(name: &str) -> bool {
    let n = name.trim_end_matches('/');
    matches!(n, "runtime" | ".cocktail/tmp" | ".cocktail/appdata")
        || n.starts_with("runtime/")
        || n.starts_with(".cocktail/tmp/")
        || n.starts_with(".cocktail/appdata/")
}

pub fn copy_instance_tree(
    src: &str,
    dst: &str,
    copy_data: bool,
    skip_logs: bool,
) -> anyhow::Result<u32> {
    let src_root = Path::new(src);
    if !src_root.is_dir() {
        anyhow::bail!("源目录不存在：{src}");
    }
    fs::create_dir_all(dst)?;
    let mut copied = 0u32;
    copy_walk(
        src_root,
        Path::new(dst),
        src_root,
        copy_data,
        skip_logs,
        &mut copied,
        0,
    )?;
    Ok(copied)
}

fn copy_skip(name: &str, copy_data: bool, skip_logs: bool) -> bool {
    let n = name.trim_end_matches('/');
    if matches!(n, ".cocktail/tmp" | ".cocktail/appdata") {
        return true;
    }
    if n.starts_with(".cocktail/tmp/") || n.starts_with(".cocktail/appdata/") {
        return true;
    }
    if skip_logs && (n == "logs" || n.starts_with("logs/")) {
        return true;
    }
    if skip_logs && (n == "crash-reports" || n.starts_with("crash-reports/")) {
        return true;
    }
    if !copy_data {
        if n == "world" || n.starts_with("world/") {
            return true;
        }
        if n.starts_with("world_") {
            return true;
        }
    }
    false
}

fn copy_walk(
    current: &Path,
    dst_root: &Path,
    src_root: &Path,
    copy_data: bool,
    skip_logs: bool,
    copied: &mut u32,
    depth: u32,
) -> anyhow::Result<()> {
    if depth > 32 {
        return Ok(());
    }
    for ent in fs::read_dir(current)? {
        let ent = ent?;
        let path = ent.path();
        if path.is_symlink() {
            continue;
        }
        let rel = path
            .strip_prefix(src_root)?
            .to_string_lossy()
            .replace('\\', "/");
        if copy_skip(&rel, copy_data, skip_logs) {
            continue;
        }
        let target = dst_root.join(path.strip_prefix(src_root)?);
        if path.is_dir() {
            fs::create_dir_all(&target)?;
            copy_walk(
                &path,
                dst_root,
                src_root,
                copy_data,
                skip_logs,
                copied,
                depth + 1,
            )?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, &target)?;
            *copied += 1;
        }
    }
    Ok(())
}

pub fn pack_subdir_zip(workdir: &str, relative: &str, dest: &Path) -> anyhow::Result<u64> {
    let root = resolve_in_workdir(workdir, relative)?;
    if !root.is_dir() {
        anyhow::bail!("目录不存在：{relative}");
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    zip_dir(&root, dest)?;
    Ok(fs::metadata(dest)?.len())
}

pub fn extract_zip_into(workdir: &str, relative: &str, bytes: &[u8]) -> anyhow::Result<u32> {
    let dest = resolve_in_workdir(workdir, relative)?;
    fs::create_dir_all(&dest)?;
    let staging = PathBuf::from(workdir)
        .join(".cocktail")
        .join("tmp")
        .join(format!("world-{}", uuid::Uuid::new_v4()));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    let tmp_zip = PathBuf::from(workdir)
        .join(".cocktail")
        .join("tmp")
        .join(format!("world-{}.zip", uuid::Uuid::new_v4()));
    fs::write(&tmp_zip, bytes)?;
    let result = (|| -> anyhow::Result<u32> {
        unzip_archive(&tmp_zip, &staging)?;
        let inner = single_child_dir(&staging)?;
        let files = count_files(&inner)?;
        merge_into(&inner, &dest)?;
        Ok(files)
    })();
    let _ = fs::remove_file(&tmp_zip);
    let _ = fs::remove_dir_all(&staging);
    result
}

fn single_child_dir(root: &Path) -> anyhow::Result<PathBuf> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for ent in fs::read_dir(root)? {
        let path = ent?.path();
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if name == "__MACOSX" || name.starts_with("._") || name == ".DS_Store" {
            continue;
        }
        if path.is_dir() {
            dirs.push(path);
        } else {
            files.push(path);
        }
    }
    if dirs.len() == 1 && files.is_empty() {
        let inner = &dirs[0];
        let has_level = inner.join("level.dat").is_file()
            || inner.join("region").is_dir()
            || inner.join("DIM1").is_dir();
        if has_level {
            return Ok(inner.clone());
        }
    }
    Ok(root.to_path_buf())
}

fn count_files(root: &Path) -> anyhow::Result<u32> {
    let mut n = 0u32;
    fn walk(dir: &Path, n: &mut u32, depth: u32) {
        if depth > 32 {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for ent in entries.flatten() {
            let p = ent.path();
            if p.is_symlink() {
                continue;
            }
            if p.is_dir() {
                walk(&p, n, depth + 1);
            } else {
                *n += 1;
            }
        }
    }
    walk(root, &mut n, 0);
    Ok(n)
}

fn merge_into(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dst)?;
    for ent in fs::read_dir(src)? {
        let ent = ent?;
        let from = ent.path();
        if from.is_symlink() {
            continue;
        }
        let to = dst.join(ent.file_name());
        if from.is_dir() {
            merge_into(&from, &to)?;
        } else {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

pub fn guess_mc_version(workdir: &str) -> Option<String> {
    if let Some(v) = guess_version_from_properties(workdir) {
        return Some(v);
    }
    if let Some(v) = guess_version_from_jars(workdir) {
        return Some(v);
    }
    guess_version_from_logs(workdir)
}

fn guess_version_from_properties(workdir: &str) -> Option<String> {
    let path = Path::new(workdir).join("server.properties");
    let raw = fs::read_to_string(path).ok()?;
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with("version=") || line.starts_with("level-name=") {
            continue;
        }
        if let Some(v) = line.strip_prefix("cocktail-mc-version=") {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn guess_version_from_jars(workdir: &str) -> Option<String> {
    let mut names: Vec<String> = Vec::new();
    let mut roots = vec![PathBuf::from(workdir)];
    roots.push(Path::new(workdir).join("versions"));
    for root in roots {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for ent in entries.flatten() {
            let name = ent.file_name().to_string_lossy().to_string();
            if name.to_ascii_lowercase().ends_with(".jar") {
                names.push(name);
            }
        }
    }
    for name in names {
        if let Some(v) = extract_semver(&name) {
            return Some(v);
        }
    }
    None
}

fn extract_semver(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    if !(lower.contains("paper")
        || lower.contains("purpur")
        || lower.contains("folia")
        || lower.contains("leaves")
        || lower.contains("server")
        || lower.contains("vanilla")
        || lower.contains("fabric")
        || lower.contains("forge")
        || lower.contains("quilt"))
    {
        return None;
    }
    let bytes: Vec<char> = name.chars().collect();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            let mut dots = 0;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == '.') {
                if bytes[i] == '.' {
                    dots += 1;
                }
                i += 1;
            }
            let candidate: String = bytes[start..i].iter().collect();
            let candidate = candidate.trim_matches('.').to_string();
            if dots >= 1 && candidate.len() >= 3 && candidate.len() <= 12 {
                return Some(candidate);
            }
        } else {
            i += 1;
        }
    }
    None
}

fn guess_version_from_logs(workdir: &str) -> Option<String> {
    let path = Path::new(workdir).join("logs").join("latest.log");
    let meta = fs::metadata(&path).ok()?;
    if meta.len() > 4 * 1024 * 1024 {
        return None;
    }
    let raw = fs::read_to_string(&path).ok()?;
    for line in raw.lines().take(200) {
        if let Some(idx) = line.find("Starting minecraft server version") {
            let tail = &line[idx + "Starting minecraft server version".len()..];
            let v = tail.trim().trim_end_matches(['.', ',', '!']).trim();
            if !v.is_empty() && v.len() <= 24 {
                return Some(v.to_string());
            }
        }
    }
    None
}

pub fn write_mc_version_marker(workdir: &str, version: &str) -> anyhow::Result<()> {
    let path = Path::new(workdir).join("server.properties");
    let mut lines: Vec<String> = fs::read_to_string(&path)
        .map(|s| s.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default();
    lines.retain(|l| !l.trim_start().starts_with("cocktail-mc-version="));
    lines.push(format!("cocktail-mc-version={version}"));
    let mut out = lines.join(
        "
",
    );
    out.push('\n');
    fs::write(path, out)?;
    Ok(())
}

pub fn total_dir_bytes(path: &str) -> u64 {
    let mut total = 0u64;
    fn walk(dir: &Path, total: &mut u64, depth: u32) {
        if depth > 32 {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for ent in entries.flatten() {
            let p = ent.path();
            if p.is_symlink() {
                continue;
            }
            if p.is_dir() {
                walk(&p, total, depth + 1);
            } else if let Ok(meta) = p.metadata() {
                *total += meta.len();
            }
        }
    }
    walk(Path::new(path), &mut total, 0);
    total
}

/// 内联自 control 端 `util::set_property_file`：写入/更新 .properties 文件中的某个键。
fn set_property_file(path: &Path, key: &str, value: &str) -> anyhow::Result<()> {
    let mut lines: Vec<String> = if path.exists() {
        fs::read_to_string(path)?
            .lines()
            .map(|l| l.to_string())
            .collect()
    } else {
        Vec::new()
    };

    let mut found = false;
    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, _)) = trimmed.split_once('=') {
            if k.trim() == key {
                *line = format!("{key}={value}");
                found = true;
                break;
            }
        }
    }
    if !found {
        lines.push(format!("{key}={value}"));
    }
    fs::write(path, lines.join("\n") + "\n")?;
    Ok(())
}

/// 内联自 control 端 `util::write_eula`：写入 eula.txt。
fn write_eula(workdir: &str, accepted: bool) -> anyhow::Result<()> {
    let path = Path::new(workdir).join("eula.txt");
    let val = if accepted { "true" } else { "false" };
    fs::write(
        path,
        format!(
            "# By changing the setting below to TRUE you are indicating your agreement to Mojang EULA.\n\
             # https://aka.ms/MinecraftEULA\n\
             eula={val}\n"
        ),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_root_is_per_id() {
        assert_eq!(
            default_instance_root("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
            "data/instances/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
        );
    }

    #[test]
    fn workdir_overlap_is_component_aware() {
        assert!(workdirs_conflict("data/instances/a", "data/instances/a"));
        assert!(workdirs_conflict(
            "data/instances/a",
            "data/instances/a/world"
        ));
        assert!(!workdirs_conflict("data/instances/a", "data/instances/ab"));
    }

    #[test]
    fn backup_skips_jre_and_tmp() {
        assert!(skip_backup_rel("runtime"));
        assert!(skip_backup_rel("runtime/jre/bin/java.exe"));
        assert!(skip_backup_rel(".cocktail/tmp/x"));
        assert!(!skip_backup_rel("world/level.dat"));
        assert!(!skip_backup_rel("plugins/foo.jar"));
    }
}
