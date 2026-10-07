//! Java 运行时管理：阶段 2 起 cocktail-init 接管 JRE 探测/下载/解压/缓存校验。
//!
//! control 端通过 IPC `java.*` 调用本模块：
//! - `java.inventory`：盘点系统 + 已安装的 JRE
//! - `java.install`：从 Adoptium 下载并落地一份 Temurin JRE/JDK
//! - `java.remove`：按 id 删除已安装的运行时
//! - `java.ensure_api` / `java.ensure_for_spec` / `java.ensure_instance_jre`：
//!   按主版本号选合适的 JRE，找不到就装
//! - `java.probe_system` / `java.list_installed` / `java.find_managed`
//!
//! TODO 阶段 2：下载进度推送（原 control 端 `http::Transfer`）暂砍掉，
//! 待 event push 通道就位后用 init→control 单向事件回传 progress。
//! TODO 阶段 3：init 不可达时考虑 fallback 到本地实现（兼容单进程部署）。

use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use flate2::read::GzDecoder;
use tar::Archive;
use tokio::sync::Mutex;
use zip::ZipArchive;

use cocktail_shared::java::{
    EnsureJavaRequest, EnsureJavaResponse, ImageType, InstallJavaRequest, InstalledRuntime,
    JavaInventory, RuntimeMeta, SystemJava,
};

const USER_AGENT: &str = "Cocktail-Manager/0.1 (Adoptium runtime manager)";
const ROOT: &str = "data/java";
const LTS_FALLBACK: &[u32] = &[8, 11, 17, 21, 25];

/// init 进程内单例锁：install 串行化，避免并发下载同一份 Temurin。
static INSTALL_LOCK: Mutex<()> = Mutex::const_new(());

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

pub fn docker_image_for(major: u32) -> String {
    format!("eclipse-temurin:{major}-jre")
}

pub fn runtime_id(major: u32, image: ImageType) -> String {
    format!("temurin-{major}-{}", image.as_str())
}

/// 构建 Adoptium 用的 reqwest client：UA 自定义、超时 600s（JRE 包大）、
/// 重定向 16 跳。代理检测/Windows TLS 走 init 端 http::builder() 默认。
fn client() -> reqwest::Client {
    crate::http::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(600))
        .redirect(reqwest::redirect::Policy::limited(16))
        .build()
        .expect("http client")
}

fn adoptium_os() -> &'static str {
    if Path::new("/etc/alpine-release").exists() {
        return "alpine-linux";
    }
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "mac",
        _ => "linux",
    }
}

fn adoptium_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        "x86" => "x86",
        other => other,
    }
}

fn java_exe() -> &'static str {
    if cfg!(windows) { "java.exe" } else { "java" }
}

fn root_dir() -> PathBuf {
    PathBuf::from(ROOT)
}

fn runtime_dir(id: &str) -> PathBuf {
    root_dir().join(id)
}

pub fn locate_java(root: &Path) -> Option<PathBuf> {
    let exe = java_exe();
    let direct = root.join("bin").join(exe);
    if direct.is_file() {
        return Some(direct);
    }
    locate_java_walk(root, 4)
}

