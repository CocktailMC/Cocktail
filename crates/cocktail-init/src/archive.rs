//! 服务端压缩包导入与启动探测：阶段 2 起 cocktail-init 接管压缩包解压、
//! 嵌套 tarball 展开、目录扁平化、越界路径防护、合并到 workdir，以及
//! 启动命令解析。
//!
//! control 端通过 IPC `archive.*` 调用本模块：
//! - `archive.extract_pack`：解压 → 去嵌套 → 去 junk → 扁平化 → 越界防护 →
//!   合并到 workdir，返回 `{ flattened, files }`；
//! - `archive.resolve_startup`：根据 workdir + 用户 override command/args，
//!   返回 `{ startup, command, args }`。
//!
//! `import_archive` 主入口（含状态校验、AppState mutation、audit）留 control 端，
//! 它调本模块完成纯 fs 操作；状态校验 + 持久化属 control 职责。
//!
//! TODO 阶段 2：原 control 端 `http::Transfer` 进度推送（"extract" phase）暂砍，
//! 待 event push 通道就位后用 init→control 单向事件回传 progress。
//! TODO 阶段 3：init 不可达时考虑 fallback 到本地实现（兼容单进程老部署）。

use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use serde::Serialize;
use tar::Archive;

const WELL_KNOWN_JARS: &[&str] = &[
    "server.jar",
    "paper.jar",
    "purpur.jar",
    "folia.jar",
    "leaves.jar",
    "spigot.jar",
    "bukkit.jar",
    "mohist.jar",
    "arclight.jar",
    "banner.jar",
    "fabric-server-launch.jar",
    "quilt-server-launch.jar",
    "velocity.jar",
    "bungeecord.jar",
    "waterfall.jar",
];

const SCRIPT_NAMES: &[&str] = &[
    "run.bat",
    "start.bat",
    "launch.bat",
    "start.cmd",
    "run.cmd",
    "run.sh",
    "start.sh",
    "launch.sh",
    "start.command",
];

const DATA_DIR_NAMES: &[&str] = &[
    "world",
    "worlds",
    "plugins",
    "mods",
    "config",
    "configs",
    "libraries",
    "versions",
    "logs",
    "crash-reports",
    "datapacks",
    "kubejs",
    "defaultconfigs",
    "cache",
    "runtime",
];

#[derive(Debug, Clone, Serialize)]
pub struct ExtractOut {
    pub flattened: bool,
    pub files: u32,
}

/// 解压 → 去嵌套 → 去 junk → 扁平化 → 越界防护 → 合并到 workdir。
/// staging 用 `workdir/.cocktail/tmp/unpack-<uuid>`，结束清理。
pub async fn extract_pack(
    archive: &Path,
    workdir: &Path,
    filename: &str,
) -> anyhow::Result<ExtractOut> {
    fs::create_dir_all(workdir)?;
    let staging = workdir
        .join(".cocktail")
        .join("tmp")
        .join(format!("unpack-{}", uuid::Uuid::new_v4()));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    let outcome: anyhow::Result<ExtractOut> = async {
        extract_archive(archive, &staging, filename).await?;
        unwrap_nested_tarball(&staging).await?;
        strip_junk(&staging)?;
        let flattened = maybe_flatten(&staging)?;
        reject_escapes(&staging)?;
        let files = merge_tree(&staging, workdir)?;
        Ok(ExtractOut { flattened, files })
    }
    .await;
    let _ = fs::remove_dir_all(&staging);
    outcome
}

async fn extract_archive(archive: &Path, dest: &Path, filename: &str) -> anyhow::Result<()> {
    let name = filename.to_ascii_lowercase();
    if name.ends_with(".zip") {
        crate::files::unzip_archive(archive, dest)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar_gz(archive, dest)
    } else if name.ends_with(".tar") {
        extract_tar(archive, dest)
    } else {
        crate::sevenz::extract(archive, dest)
    }
}

fn extract_tar_gz(path: &Path, dest: &Path) -> anyhow::Result<()> {
    let file = File::open(path)?;
    let gz = GzDecoder::new(file);
    unpack_tar(Archive::new(gz), dest)
}

fn extract_tar(path: &Path, dest: &Path) -> anyhow::Result<()> {
    let file = File::open(path)?;
    unpack_tar(Archive::new(file), dest)
}

fn unpack_tar<R: io::Read>(mut archive: Archive<R>, dest: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let rel = entry.path()?.into_owned();
        let Some(out) = safe_join(dest, &rel) else {
            continue;
        };
        if entry.header().entry_type().is_symlink() || entry.header().entry_type().is_hard_link() {
            continue;
        }
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            entry.unpack(&out)?;
        }
    }
    Ok(())
}

