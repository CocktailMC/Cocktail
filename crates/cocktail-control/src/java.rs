//! Java 运行时管理客户端：阶段 2 起实际逻辑搬到 cocktail-init 子进程。
//!
//! 本模块只做：
//! - async RPC 客户端封装：每个 pub async fn 调 `crate::init_call("java.*", ...)`
//!   跨进程完成 JRE 探测/下载/解压/缓存校验；
//! - 本地纯函数 `recommended_java_major` / `docker_image_for` /
//!   `runtime_id` / `instance_jre_home` / `rewrite_java_command` /
//!   `apply_java_home` / `apply_isolated_env`（含 `abs_workdir` /
//!   `native_path` / `java_prop_path`）：纯字符串/路径/环境变量逻辑，
//!   control 本地算比走 RPC 便宜，且 `apply_isolated_env` 要在 control
//!   spawn 子进程前直接注入 env。
//!
//! `ImageType` / `InstalledRuntime` / `SystemJava` /
//! `JavaInventory` / `InstallJavaRequest` / `EnsureJavaRequest` /
//! `EnsureJavaResponse` 全部从 `cocktail_shared::java` re-export，
//! 跨进程复用同一份定义，调用方 `crate::java::ImageType` 等无需改动。
//!
//! TODO 阶段 3：init 不可达时考虑 fallback 到本地实现（兼容单进程老部署）。

use std::path::{Path, PathBuf};

pub use cocktail_shared::java::{
    EnsureJavaRequest, EnsureJavaResponse, ImageType, InstallJavaRequest, InstalledRuntime,
    JavaInventory, SystemJava,
};

/// 根据 Minecraft 版本号推荐 Java 主版本（无 IO，control 本地算）。
pub fn recommended_java_major(mc: Option<&str>) -> u32 {
    let Some(id) = mc.map(str::trim).filter(|s| !s.is_empty()) else {
        return 21;
    };
    let nums: Vec<u32> = id
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    let minor = if nums.first() == Some(&1) {
        nums.get(1).copied().unwrap_or(0)
    } else {
        nums.first().copied().unwrap_or(0)
    };
    let patch = if nums.first() == Some(&1) {
        nums.get(2).copied().unwrap_or(0)
    } else {
        nums.get(1).copied().unwrap_or(0)
    };
    if minor >= 21 || (minor == 20 && patch >= 5) {
        21
    } else if minor >= 17 {
        17
    } else {
        8
    }
}

/// Docker 镜像名（无 IO，control 本地算）。
pub fn docker_image_for(major: u32) -> String {
    format!("eclipse-temurin:{major}-jre")
}

/// 运行时 id（无 IO，control 本地算）。
pub fn runtime_id(major: u32, image: ImageType) -> String {
    format!("temurin-{major}-{}", image.as_str())
}

pub const INSTANCE_JRE_REL: &str = "runtime/jre";

/// 实例 JRE 目录（无 IO，control 本地算）。
pub fn instance_jre_home(workdir: &Path) -> PathBuf {
    INSTANCE_JRE_REL
        .split('/')
        .fold(workdir.to_path_buf(), |p, seg| p.join(seg))
}

/// 替换启动命令中的 java 为绝对路径（无 IO，control 本地算）。
pub fn rewrite_java_command(command: Option<String>, java_bin: &Path) -> Option<String> {
    let path = java_bin.to_string_lossy().into_owned();
    match command {
        None => Some(path),
        Some(cmd) if crate::util::is_java_command(&cmd) => Some(path),
        Some(cmd) => Some(cmd),
    }
}

/// 把 java 的 bin 目录塞进 PATH、设 JAVA_HOME（无 IO，control 本地算）。
pub fn apply_java_home(cmd: &mut tokio::process::Command, bin: &str) {
    let path = Path::new(bin);
    if !crate::util::is_java_command(bin) {
        return;
    }
    if let Some(bin_dir) = path.parent() {
        if let Ok(old) = std::env::var("PATH") {
            let sep = if cfg!(windows) { ';' } else { ':' };
            cmd.env("PATH", format!("{}{sep}{old}", bin_dir.display()));
        } else {
            cmd.env("PATH", bin_dir);
        }
        if let Some(home) = bin_dir.parent() {
            if home.join("release").is_file() || home.join("lib").is_dir() {
                cmd.env("JAVA_HOME", home);
            }
        }
    }
}

