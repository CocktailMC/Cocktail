//! 实例工作区文件操作的 RPC 客户端封装。
//!
//! 阶段 2 起，纯文件操作下沉到 cocktail-init 子进程；本模块仅保留：
//! - async RPC 客户端封装：每个 pub async fn 调 `crate::init_call("files.*", ...)`
//!   跨进程完成实际文件读写；
//! - 本地纯函数 `default_instance_root` / `workdirs_conflict`
//!   （含 `comparable_workdir` / `lexical_normalize`）：纯字符串/路径逻辑，
//!   control 本地算比走 RPC 便宜。
//!
//! `BackupScan` / `FileEntry` / `FileContent` / `BackupInfo` 全部从
//! `cocktail_shared::model` re-export，跨进程复用同一份定义。

use std::path::{Component, Path, PathBuf};

pub use cocktail_shared::model::{BackupInfo, BackupScan, FileContent, FileEntry};

/// 默认实例根：`data/instances/<id>`（路径分隔符统一成正斜杠）。
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

/// 在 workdir 内解析相对路径，做 zip-slip / 路径穿越防护。
pub async fn resolve_in_workdir(workdir: &str, relative: &str) -> anyhow::Result<PathBuf> {
    let v = crate::init_call(
        "files.resolve_in_workdir",
        serde_json::json!({ "workdir": workdir, "relative": relative }),
    )
    .await?;
    Ok(PathBuf::from(v["path"].as_str().unwrap_or_else(|| "")))
}

/// 列出 workdir/relative 目录下的条目。
pub async fn list_files(workdir: &str, relative: &str) -> anyhow::Result<Vec<FileEntry>> {
    let v = crate::init_call(
        "files.list_files",
        serde_json::json!({ "workdir": workdir, "relative": relative }),
    )
    .await?;
    Ok(serde_json::from_value(v)?)
}

/// 读取文本文件（≤2MiB）。
pub async fn read_file(workdir: &str, relative: &str) -> anyhow::Result<FileContent> {
    let v = crate::init_call(
        "files.read_file",
        serde_json::json!({ "workdir": workdir, "relative": relative }),
    )
    .await?;
    Ok(serde_json::from_value(v)?)
}

/// 读取二进制文件（≤512MiB）。
pub async fn read_bytes(workdir: &str, relative: &str) -> anyhow::Result<(String, Vec<u8>)> {
    let v = crate::init_call(
        "files.read_bytes",
        serde_json::json!({ "workdir": workdir, "relative": relative }),
    )
    .await?;
    let path = v["path"].as_str().unwrap_or("").to_string();
    let bytes: Vec<u8> = serde_json::from_value(v["bytes"].clone())?;
    Ok((path, bytes))
}

/// 写入文本文件。
pub async fn write_file(
    workdir: &str,
    relative: &str,
    content: &str,
) -> anyhow::Result<FileContent> {
    let v = crate::init_call(
        "files.write_file",
        serde_json::json!({ "workdir": workdir, "relative": relative, "content": content }),
    )
    .await?;
    Ok(serde_json::from_value(v)?)
}

/// 写入二进制文件。
pub async fn write_bytes(workdir: &str, relative: &str, bytes: &[u8]) -> anyhow::Result<FileEntry> {
    let v = crate::init_call(
        "files.write_bytes",
        serde_json::json!({ "workdir": workdir, "relative": relative, "bytes": bytes }),
    )
    .await?;
    Ok(serde_json::from_value(v)?)
}

/// 删除 workdir 内的相对路径。
pub async fn delete_path(workdir: &str, relative: &str) -> anyhow::Result<()> {
    let _ = crate::init_call(
        "files.delete_path",
        serde_json::json!({ "workdir": workdir, "relative": relative }),
    )
    .await?;
    Ok(())
}

/// 在 workdir 内创建目录。
pub async fn mkdir(workdir: &str, relative: &str) -> anyhow::Result<FileEntry> {
    let v = crate::init_call(
        "files.mkdir",
        serde_json::json!({ "workdir": workdir, "relative": relative }),
    )
    .await?;
    Ok(serde_json::from_value(v)?)
}

/// 检查 workdir/relative 是否为已存在的 jar 文件。
pub async fn jar_exists(workdir: &str, relative: &str) -> anyhow::Result<bool> {
    let v = crate::init_call(
        "files.jar_exists",
        serde_json::json!({ "workdir": workdir, "relative": relative }),
    )
    .await?;
    Ok(v["exists"].as_bool().unwrap_or(false))
}

/// 建好实例种子目录结构与 server.properties/eula.txt。
pub async fn ensure_seed_files(
    workdir: &str,
    port: u16,
    eula_accepted: bool,
) -> anyhow::Result<()> {
    let _ = crate::init_call(
        "files.ensure_seed_files",
        serde_json::json!({ "workdir": workdir, "port": port, "eula_accepted": eula_accepted }),
    )
    .await?;
    Ok(())
}

/// 把 server-port= 同步进 server.properties。
pub async fn sync_port(workdir: &str, port: u16) -> anyhow::Result<()> {
    let _ = crate::init_call(
        "files.sync_port",
        serde_json::json!({ "workdir": workdir, "port": port }),
    )
    .await?;
    Ok(())
}

/// 把 workdir 打包成 data/backups/<id>/<stamp>.zip。
pub async fn create_backup(instance_id: &str, workdir: &str) -> anyhow::Result<BackupInfo> {
    let v = crate::init_call(
        "files.create_backup",
        serde_json::json!({ "instance_id": instance_id, "workdir": workdir }),
    )
    .await?;
    Ok(serde_json::from_value(v)?)
}