fn locate_java_walk(dir: &Path, depth: u32) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let exe = java_exe();
    let candidate = dir.join("bin").join(exe);
    if candidate.is_file() {
        return Some(candidate);
    }
    let entries = fs::read_dir(dir).ok()?;
    for ent in entries.flatten() {
        let p = ent.path();
        if p.is_dir() {
            if let Some(found) = locate_java_walk(&p, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

fn java_home_of(bin: &Path) -> PathBuf {
    bin.parent()
        .and_then(|p| p.parent())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = fs::metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    let Ok(rd) = fs::read_dir(path) else {
        return 0;
    };
    rd.flatten().map(|e| dir_size(&e.path())).sum()
}

pub fn list_installed() -> Vec<InstalledRuntime> {
    let mut out = Vec::new();
    let root = root_dir();
    let Ok(entries) = fs::read_dir(&root) else {
        return out;
    };
    for ent in entries.flatten() {
        let dir = ent.path();
        if !dir.is_dir() {
            continue;
        }
        if let Some(rt) = read_installed(&dir) {
            out.push(rt);
        }
    }
    out.sort_by(|a, b| {
        b.major
            .cmp(&a.major)
            .then(a.image_type.as_str().cmp(b.image_type.as_str()))
    });
    out
}

fn read_installed(dir: &Path) -> Option<InstalledRuntime> {
    let meta_path = dir.join(".cocktail.json");
    let meta: RuntimeMeta = if meta_path.is_file() {
        serde_json::from_str(&fs::read_to_string(&meta_path).ok()?).ok()?
    } else {
        let bin = locate_java(dir)?;
        let name = dir.file_name()?.to_string_lossy();
        let (major, image) = parse_id(&name)?;
        RuntimeMeta {
            id: name.into(),
            vendor: "temurin".into(),
            major,
            image_type: image,
            release_name: String::new(),
            os: adoptium_os().into(),
            arch: adoptium_arch().into(),
            java_bin: bin.to_string_lossy().into(),
            java_home: java_home_of(&bin).to_string_lossy().into(),
        }
    };
    let bin = PathBuf::from(&meta.java_bin);
    let bin = if bin.is_file() {
        bin
    } else {
        locate_java(dir)?
    };
    Some(InstalledRuntime {
        id: meta.id,
        vendor: meta.vendor,
        major: meta.major,
        image_type: meta.image_type,
        release_name: meta.release_name,
        java_bin: bin.to_string_lossy().into(),
        java_home: java_home_of(&bin).to_string_lossy().into(),
        size_bytes: dir_size(dir),
    })
}

fn parse_id(id: &str) -> Option<(u32, ImageType)> {
    let rest = id.strip_prefix("temurin-")?;
    let (major, kind) = rest.rsplit_once('-')?;
    Some((major.parse().ok()?, ImageType::parse(kind).ok()?))
}

pub fn find_managed(major: u32, prefer: Option<ImageType>) -> Option<InstalledRuntime> {
    let list = list_installed();
    if let Some(want) = prefer {
        if let Some(hit) = list
            .iter()
            .find(|r| r.major == major && r.image_type == want)
        {
            return Some(hit.clone());
        }
    }
    list.into_iter().find(|r| r.major == major)
}

pub async fn probe_system() -> Option<SystemJava> {
    let bin = which_java()?;
    let mut cmd = tokio::process::Command::new(&bin);
    cmd.arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    let output = tokio::time::timeout(Duration::from_secs(8), cmd.output())
        .await
        .ok()?
        .ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let (major, version) = parse_java_version(&text)?;
    Some(SystemJava {
        java_bin: bin,
        major,
        version,
    })
}

fn which_java() -> Option<String> {
    #[cfg(windows)]
    let (prog, flag) = ("where.exe", "java");
    #[cfg(not(windows))]
    let (prog, flag) = ("which", "java");
    let mut cmd = std::process::Command::new(prog);
    cmd.arg(flag);
    hide_console_std(&mut cmd);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| {
            if l.is_empty() {
                return false;
            }
            if cfg!(windows) {
                let lower = l.to_ascii_lowercase();
                lower.ends_with("java.exe")
                    && !lower.ends_with("javaw.exe")
                    && !lower.contains(r"\windowsapps\")
                    && !lower.contains("/windowsapps/")
            } else {
                true
            }
        })
        .or_else(|| text.lines().map(str::trim).find(|l| !l.is_empty()))?
        .to_string();
    if line.is_empty() { None } else { Some(line) }
}

fn parse_java_version(text: &str) -> Option<(u32, String)> {
    let marker = "version \"";
    let start = text.find(marker)? + marker.len();
    let end = text[start..].find('"')? + start;
    let ver = text[start..end].to_string();
    let major = if ver.starts_with("1.") {
        ver.split('.').nth(1)?.parse().ok()?
    } else {
        ver.split(|c: char| !c.is_ascii_digit())
            .next()?
            .parse()
            .ok()?
    };
    Some((major, ver))
}

fn system_satisfies(have: u32, need: u32) -> bool {
    if need <= 8 { have == 8 } else { have >= need }
}

async fn available_lts() -> Vec<u32> {
    let url = "https://api.adoptium.net/v3/info/available_releases";
    let Ok(resp) = client().get(url).send().await else {
        return LTS_FALLBACK.to_vec();
    };
    let Ok(v) = resp.json::<serde_json::Value>().await else {
        return LTS_FALLBACK.to_vec();
    };
    let mut lts: Vec<u32> = v
        .get("available_lts_releases")
        .and_then(|x| x.as_array())
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_u64().map(|n| n as u32))
        .filter(|n| *n >= 8)
        .collect();
    if lts.is_empty() {
        return LTS_FALLBACK.to_vec();
    }
    lts.sort_unstable();
    lts.dedup();
    lts
}

pub async fn inventory() -> JavaInventory {
    let system = probe_system().await;
    JavaInventory {
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        adoptium_os: adoptium_os().into(),
        adoptium_arch: adoptium_arch().into(),
        system,
        installed: list_installed(),
        available_lts: available_lts().await,
        recommended_major: 21,
    }
}

pub const INSTANCE_JRE_REL: &str = "runtime/jre";

pub fn instance_jre_home(workdir: &Path) -> PathBuf {
    INSTANCE_JRE_REL
        .split('/')
        .fold(workdir.to_path_buf(), |p, seg| p.join(seg))
}

pub async fn ensure_template(major: u32, image: ImageType) -> anyhow::Result<PathBuf> {
    if let Some(rt) = find_managed(major, Some(image)).or_else(|| find_managed(major, None)) {
        return Ok(PathBuf::from(rt.java_bin));
    }
    let rt = install(major, image).await?;
    Ok(PathBuf::from(rt.java_bin))
}

pub async fn ensure(major: u32, image: ImageType) -> anyhow::Result<PathBuf> {
    if let Some(rt) = find_managed(major, Some(image)).or_else(|| find_managed(major, None)) {
        return Ok(PathBuf::from(rt.java_bin));
    }
    if let Some(sys) = probe_system().await {
        if system_satisfies(sys.major, major) {
            return Ok(PathBuf::from(sys.java_bin));
        }
    }
    let rt = install(major, image).await?;
    Ok(PathBuf::from(rt.java_bin))
}

pub async fn ensure_for_spec(
    workdir: &str,
    java_major: Option<u32>,
    mc_version: Option<&str>,
) -> anyhow::Result<PathBuf> {
    ensure_instance_jre(workdir, java_major, mc_version).await
}

pub async fn ensure_instance_jre(
    workdir: &str,
    java_major: Option<u32>,
    mc_version: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let major = java_major.unwrap_or_else(|| recommended_java_major(mc_version));

    let work = std::path::absolute(workdir)
        .with_context(|| format!("resolve instance directory {workdir}"))?;
    let dest = instance_jre_home(&work);
    if let Some(bin) = locate_java(&dest) {
        let cached_major = major_from_home(&dest);
        // 双重校验：meta 中声明的 major 必须与 java.exe 实际版本一致。
        // 历史上 copy_dir 会把 template 自带的 .cocktail.json 一并复制过来，
        // 若 template 的 meta 与实际 java.exe 版本不符（例如之前版本写错了 meta，
        // 或用户手动替换过 java.exe），单看 meta 会让缓存错误命中、永远用旧 JRE。
        // 这里探测一次 java -version，与 meta 不一致就视为缓存失效。
        let probed_major = probe_java_major(&bin).await;
        let effective = match (cached_major, probed_major) {
            (Some(m), Some(p)) if m == p => Some(m),
            (Some(m), None) => {
                tracing::warn!(
                    workdir,
                    meta_major = m,
                    "instance JRE meta major 无法用 java -version 校验，仅按 meta 判断"
                );
                Some(m)
            }
            (Some(m), Some(p)) => {
                tracing::warn!(
                    workdir,
                    meta_major = m,
                    actual_major = p,
                    "instance JRE meta 与 java -version 不一致，以实际为准"
                );
                Some(p)
            }
            (None, Some(p)) => {
                tracing::warn!(
                    workdir,
                    actual_major = p,
                    "instance JRE meta 缺失，用 java -version 探测"
                );
                Some(p)
            }
            (None, None) => None,
        };
        if effective == Some(major) {
            tracing::debug!(
                workdir,
                major,
                cached = ?cached_major,
                actual = ?probed_major,
                "instance JRE cache hit"
            );
            return Ok(bin);
        }
        tracing::info!(
            workdir,
            have = ?effective,
            need = major,
            cached_meta = ?cached_major,
            actual_probe = ?probed_major,
            "replacing instance JRE (major mismatch)"
        );
        let _ = fs::remove_dir_all(&dest);
    }

    let template_bin = ensure_template(major, ImageType::Jre).await?;
    let template_home = java_home_of(&template_bin);
    if !template_home.is_dir() {
        anyhow::bail!("Temurin 模板目录不存在：{}", template_home.display());
    }

    // TODO 阶段 2：进度推送通过 event push 通道回传 control。
    // 原 control 端 `Transfer::emit("copy", 0, None)` 暂以 tracing 替代。
    tracing::info!(workdir, major, "copying instance JRE from template");
    let src = template_home.clone();
    let dst = dest.clone();
    tokio::task::spawn_blocking(move || {
        if dst.exists() {
            fs::remove_dir_all(&dst)?;
        }
        copy_dir(&src, &dst)
    })
    .await
    .map_err(|e| anyhow::anyhow!("复制实例 JRE 任务失败：{e}"))?
    .with_context(|| format!("复制 JRE {} → {}", template_home.display(), dest.display()))?;

    let bin = locate_java(&dest).ok_or_else(|| {
        anyhow::anyhow!("实例 JRE 复制后找不到 {}（{}）", java_exe(), dest.display())
    })?;
    chmod_bin(bin.parent().unwrap_or(&dest))?;
    // 复制后立即用 java -version 校验实际版本，避免 meta 写 25 但 java.exe 实际是 21
    // 的失配（template_home 来源错误、copy_dir 中断、外部替换等都可能导致）。
    // 不匹配直接 bail 让上层走重试/清理路径，绝不能让错的 JRE 启动。
    if let Some(actual) = probe_java_major(&bin).await {
        if actual != major {
            let _ = fs::remove_dir_all(&dest);
            anyhow::bail!(
                "实例 JRE 复制后 java -version 实际为 Java {actual}，\
                 与期望 Java {major} 不符；已清理 dest，请重试启动"
            );
        }
    } else {
        tracing::warn!(
            workdir,
            bin = %bin.display(),
            major,
            "复制后无法用 java -version 探测实际版本，仅按 meta 写入"
        );
    }
    write_instance_meta(&dest, major, &bin)?;
    // TODO 阶段 2：原 `job.finish(dir_size(&dest), ...)` 改为 event push。
    tracing::info!(
        workdir,
        bin = %bin.display(),
        major,
        size = dir_size(&dest),
        "instance JRE ready"
    );
    Ok(bin)
}

fn major_from_home(home: &Path) -> Option<u32> {
    if let Ok(text) = fs::read_to_string(home.join(".cocktail.json")) {
        if let Ok(meta) = serde_json::from_str::<RuntimeMeta>(&text) {
            return Some(meta.major);
        }
    }
    let release = fs::read_to_string(home.join("release")).ok()?;
    parse_release_major(&release)
}

/// 通过 `java -version` 探测实际主版本号。
/// 仅在 ensure_instance_jre 缓存判断时调用，用于校验 meta 与实际 java.exe 一致。
/// 失败（找不到 java、超时、无法解析）返回 None，调用方回退到 meta 判断。
async fn probe_java_major(bin: &Path) -> Option<u32> {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    let output = tokio::time::timeout(Duration::from_secs(8), cmd.output())
        .await
        .ok()?
        .ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    parse_java_version(&text).map(|(major, _)| major)
}

fn parse_release_major(text: &str) -> Option<u32> {
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("JAVA_VERSION=") else {
            continue;
        };
        let ver = rest.trim().trim_matches('"');
        return if ver.starts_with("1.") {
            ver.split('.').nth(1)?.parse().ok()
        } else {
            ver.split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse()
                .ok()
        };
    }
    None
}

fn write_instance_meta(home: &Path, major: u32, bin: &Path) -> anyhow::Result<()> {
    let meta = RuntimeMeta {
        id: format!("instance-{}", runtime_id(major, ImageType::Jre)),
        vendor: "temurin".into(),
        major,
        image_type: ImageType::Jre,
        release_name: String::new(),
        os: adoptium_os().into(),
        arch: adoptium_arch().into(),
        java_bin: bin.to_string_lossy().into(),
        java_home: java_home_of(bin).to_string_lossy().into(),
    };
    fs::write(
        home.join(".cocktail.json"),
        serde_json::to_vec_pretty(&meta)?,
    )?;
    Ok(())
}

pub async fn install(major: u32, image: ImageType) -> anyhow::Result<InstalledRuntime> {
    if major < 8 {
        anyhow::bail!("不支持的 Java 主版本：{major}");
    }
    let _guard = INSTALL_LOCK.lock().await;
    if let Some(rt) = find_managed(major, Some(image)) {
        return Ok(rt);
    }

    let os = adoptium_os();
    let arch = adoptium_arch();
    let (url, filename, release_name) = resolve_asset(major, image, os, arch).await?;
    tracing::info!(%url, major, image = image.as_str(), "downloading Adoptium Temurin");

    fs::create_dir_all(root_dir())?;
    let id = runtime_id(major, image);
    let dest = runtime_dir(&id);
    let staging = root_dir().join(format!("{id}.partial"));
    let archive = root_dir().join(format!("{id}-{filename}"));
    let _ = fs::remove_dir_all(&staging);
    let _ = fs::remove_file(&archive);
    fs::create_dir_all(&staging)?;

    // TODO 阶段 2：进度推送通过 event push 通道回传 control。
    // 原 control 端 `Transfer::new/emit/finish` 暂以 tracing 替代。
    let label = format!("Temurin {major} {}", image.as_str().to_ascii_uppercase());
    tracing::info!(%label, %url, "download starting");
    download_to(&url, &archive)
        .await
        .with_context(|| format!("下载 Temurin {major} 失败"))?;
    tracing::info!(%label, archive = %archive.display(), "extracting");
    extract_archive(&archive, &staging)
        .with_context(|| format!("解压 {} 失败", archive.display()))?;
    let _ = fs::remove_file(&archive);
    flatten_single_root(&staging)?;
    let bin = locate_java(&staging)
        .ok_or_else(|| anyhow::anyhow!("解压后找不到 {}（请检查 Adoptium 包结构）", java_exe()))?;
    chmod_bin(bin.parent().unwrap_or(&staging))?;
    let home = java_home_of(&bin);
    let meta = RuntimeMeta {
        id: id.clone(),
        vendor: "temurin".into(),
        major,
        image_type: image,
        release_name,
        os: os.into(),
        arch: arch.into(),
        java_bin: bin.to_string_lossy().into(),
        java_home: home.to_string_lossy().into(),
    };
    fs::write(
        staging.join(".cocktail.json"),
        serde_json::to_vec_pretty(&meta)?,
    )?;

    if dest.exists() {
        fs::remove_dir_all(&dest)?;
    }
    if fs::rename(&staging, &dest).is_err() {
        copy_dir(&staging, &dest)?;
        fs::remove_dir_all(&staging)?;
    }

    let installed =
        read_installed(&dest).ok_or_else(|| anyhow::anyhow!("安装完成但无法读取运行时"))?;
    // TODO 阶段 2：原 `job.finish(installed.size_bytes, ...)` 改为 event push。
    tracing::info!(
        id = %installed.id,
        bin = %installed.java_bin,
        size = installed.size_bytes,
        "Temurin installed"
    );
    Ok(installed)
}

pub fn remove(id: &str) -> anyhow::Result<()> {
    let id = id.trim();
    if id.is_empty() || id.contains(['/', '\\', '.']) {
        anyhow::bail!("invalid runtime id");
    }
    let dir = runtime_dir(id);
    if !dir.exists() {
        anyhow::bail!("运行时不存在：{id}");
    }
    fs::remove_dir_all(&dir)?;
    Ok(())
}

async fn resolve_asset(
    major: u32,
    image: ImageType,
    os: &str,
    arch: &str,
) -> anyhow::Result<(String, String, String)> {
    let url = format!(
        "https://api.adoptium.net/v3/assets/latest/{major}/hotspot?os={os}&architecture={arch}&image_type={}&vendor=eclipse&project=jdk",
        image.as_str()
    );
    let v = client()
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json::<serde_json::Value>()
        .await?;
    let row = v.as_array().and_then(|a| a.first()).ok_or_else(|| {
        anyhow::anyhow!(
            "Adoptium 没有 {os}/{arch} 的 Temurin {major} {}",
            image.as_str()
        )
    })?;
    let pkg = row
        .pointer("/binary/package")
        .ok_or_else(|| anyhow::anyhow!("Adoptium 响应缺少 package"))?;
    let link = pkg
        .get("link")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow::anyhow!("Adoptium 缺少下载链接"))?
        .to_string();
    let name = pkg
        .get("name")
        .and_then(|x| x.as_str())
        .unwrap_or("temurin.tar.gz")
        .to_string();
    let release = row
        .get("release_name")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    Ok((link, name, release))
}