async fn unwrap_nested_tarball(dest: &Path) -> anyhow::Result<()> {
    let Ok(entries) = fs::read_dir(dest) else {
        return Ok(());
    };
    let files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    if files.len() != 1 {
        return Ok(());
    }
    let name = files[0]
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !(name.ends_with(".tar")
        || name.ends_with(".tar.gz")
        || name.ends_with(".tgz")
        || name.ends_with(".tar.xz")
        || name.ends_with(".xz"))
    {
        return Ok(());
    }
    let inner = files[0].clone();
    crate::sevenz::extract(&inner, dest)?;
    let _ = fs::remove_file(&inner);
    if name.ends_with(".xz") && !name.ends_with(".tar.xz") {
        // 递归 async fn 必须装箱，否则 future 尺寸无限
        Box::pin(unwrap_nested_tarball(dest)).await?;
    } else if name.ends_with(".tar.xz") || name.ends_with(".xz") {
        Box::pin(unwrap_nested_tarball(dest)).await?;
    }
    Ok(())
}

fn maybe_flatten(dest: &Path) -> anyhow::Result<bool> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for ent in fs::read_dir(dest)? {
        let path = ent?.path();
        if is_junk_name(&file_name(&path)) {
            continue;
        }
        if path.is_dir() {
            dirs.push(path);
        } else {
            files.push(path);
        }
    }
    if dirs.len() != 1 || !files.is_empty() {
        return Ok(false);
    }
    let inner = &dirs[0];
    let inner_name = file_name(inner).to_ascii_lowercase();
    let flatten = looks_like_server_root(inner) || !DATA_DIR_NAMES.contains(&inner_name.as_str());
    if !flatten {
        return Ok(false);
    }
    hoist_children(inner, dest)?;
    Ok(true)
}

fn hoist_children(inner: &Path, dest: &Path) -> anyhow::Result<()> {
    for ent in fs::read_dir(inner)? {
        let from = ent?.path();
        let to = dest.join(from.file_name().unwrap_or_default());
        if to.exists() {
            if from.is_dir() && to.is_dir() {
                merge_tree(&from, &to)?;
                let _ = fs::remove_dir_all(&from);
                continue;
            }
            if to.is_dir() {
                fs::remove_dir_all(&to)?;
            } else {
                fs::remove_file(&to)?;
            }
        }
        fs::rename(&from, &to).or_else(|_| -> anyhow::Result<()> {
            if from.is_dir() {
                merge_tree(&from, &to)?;
                fs::remove_dir_all(&from)?;
            } else {
                fs::copy(&from, &to)?;
                fs::remove_file(&from)?;
            }
            Ok(())
        })?;
    }
    let _ = fs::remove_dir_all(inner);
    Ok(())
}

fn merge_tree(src: &Path, dst: &Path) -> anyhow::Result<u32> {
    fs::create_dir_all(dst)?;
    let mut files = 0u32;
    for ent in fs::read_dir(src)? {
        let ent = ent?;
        let name = ent.file_name();
        let name_str = name.to_string_lossy();
        if name_str == ".cocktail" || is_junk_name(&name_str) {
            continue;
        }
        let from = ent.path();
        let to = dst.join(&name);
        if from.is_symlink() {
            continue;
        }
        if from.is_dir() {
            files += merge_tree(&from, &to)?;
        } else {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&from, &to)?;
            files += 1;
        }
    }
    Ok(files)
}

fn strip_junk(root: &Path) -> anyhow::Result<()> {
    let Ok(entries) = fs::read_dir(root) else {
        return Ok(());
    };
    for ent in entries.flatten() {
        let path = ent.path();
        if is_junk_name(&file_name(&path)) {
            if path.is_dir() {
                let _ = fs::remove_dir_all(&path);
            } else {
                let _ = fs::remove_file(&path);
            }
        }
    }
    Ok(())
}

fn reject_escapes(root: &Path) -> anyhow::Result<()> {
    let canon = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    fn walk(dir: &Path, root: &Path) -> anyhow::Result<()> {
        for ent in fs::read_dir(dir)? {
            let path = ent?.path();
            if path.is_symlink() {
                let _ = fs::remove_file(&path);
                continue;
            }
            if let Ok(c) = fs::canonicalize(&path) {
                if !c.starts_with(root) {
                    anyhow::bail!("压缩包包含越界路径：{}", path.display());
                }
            }
            if path.is_dir() {
                walk(&path, root)?;
            }
        }
        Ok(())
    }
    walk(&canon, &canon)
}

fn looks_like_server_root(dir: &Path) -> bool {
    if dir.join("server.properties").is_file() || dir.join("eula.txt").is_file() {
        return true;
    }
    for jar in WELL_KNOWN_JARS {
        if dir.join(jar).is_file() {
            return true;
        }
    }
    for script in SCRIPT_NAMES {
        if dir.join(script).is_file() {
            return true;
        }
    }
    crate::versions::detect_modloader_startup(dir).is_some()
}