/// 把实例 workdir 隔离成 HOME/TEMP/JAVA_TOOL_OPTIONS（无 IO，control 本地算）。
/// 在 control spawn 子进程前直接注入 env，不能走 RPC。
pub fn apply_isolated_env(cmd: &mut tokio::process::Command, java_bin: &str, workdir: &str) {
    apply_java_home(cmd, java_bin);
    let work = abs_workdir(workdir);
    let cocktail = work.join(".cocktail");
    let tmp = cocktail.join("tmp");
    let appdata = cocktail.join("appdata");
    let _ = std::fs::create_dir_all(&tmp);
    let _ = std::fs::create_dir_all(&appdata);

    let work_env = native_path(&work);
    let tmp_env = native_path(&tmp);
    let work_prop = java_prop_path(&work);
    let tmp_prop = java_prop_path(&tmp);

    cmd.env("HOME", &work_env);
    cmd.env("TEMP", &tmp_env);
    cmd.env("TMP", &tmp_env);
    cmd.env("TMPDIR", &tmp_env);
    cmd.env_remove("_JAVA_OPTIONS");
    cmd.env_remove("JDK_JAVA_OPTIONS");
    cmd.env_remove("JAVA_TOOL_OPTIONS");
    cmd.env(
        "JAVA_TOOL_OPTIONS",
        format!("-Duser.home={work_prop} -Djava.io.tmpdir={tmp_prop}"),
    );

    #[cfg(windows)]
    {
        cmd.env("USERPROFILE", &work_env);
        cmd.env("APPDATA", native_path(&appdata));
        cmd.env("LOCALAPPDATA", native_path(&appdata));
        if let Some(s) = work.to_str() {
            if s.len() >= 2 && s.as_bytes()[1] == b':' {
                cmd.env("HOMEDRIVE", &s[..2]);
                cmd.env("HOMEPATH", &s[2..]);
            }
        }
    }
}

fn abs_workdir(workdir: &str) -> PathBuf {
    let p = PathBuf::from(workdir);
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(p))
            .unwrap_or_else(|_| PathBuf::from(workdir))
    }
}

fn native_path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn java_prop_path(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if s.chars().any(|c| c.is_whitespace()) {
        format!("\"{s}\"")
    } else {
        s
    }
}

/// 盘点系统 + 已安装的 JRE。
pub async fn inventory() -> anyhow::Result<JavaInventory> {
    let v = crate::init_call("java.inventory", serde_json::json!({})).await?;
    Ok(serde_json::from_value(v)?)
}

/// 按 major + image 下载并安装一份 Temurin JRE/JDK。
pub async fn install(major: u32, image: ImageType) -> anyhow::Result<InstalledRuntime> {
    let v = crate::init_call(
        "java.install",
        serde_json::json!({ "major": major, "image_type": image.as_str() }),
    )
    .await?;
    Ok(serde_json::from_value(v)?)
}

/// 按 id 删除已安装的运行时。原 sync，改 async（实际删除在 init 子进程）。
pub async fn remove(id: &str) -> anyhow::Result<()> {
    let _ = crate::init_call("java.remove", serde_json::json!({ "id": id })).await?;
    Ok(())
}

/// 按 EnsureJavaRequest 选合适的 JRE，找不到就装。
pub async fn ensure_api(req: EnsureJavaRequest) -> anyhow::Result<EnsureJavaResponse> {
    let v = crate::init_call("java.ensure_api", serde_json::to_value(&req)?).await?;
    Ok(serde_json::from_value(v)?)
}

/// 按 workdir + java_major + mc_version 确保实例有可用 JRE。
pub async fn ensure_for_spec(
    workdir: &str,
    java_major: Option<u32>,
    mc_version: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let v = crate::init_call(
        "java.ensure_for_spec",
        serde_json::json!({
            "workdir": workdir,
            "java_major": java_major,
            "mc_version": mc_version,
        }),
    )
    .await?;
    let bin = v["java_bin"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("java.ensure_for_spec 响应缺少 java_bin"))?
        .to_string();
    Ok(PathBuf::from(bin))
}

/// 与 ensure_for_spec 同参同返（versions.rs 直接调它）。
pub async fn ensure_instance_jre(
    workdir: &str,
    java_major: Option<u32>,
    mc_version: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let v = crate::init_call(
        "java.ensure_instance_jre",
        serde_json::json!({
            "workdir": workdir,
            "java_major": java_major,
            "mc_version": mc_version,
        }),
    )
    .await?;
    let bin = v["java_bin"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("java.ensure_instance_jre 响应缺少 java_bin"))?
        .to_string();
    Ok(PathBuf::from(bin))
}

/// 探测 PATH 上的 java。
pub async fn probe_system() -> anyhow::Result<Option<SystemJava>> {
    let v = crate::init_call("java.probe_system", serde_json::json!({})).await?;
    if v.is_null() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_value(v)?))
}

/// 列出 data/java 下所有已安装运行时。
pub async fn list_installed() -> anyhow::Result<Vec<InstalledRuntime>> {
    let v = crate::init_call("java.list_installed", serde_json::json!({})).await?;
    Ok(serde_json::from_value(v)?)
}

/// 按 major + prefer 找已安装运行时。
pub async fn find_managed(
    major: u32,
    prefer: Option<ImageType>,
) -> anyhow::Result<Option<InstalledRuntime>> {
    let v = crate::init_call(
        "java.find_managed",
        serde_json::json!({
            "major": major,
            "prefer": prefer.map(|i| i.as_str()),
        }),
    )
    .await?;
    if v.is_null() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_value(v)?))
}