/// 下载 url 到 dest。校验最小体积（防 Adoptium 404 / 重定向到错误页）。
/// 实际下载走 init 端 `http::download_to_path(url, dest) -> Result<u64>`，
/// 不再接收外部 client 与 Transfer job（init 端无 Transfer）。
async fn download_to(url: &str, dest: &Path) -> anyhow::Result<()> {
    let written = crate::http::download_to_path(url, dest).await?;
    if written < 1024 * 1024 {
        anyhow::bail!("下载文件过小（{written} bytes），可能不是完整的 JDK/JRE");
    }
    Ok(())
}

fn extract_archive(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    let name = archive
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.ends_with(".zip") {
        extract_zip(archive, dest)
    } else {
        extract_tar_gz(archive, dest)
    }
}

fn extract_zip(zip_path: &Path, dest: &Path) -> anyhow::Result<()> {
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
            io::copy(&mut file, &mut outfile)?;
        }
    }
    Ok(())
}

fn extract_tar_gz(path: &Path, dest: &Path) -> anyhow::Result<()> {
    let file = File::open(path)?;
    let gz = GzDecoder::new(file);
    let mut archive = Archive::new(gz);
    fs::create_dir_all(dest)?;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let rel = entry.path()?.into_owned();
        let Some(out) = safe_join(dest, &rel) else {
            continue;
        };
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

fn flatten_single_root(dest: &Path) -> anyhow::Result<()> {
    if locate_java(dest).is_some() && dest.join("bin").is_dir() {
        return Ok(());
    }
    let mut dirs = Vec::new();
    for ent in fs::read_dir(dest)? {
        let p = ent?.path();
        if p.file_name().and_then(|n| n.to_str()) == Some(".cocktail.json") {
            continue;
        }
        if p.is_dir() {
            dirs.push(p);
        }
    }
    if dirs.len() != 1 {
        return Ok(());
    }
    let inner = dirs.remove(0);
    if locate_java(&inner).is_none() {
        return Ok(());
    }
    let tmp = dest.join(".flatten-tmp");
    let _ = fs::remove_dir_all(&tmp);
    fs::rename(&inner, &tmp)?;
    for ent in fs::read_dir(&tmp)? {
        let ent = ent?;
        let to = dest.join(ent.file_name());
        fs::rename(ent.path(), to)?;
    }
    let _ = fs::remove_dir_all(&tmp);
    Ok(())
}

fn chmod_bin(bin_dir: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !bin_dir.is_dir() {
            return Ok(());
        }
        for ent in fs::read_dir(bin_dir)? {
            let p = ent?.path();
            if p.is_file() {
                let mut perms = fs::metadata(&p)?.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&p, perms);
            }
        }
    }
    let _ = bin_dir;
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

