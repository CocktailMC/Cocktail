//! 服务端压缩包导入客户端：阶段 2 起实际解压/启动探测逻辑搬到 cocktail-init 子进程。
//!
//! 本模块只做：
//! - `import_archive` 主入口：实例状态校验、jar 直接落地（走 registry::install_local_jar）、
//!   seed files/sync port/eula 写入（已 RPC）、调 init `archive.*` RPC 完成解压与启动探测、
//!   AppState mutation 与 audit；属 control 职责，留本地；
//! - async RPC 客户端封装：`extract_pack` / `resolve_startup` 调
//!   `crate::init_call("archive.*", ...)` 跨进程完成 fs 操作；
//! - `ImportArchiveResult` / `ImportArchiveOpts` / `MAX_ARCHIVE_BYTES`：纯数据，留本地；
//! - `preview`：纯字符串格式化，留本地（仅本模块用）。
//!
//! 解压进度通过 init→control 的 extract.* 事件回传。
//! TODO 阶段 3：init 不可达时考虑 fallback 到本地实现（兼容单进程老部署）。

use std::fs;
use std::path::Path;

use serde::Serialize;

use super::files;
use super::model::{InstanceStatus, InstanceView};
use crate::state::AppState;
use crate::util;

pub const MAX_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

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
    let archive = archive_path.to_path_buf();
    let dest = std::path::PathBuf::from(&workdir);
    let filename = opts.filename.clone();

    let extract_out = extract_pack(&archive, &dest, &filename)
        .await
        .map_err(|e| anyhow::anyhow!("解压任务失败：{e}"))?;

    files::ensure_seed_files(&workdir, port, opts.accept_eula || view.spec.eula_accepted).await?;
    files::sync_port(&workdir, port).await?;
    if opts.accept_eula {
        util::write_eula(&workdir, true)?;
    }

    let (startup, command, args) = resolve_startup(&workdir, &opts).await?;

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

    Ok(ImportArchiveResult {
        command_preview: preview(&out.spec.command, &out.spec.args),
        instance: out,
        archive: opts.filename,
        flattened: extract_out.flattened,
        startup,
        extracted_files: extract_out.files,
    })
}

/// 解压 → 去嵌套 → 去 junk → 扁平化 → 越界防护 → 合并到 workdir。
/// 走 init RPC `archive.extract_pack`，返回 `{ flattened, files }`。
async fn extract_pack(
    archive: &Path,
    workdir: &Path,
    filename: &str,
) -> anyhow::Result<ExtractOut> {
    #[derive(serde::Deserialize)]
    struct R {
        flattened: bool,
        files: u32,
    }
    let progress = cocktail_shared::progress::TransferContext::new(format!("导入 {filename}"));
    let v = crate::init_call(
        "archive.extract_pack",
        serde_json::json!({
            "progress": progress,
            "archive_path": archive.to_string_lossy(),
            "workdir": workdir.to_string_lossy(),
            "filename": filename,
        }),
    )
    .await?;
    let r: R = serde_json::from_value(v)?;
    Ok(ExtractOut {
        flattened: r.flattened,
        files: r.files,
    })
}

struct ExtractOut {
    flattened: bool,
    files: u32,
}

/// 解析启动命令：用户 override 优先，否则扫描 workdir 自动探测。
/// 走 init RPC `archive.resolve_startup`，返回 `{ startup, command, args }`。
async fn resolve_startup(
    workdir: &str,
    opts: &ImportArchiveOpts,
) -> anyhow::Result<(String, Option<String>, Vec<String>)> {
    #[derive(serde::Deserialize)]
    struct R {
        startup: String,
        command: Option<String>,
        args: Vec<String>,
    }
    let v = crate::init_call(
        "archive.resolve_startup",
        serde_json::json!({
            "workdir": workdir,
            "command": opts.command,
            "args": opts.args,
        }),
    )
    .await?;
    let r: R = serde_json::from_value(v)?;
    Ok((r.startup, r.command, r.args))
}

fn preview(command: &Option<String>, args: &[String]) -> String {
    match command {
        Some(cmd) if !args.is_empty() => format!("{cmd} {}", args.join(" ")),
        Some(cmd) => cmd.clone(),
        None => "（未检测到启动命令，请手动填写）".into(),
    }
}
