use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use serde::Serialize;
use tar::Archive;

use super::files;
use super::model::{InstanceStatus, InstanceView};
use super::versions;
use crate::state::AppState;
use crate::util;

pub const MAX_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

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
pub struct ImportArchiveResult {
    pub instance: InstanceView,
    pub archive: String,
    pub flattened: bool,
    pub startup: String,
    pub command_preview: String,
    pub extracted_files: u32,
}

pub struct ImportArchiveOpts {
    pub filename: String,
    pub accept_eula: bool,
    pub core: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
}

pub async fn import_archive(
    state: &AppState,
    id: &str,
    archive_path: &Path,
    opts: ImportArchiveOpts,
) -> anyhow::Result<ImportArchiveResult> {
    let view = super::get_instance(state, id)
        .await
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    if matches!(
        view.status,
        InstanceStatus::Running | InstanceStatus::Starting | InstanceStatus::Stopping
    ) {
        anyhow::bail!("请先停止实例再导入压缩包");
    }

    let name = opts.filename.to_ascii_lowercase();
    if name.ends_with(".jar") {
        let bytes = fs::read(archive_path)?;
        let inst = super::install_local_jar(
            state,
            id,
            "server.jar",
            &bytes,
            opts.core.clone(),
            opts.accept_eula,
        )
        .await?;
        return Ok(ImportArchiveResult {
            archive: opts.filename,
            flattened: false,
            startup: "jar".into(),
            command_preview: preview(&inst.spec.command, &inst.spec.args),
            extracted_files: 1,
            instance: inst,
        });
    }

    let workdir = view.spec.workdir.clone();
    let port = view.spec.port;
    let job = crate::http::Transfer::new(format!("导入 {}", opts.filename));
    job.emit("extract", 0, None);

    let archive = archive_path.to_path_buf();
    let dest = PathBuf::from(&workdir);
    let filename = opts.filename.clone();
    // extract_pack 内含 sevenz RPC 调用（async），整体改 async；fs IO 仍走 std::fs。
    let extract_out = extract_pack(&archive, &dest, &filename)
        .await
        .map_err(|e| anyhow::anyhow!("解压任务失败：{e}"))?;

    job.emit("extract", 1, Some(1));

    files::ensure_seed_files(&workdir, port, opts.accept_eula || view.spec.eula_accepted)?;
    files::sync_port(&workdir, port)?;
    if opts.accept_eula {
        util::write_eula(&workdir, true)?;
    }

    let (startup, command, args) = resolve_startup(&workdir, &opts)?;

    let mut guard = state.instances.write().await;
    let instance = guard
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;
    instance.spec.core = opts
        .core
        .filter(|c| !c.trim().is_empty())
        .unwrap_or_else(|| "custom".into());
    if let Some(cmd) = command {
        instance.spec.command = Some(cmd);
        instance.spec.args = args;
    }
    if opts.accept_eula {
        instance.spec.eula_accepted = true;
    }
    instance.updated_at = chrono::Utc::now();
    let out = instance.public_view();
    drop(guard);
    let _ = state.persist().await;

    util::audit(
        "archive.import",
        Some(id),
        serde_json::json!({
            "archive": opts.filename,
            "flattened": extract_out.flattened,
            "files": extract_out.files,
            "startup": startup,
        }),
        "api",
    );

    job.finish(1, Some(1));
    Ok(ImportArchiveResult {
        command_preview: preview(&out.spec.command, &out.spec.args),
        instance: out,
        archive: opts.filename,
        flattened: extract_out.flattened,
        startup,
        extracted_files: extract_out.files,
    })
}

struct ExtractOut {
    flattened: bool,
    files: u32,
}

async fn extract_pack(archive: &Path, workdir: &Path, filename: &str) -> anyhow::Result<ExtractOut> {
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
        files::unzip_archive(archive, dest)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar_gz(archive, dest)
    } else if name.ends_with(".tar") {
        extract_tar(archive, dest)
    } else {
        crate::sevenz::extract(archive, dest).await
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
    crate::sevenz::extract(&inner, dest).await?;
    let _ = fs::remove_file(&inner);
    if name.ends_with(".xz") && !name.ends_with(".tar.xz") {
        unwrap_nested_tarball(dest).await?;
    } else if name.ends_with(".tar.xz") || name.ends_with(".xz") {
        unwrap_nested_tarball(dest).await?;
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
    versions::has_modloader_startup(dir)
}

pub fn detect_startup(workdir: &Path) -> Option<(String, Vec<String>)> {
    if let Some(found) = versions::detect_modloader_startup(workdir) {
        return Some(found);
    }
    for jar in WELL_KNOWN_JARS {
        if workdir.join(jar).is_file() {
            return Some(util::java_jar_startup(jar));
        }
    }
    if let Some(jar) = first_root_server_jar(workdir) {
        return Some(util::java_jar_startup(&jar));
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

fn resolve_startup(
    workdir: &str,
    opts: &ImportArchiveOpts,
) -> anyhow::Result<(String, Option<String>, Vec<String>)> {
    let override_cmd = opts
        .command
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(line) = override_cmd {
        let (cmd, args) = if opts.args.is_empty() && line.contains(' ') {
            util::parse_command_line(line)?
        } else {
            (line.to_string(), opts.args.clone())
        };
        return Ok(("override".into(), Some(cmd), args));
    }
    if let Some((cmd, args)) = detect_startup(Path::new(workdir)) {
        return Ok(("detected".into(), Some(cmd), args));
    }
    Ok(("none".into(), None, Vec::new()))
}

fn preview(command: &Option<String>, args: &[String]) -> String {
    match command {
        Some(cmd) if !args.is_empty() => format!("{cmd} {}", args.join(" ")),
        Some(cmd) => cmd.clone(),
        None => "（未检测到启动命令，请手动填写）".into(),
    }
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