pub async fn ensure_api(req: EnsureJavaRequest) -> anyhow::Result<EnsureJavaResponse> {
    let major = req.major.unwrap_or(21);
    let image = ImageType::parse(req.image_type.as_deref().unwrap_or("jre"))?;
    if !req.managed {
        if let Some(rt) = find_managed(major, Some(image)).or_else(|| find_managed(major, None)) {
            return Ok(EnsureJavaResponse {
                java_bin: rt.java_bin,
                java_home: Some(rt.java_home),
                major: rt.major,
                source: "managed".into(),
            });
        }
        if let Some(sys) = probe_system().await {
            if system_satisfies(sys.major, major) {
                return Ok(EnsureJavaResponse {
                    java_bin: sys.java_bin,
                    java_home: None,
                    major: sys.major,
                    source: "system".into(),
                });
            }
        }
    }
    let rt = if let Some(rt) = find_managed(major, Some(image)) {
        rt
    } else {
        install(major, image).await?
    };
    Ok(EnsureJavaResponse {
        java_bin: rt.java_bin,
        java_home: Some(rt.java_home),
        major: rt.major,
        source: "adoptium".into(),
    })
}

/// 隐藏 Windows 控制台窗口（避免 java / where / which 子进程弹出黑窗）。
/// tokio::process::Command 版本，参照 sevenz.rs 的 hide_console_std。
fn hide_console(cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        // tokio::process::Command 在 windows 上同样用 creation_flags，
        // 通过 OsStringExt / CommandExt 暴露；与 std 版本一致用 CREATE_NO_WINDOW。
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd;
}