pub fn detect_startup(workdir: &Path) -> Option<(String, Vec<String>)> {
    if let Some(found) = crate::versions::detect_modloader_startup(workdir) {
        return Some(found);
    }
    for jar in WELL_KNOWN_JARS {
        if workdir.join(jar).is_file() {
            return Some(java_jar_startup(jar));
        }
    }
    if let Some(jar) = first_root_server_jar(workdir) {
        return Some(java_jar_startup(&jar));
    }
    for script in SCRIPT_NAMES {
        if workdir.join(script).is_file() {
            return Some(script_startup(script));
        }
    }
    None
}

fn first_root_server_jar(workdir: &Path) -> Option<String> {
    let mut jars = Vec::new();
    let entries = fs::read_dir(workdir).ok()?;
    for ent in entries.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        let lower = name.to_ascii_lowercase();
        if !lower.ends_with(".jar") {
            continue;
        }
        if lower.contains("installer") || lower.ends_with("-sources.jar") {
            continue;
        }
        jars.push(name);
    }
    jars.sort_by(|a, b| {
        server_jar_score(a)
            .cmp(&server_jar_score(b))
            .then(a.len().cmp(&b.len()))
    });
    jars.into_iter().next()
}

fn server_jar_score(name: &str) -> u8 {
    let n = name.to_ascii_lowercase();
    if n == "server.jar" {
        0
    } else if n.contains("paper")
        || n.contains("purpur")
        || n.contains("folia")
        || n.contains("spigot")
        || n.contains("fabric")
        || n.contains("quilt")
        || n.contains("forge")
        || n.contains("neoforge")
    {
        1
    } else if n.contains("snapshot") || n.contains("javadoc") {
        9
    } else {
        2
    }
}

fn script_startup(script: &str) -> (String, Vec<String>) {
    let lower = script.to_ascii_lowercase();
    if lower.ends_with(".bat") || lower.ends_with(".cmd") {
        ("cmd.exe".into(), vec!["/C".into(), script.to_string()])
    } else {
        ("sh".into(), vec![script.to_string()])
    }
}

pub fn resolve_startup(
    workdir: &str,
    command: Option<&str>,
    args: &[String],
) -> anyhow::Result<(String, Option<String>, Vec<String>)> {
    let override_cmd = command.map(str::trim).filter(|s| !s.is_empty());
    if let Some(line) = override_cmd {
        let (cmd, args_out) = if args.is_empty() && line.contains(' ') {
            parse_command_line(line)?
        } else {
            (line.to_string(), args.to_vec())
        };
        return Ok(("override".into(), Some(cmd), args_out));
    }
    if let Some((cmd, args_out)) = detect_startup(Path::new(workdir)) {
        return Ok(("detected".into(), Some(cmd), args_out));
    }
    Ok(("none".into(), None, Vec::new()))
}

/// 内联自 control/src/util.rs：构造 `java -jar <jar> nogui` 启动命令。
/// control 端 `crate::util::java_jar_startup` 不在 init 端可用，参照
/// init/src/versions.rs 内联范本复制一份。
fn java_jar_startup(jar_rel: &str) -> (String, Vec<String>) {
    let jar = jar_rel
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string();
    ("java".into(), vec!["-jar".into(), jar, "nogui".into()])
}

/// 内联自 control/src/util.rs：把一行命令拆成 (command, args)。
/// control 端 `crate::util::parse_command_line` 不在 init 端可用。
fn parse_command_line(line: &str) -> anyhow::Result<(String, Vec<String>)> {
    let parts = split_command_line(line);
    let mut iter = parts.into_iter();
    let command = iter
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("启动命令为空"))?;
    Ok((command, iter.collect()))
}

fn split_command_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in line.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            None => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn is_junk_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "__macosx"
        || n == ".ds_store"
        || n == "thumbs.db"
        || n == "desktop.ini"
        || n.starts_with("._")
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn safe_join(base: &Path, rel: &Path) -> Option<PathBuf> {
    let mut out = base.to_path_buf();
    for c in rel.components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattens_pack_root() {
        let root = std::env::temp_dir().join(format!("cocktail-flatten-{}", uuid::Uuid::new_v4()));
        let inner = root.join("MyPack");
        fs::create_dir_all(inner.join("plugins")).unwrap();
        fs::write(inner.join("server.jar"), b"jar").unwrap();
        assert!(maybe_flatten(&root).unwrap());
        assert!(root.join("server.jar").is_file());
        assert!(root.join("plugins").is_dir());
        assert!(!inner.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn does_not_flatten_plugins_only() {
        let root = std::env::temp_dir().join(format!("cocktail-plugins-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("plugins")).unwrap();
        fs::write(root.join("plugins").join("x.jar"), b"jar").unwrap();
        assert!(!maybe_flatten(&root).unwrap());
        assert!(root.join("plugins").join("x.jar").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detects_server_jar() {
        let root = std::env::temp_dir().join(format!("cocktail-detect-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("paper-1.21.jar"), b"jar").unwrap();
        let (cmd, args) = detect_startup(&root).unwrap();
        assert_eq!(cmd, "java");
        assert!(args.contains(&"paper-1.21.jar".into()));
        let _ = fs::remove_dir_all(&root);
    }
}