/// 保留最近 keep 个备份，删其余。
pub async fn prune_backups(instance_id: &str, keep: u32) -> anyhow::Result<usize> {
    let v = crate::init_call(
        "files.prune_backups",
        serde_json::json!({ "instance_id": instance_id, "keep": keep }),
    )
    .await?;
    Ok(v["pruned"].as_u64().unwrap_or(0) as usize)
}

/// 返回某备份的绝对 PathBuf（带 file_name 防穿越）。
pub async fn backup_path(instance_id: &str, backup_id: &str) -> anyhow::Result<PathBuf> {
    let v = crate::init_call(
        "files.backup_path",
        serde_json::json!({ "instance_id": instance_id, "backup_id": backup_id }),
    )
    .await?;
    Ok(PathBuf::from(v["path"].as_str().unwrap_or("")))
}

/// 返回某备份的 BackupInfo（id/created_at/path/size）。
pub async fn backup_meta(instance_id: &str, backup_id: &str) -> anyhow::Result<BackupInfo> {
    let v = crate::init_call(
        "files.backup_meta",
        serde_json::json!({ "instance_id": instance_id, "backup_id": backup_id }),
    )
    .await?;
    Ok(serde_json::from_value(v)?)
}

/// 扫描备份内容（zip 或目录），返回 BackupScan。
pub async fn inspect_backup_zip(path: &Path) -> anyhow::Result<BackupScan> {
    let v = crate::init_call(
        "files.inspect_backup_zip",
        serde_json::json!({ "path": path.to_string_lossy() }),
    )
    .await?;
    Ok(serde_json::from_value(v)?)
}

/// 列出某实例的所有备份。
pub async fn list_backups(instance_id: &str) -> anyhow::Result<Vec<BackupInfo>> {
    let v = crate::init_call(
        "files.list_backups",
        serde_json::json!({ "instance_id": instance_id }),
    )
    .await?;
    Ok(serde_json::from_value(v)?)
}

/// 删除某实例的指定备份。
pub async fn delete_backup(instance_id: &str, backup_id: &str) -> anyhow::Result<()> {
    let _ = crate::init_call(
        "files.delete_backup",
        serde_json::json!({ "instance_id": instance_id, "backup_id": backup_id }),
    )
    .await?;
    Ok(())
}

/// 把备份内容解压/复制回 workdir。
pub async fn restore_backup(
    instance_id: &str,
    backup_id: &str,
    workdir: &str,
) -> anyhow::Result<()> {
    let _ = crate::init_call(
        "files.restore_backup",
        serde_json::json!({
            "instance_id": instance_id,
            "backup_id": backup_id,
            "workdir": workdir,
        }),
    )
    .await?;
    Ok(())
}

/// 通用解压 zip 到 dest。
pub async fn unzip_archive(zip_path: &Path, dest: &Path) -> anyhow::Result<()> {
    let _ = crate::init_call(
        "files.unzip_archive",
        serde_json::json!({
            "zip_path": zip_path.to_string_lossy(),
            "dest": dest.to_string_lossy(),
        }),
    )
    .await?;
    Ok(())
}

/// 复制实例目录树（带 skip_logs/copy_data 选项）。
pub async fn copy_instance_tree(
    src: &str,
    dst: &str,
    copy_data: bool,
    skip_logs: bool,
) -> anyhow::Result<u32> {
    let v = crate::init_call(
        "files.copy_instance_tree",
        serde_json::json!({
            "src": src,
            "dst": dst,
            "copy_data": copy_data,
            "skip_logs": skip_logs,
        }),
    )
    .await?;
    Ok(v["copied"].as_u64().unwrap_or(0) as u32)
}

/// 把 workdir/relative 子目录打包成 dest zip。
pub async fn pack_subdir_zip(workdir: &str, relative: &str, dest: &Path) -> anyhow::Result<u64> {
    let v = crate::init_call(
        "files.pack_subdir_zip",
        serde_json::json!({
            "workdir": workdir,
            "relative": relative,
            "dest": dest.to_string_lossy(),
        }),
    )
    .await?;
    Ok(v["size"].as_u64().unwrap_or(0))
}

/// 把 zip 字节流解压合并到 workdir/relative 下。
pub async fn extract_zip_into(workdir: &str, relative: &str, bytes: &[u8]) -> anyhow::Result<u32> {
    let v = crate::init_call(
        "files.extract_zip_into",
        serde_json::json!({
            "workdir": workdir,
            "relative": relative,
            "bytes": bytes,
        }),
    )
    .await?;
    Ok(v["files"].as_u64().unwrap_or(0) as u32)
}

/// 根据 properties/jars/logs 推断 MC 版本。
pub async fn guess_mc_version(workdir: &str) -> anyhow::Result<Option<String>> {
    let v = crate::init_call(
        "files.guess_mc_version",
        serde_json::json!({ "workdir": workdir }),
    )
    .await?;
    Ok(serde_json::from_value(v["version"].clone())?)
}

/// 把 cocktail-mc-version= 写进 server.properties。
pub async fn write_mc_version_marker(workdir: &str, version: &str) -> anyhow::Result<()> {
    let _ = crate::init_call(
        "files.write_mc_version_marker",
        serde_json::json!({ "workdir": workdir, "version": version }),
    )
    .await?;
    Ok(())
}

/// 递归统计某目录占用字节数。
pub async fn total_dir_bytes(path: &str) -> anyhow::Result<u64> {
    let v = crate::init_call("files.total_dir_bytes", serde_json::json!({ "path": path })).await?;
    Ok(v["bytes"].as_u64().unwrap_or(0))
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
}