/// 隐藏 Windows 控制台窗口（std::process::Command 版本）。直接抄 sevenz.rs。
fn hide_console_std(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd;
}

/// 判断 command 是否为 java 启动命令：内联自 control/src/util.rs。
/// 5 行字符串逻辑：取最后一段路径，小写后 == "java"/"java.exe" 或以 "java" 开头。
fn is_java_command(command: &str) -> bool {
    let base = command
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(command)
        .trim()
        .to_ascii_lowercase();
    base == "java" || base == "java.exe" || base.starts_with("java")
}

pub fn rewrite_java_command(command: Option<String>, java_bin: &Path) -> Option<String> {
    let path = java_bin.to_string_lossy().into_owned();
    match command {
        None => Some(path),
        Some(cmd) if is_java_command(&cmd) => Some(path),
        Some(cmd) => Some(cmd),
    }
}

pub fn apply_java_home(cmd: &mut tokio::process::Command, bin: &str) {
    let path = Path::new(bin);
    if !is_java_command(bin) {
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

pub fn apply_isolated_env(cmd: &mut tokio::process::Command, java_bin: &str, workdir: &str) {
    apply_java_home(cmd, java_bin);
    let work = abs_workdir(workdir);
    let cocktail = work.join(".cocktail");
    let tmp = cocktail.join("tmp");
    let appdata = cocktail.join("appdata");
    let _ = fs::create_dir_all(&tmp);
    let _ = fs::create_dir_all(&appdata);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cached_instance_jre_returns_absolute_path() {
        let work = std::env::temp_dir().join(format!("java-path-test-{}", uuid::Uuid::new_v4()));
        let home = instance_jre_home(&work);
        fs::create_dir_all(home.join("bin")).unwrap();
        fs::write(home.join("bin").join(java_exe()), b"").unwrap();
        fs::write(home.join("release"), "JAVA_VERSION=\"21.0.5\"\n").unwrap();
        let result = ensure_instance_jre(work.to_str().unwrap(), Some(21), None).await;
        fs::remove_dir_all(&work).unwrap();
        let bin = result.unwrap();
        assert!(bin.is_absolute());
        assert_eq!(
            bin,
            std::env::current_dir()
                .unwrap()
                .join(home)
                .join("bin")
                .join(java_exe())
        );
    }

    #[test]
    fn rewrite_always_pins_java() {
        let bin = Path::new(r"D:\cup\runtime\jre\bin\java.exe");
        assert_eq!(
            rewrite_java_command(Some("java".into()), bin).as_deref(),
            Some(bin.to_str().unwrap())
        );
        assert_eq!(
            rewrite_java_command(Some(r"C:\Program Files\Java\bin\java.exe".into()), bin)
                .as_deref(),
            Some(bin.to_str().unwrap())
        );
        assert_eq!(
            rewrite_java_command(Some("paper.bat".into()), bin).as_deref(),
            Some("paper.bat")
        );
    }

    #[test]
    fn parse_temurin_release_major() {
        assert_eq!(
            parse_release_major("JAVA_VERSION=\"21.0.5\"\nOS_NAME=\"Windows\"\n"),
            Some(21)
        );
        assert_eq!(parse_release_major("JAVA_VERSION=\"1.8.0_422\"\n"), Some(8));
        assert_eq!(
            parse_release_major("IMPLEMENTOR=\"Eclipse Adoptium\"\n"),
            None
        );
    }

    #[test]
    fn instance_jre_lives_under_workdir() {
        let home = instance_jre_home(Path::new("data/instances/abc"));
        assert!(home.ends_with(Path::new("runtime").join("jre")));
    }
}
