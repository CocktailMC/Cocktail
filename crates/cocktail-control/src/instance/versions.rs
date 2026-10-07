//! 服务端核心版本管理客户端：阶段 2 起实际逻辑搬到 cocktail-init 子进程。
//!
//! 本模块只做：
//! - async RPC 客户端封装：每个 pub async fn 调 `crate::init_call("versions.*", ...)`
//!   跨进程完成版本元数据抓取、服务端 jar 下载、Forge/Fabric/NeoForge 安装器调用；
//! - 本地纯 fs 函数 `is_known_core` / `core_needs_eula` /
//!   `has_modloader_startup` / `detect_modloader_startup`（含 `find_files` /
//!   `pathdiff`）：纯本地目录扫描 + 字符串逻辑，单机算比走 RPC 便宜，
//!   archive.rs / registry.rs 在 sync 上下文里直接调它们，不引入 async 级联。
//!   init 端 `versions.rs` 也需要 `detect_modloader_startup`（install_forge /
//!   install_neoforge 内部用），按 java.rs 的 `apply_isolated_env` 范本在
//!   init 端内联一份。
//!
//! `CoreVersion` / `CoreLoader` / `InstallRequest` 全部从
//! `cocktail_shared::versions` re-export，跨进程复用同一份定义，
//! 调用方 `crate::instance::versions::CoreVersion` 等无需改动。
//!
//! TODO 阶段 3：init 不可达时考虑 fallback 到本地实现（兼容单进程老部署）。

use std::fs;
use std::path::{Path, PathBuf};

pub use cocktail_shared::versions::{CoreLoader, CoreVersion, InstallRequest};

const SUPPORTED: &[&str] = &[
    "paper", "folia", "purpur", "leaves", "vanilla", "fabric", "quilt", "forge", "neoforge",
    "mohist", "banner", "arclight",
];

pub fn is_known_core(core: &str) -> bool {
    SUPPORTED.contains(&core)
}

pub fn core_needs_eula(core: &str) -> bool {
    core != "demo"
}

/// 列出某核心的可用 MC 版本。
pub async fn list_versions(core: &str) -> anyhow::Result<Vec<CoreVersion>> {
    let v = crate::init_call(
        "versions.list_versions",
        serde_json::json!({ "core": core }),
    )
    .await?;
    Ok(serde_json::from_value(v)?)
}

/// 列出某 MC 版本下可选 loader。
pub async fn list_loaders(core: &str, version: &str) -> anyhow::Result<Vec<CoreLoader>> {
    let v = crate::init_call(
        "versions.list_loaders",
        serde_json::json!({ "core": core, "version": version }),
    )
    .await?;
    Ok(serde_json::from_value(v)?)
}

/// 下载并落地服务端 jar / 跑 modloader 安装器。返回 (command, args) 启动命令。
pub async fn download_and_install(
    workdir: &str,
    core: &str,
    version: &str,
    loader: Option<&str>,
) -> anyhow::Result<(String, Vec<String>)> {
    #[derive(serde::Deserialize)]
    struct R {
        command: String,
        args: Vec<String>,
    }
    let v = crate::init_call(
        "versions.download_and_install",
        serde_json::json!({
            "workdir": workdir,
            "core": core,
            "version": version,
            "loader": loader,
        }),
    )
    .await?;
    let r: R = serde_json::from_value(v)?;
    Ok((r.command, r.args))
}

pub(crate) fn has_modloader_startup(workdir: &Path) -> bool {
    detect_modloader_startup(workdir).is_some()
}

pub(crate) fn detect_modloader_startup(workdir: &Path) -> Option<(String, Vec<String>)> {
    let arg_name = if cfg!(windows) {
        "win_args.txt"
    } else {
        "unix_args.txt"
    };
    let mut args_files = Vec::new();
    find_files(workdir, arg_name, 8, &mut args_files);
    if args_files.is_empty() && cfg!(windows) {
        find_files(workdir, "unix_args.txt", 8, &mut args_files);
    }
    args_files.sort_by_key(|p| {
        let s = p.to_string_lossy().to_ascii_lowercase();
        let score = if s.contains("neoforged") || s.contains("minecraftforge") {
            0
        } else {
            1
        };
        (score, s.len())
    });
    if let Some(args_file) = args_files.first() {
        let rel = pathdiff(workdir, args_file);
        let jvm = workdir.join("user_jvm_args.txt");
        if !jvm.exists() {
            let _ = fs::write(&jvm, "# Cocktail-managed JVM args\n");
        }
        return Some((
            "java".into(),
            vec![
                "@user_jvm_args.txt".into(),
                format!("@{rel}"),
                "nogui".into(),
            ],
        ));
    }
    let mut jars = Vec::new();
    if let Ok(entries) = fs::read_dir(workdir) {
        for e in entries.flatten() {
            let p = e.path();
            let name = p.file_name()?.to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".jar")
                && !name.contains("installer")
                && (name.contains("forge") || name.contains("neoforge") || name.contains("shim"))
            {
                jars.push(p.file_name()?.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    jars.sort();
    let jar = jars.into_iter().next()?;
    Some(crate::util::java_jar_startup(&jar))
}

fn pathdiff(base: &Path, file: &Path) -> String {
    file.strip_prefix(base)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

fn find_files(dir: &Path, name: &str, depth: u32, out: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            find_files(&p, name, depth - 1, out);
        } else if p.file_name().and_then(|n| n.to_str()) == Some(name) {
            out.push(p);
        }
    }
}
