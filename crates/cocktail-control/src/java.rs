//! Adoptium (Eclipse Temurin) JDK/JRE manager: list, download, extract, auto-complete.

use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tar::Archive;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use zip::ZipArchive;

const USER_AGENT: &str = "Cocktail-Manager/0.1 (Adoptium runtime manager)";
const ROOT: &str = "data/java";
const LTS_FALLBACK: &[u32] = &[8, 11, 17, 21, 25];

static INSTALL_LOCK: Mutex<()> = Mutex::const_new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageType {
    Jre,
    Jdk,
}

impl ImageType {
    pub fn as_str(self) -> &'static str {
        match self {
            ImageType::Jre => "jre",
            ImageType::Jdk => "jdk",
        }
    }

    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "jre" | "" => Ok(ImageType::Jre),
            "jdk" => Ok(ImageType::Jdk),
            other => anyhow::bail!("image_type 必须是 jre 或 jdk，收到 {other}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeMeta {
    pub id: String,
    pub vendor: String,
    pub major: u32,
    pub image_type: ImageType,
    pub release_name: String,
    pub os: String,
    pub arch: String,
    pub java_bin: String,
    #[serde(default)]
    pub java_home: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstalledRuntime {
    pub id: String,
    pub vendor: String,
    pub major: u32,
    pub image_type: ImageType,
    pub release_name: String,
    pub java_bin: String,
    pub java_home: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemJava {
    pub java_bin: String,
    pub major: u32,
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct JavaInventory {
    pub os: String,
    pub arch: String,
    pub adoptium_os: String,
    pub adoptium_arch: String,
    pub system: Option<SystemJava>,
    pub installed: Vec<InstalledRuntime>,
    pub available_lts: Vec<u32>,
    pub recommended_major: u32,
}

#[derive(Debug, Deserialize)]
pub struct InstallJavaRequest {
    pub major: u32,
    #[serde(default)]
    pub image_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct EnsureJavaRequest {
    #[serde(default)]
    pub major: Option<u32>,
    #[serde(default)]
    pub image_type: Option<String>,
    /// If true, ignore system Java and keep a managed Temurin copy.
    #[serde(default)]
    pub managed: bool,
}

#[derive(Debug, Serialize)]
pub struct EnsureJavaResponse {
    pub java_bin: String,
    pub java_home: Option<String>,
    pub major: u32,
    pub source: String,
}

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

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(20))
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
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
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
    out.sort_by(|a, b| b.major.cmp(&a.major).then(a.image_type.as_str().cmp(b.image_type.as_str())));
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
    let mut cmd = tokio::process::Command::new("java");
    cmd.arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::wincompat::hide_console(&mut cmd);
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
    let bin = which_java().unwrap_or_else(|| "java".into());
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
    crate::wincompat::hide_console_std(&mut cmd);
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
                lower.ends_with("java.exe") && !lower.ends_with("javaw.exe")
            } else {
                true
            }
        })
        .or_else(|| text.lines().map(str::trim).find(|l| !l.is_empty()))?
        .to_string();
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
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
    if need <= 8 {
        have == 8
    } else {
        have >= need
    }
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

/// Resolve a Java binary: managed Temurin, then system, then download JRE.
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

pub async fn ensure_for_spec(java_major: Option<u32>, mc_version: Option<&str>) -> anyhow::Result<PathBuf> {
    let major = java_major.unwrap_or_else(|| recommended_java_major(mc_version));
    ensure(major, ImageType::Jre).await
}

pub fn rewrite_java_command(command: Option<String>, java_bin: &Path) -> Option<String> {
    let Some(cmd) = command else {
        return Some(java_bin.to_string_lossy().into_owned());
    };
    if crate::util::is_java_command(&cmd) {
        let p = Path::new(&cmd);
        if p.is_file() {
            return Some(cmd);
        }
        return Some(java_bin.to_string_lossy().into_owned());
    }
    Some(cmd)
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

    download_to(&url, &archive).await.with_context(|| format!("下载 Temurin {major} 失败"))?;
    extract_archive(&archive, &staging)
        .with_context(|| format!("解压 {} 失败", archive.display()))?;
    let _ = fs::remove_file(&archive);
    flatten_single_root(&staging)?;
    let bin = locate_java(&staging).ok_or_else(|| {
        anyhow::anyhow!("解压后找不到 {}（请检查 Adoptium 包结构）", java_exe())
    })?;
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

    let installed = read_installed(&dest).ok_or_else(|| anyhow::anyhow!("安装完成但无法读取运行时"))?;
    tracing::info!(id = %installed.id, bin = %installed.java_bin, "Temurin installed");
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
    let row = v
        .as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| anyhow::anyhow!("Adoptium 没有 {os}/{arch} 的 Temurin {major} {}", image.as_str()))?;
    let pkg = row.pointer("/binary/package").ok_or_else(|| anyhow::anyhow!("Adoptium 响应缺少 package"))?;
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

async fn download_to(url: &str, dest: &Path) -> anyhow::Result<()> {
    let resp = client().get(url).send().await?.error_for_status()?;
    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = resp.bytes_stream();
    let mut written: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        written += chunk.len() as u64;
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
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

/// Set JAVA_HOME on a command when `bin` is a managed/system java path.
pub fn apply_java_home(cmd: &mut tokio::process::Command, bin: &str) {
    let path = Path::new(bin);
    if !crate::util::is_java_command(bin) {
        return;
    }
    if let Some(home) = path.parent().and_then(|p| p.parent()) {
        if home.join("release").is_file() || home.join("lib").is_dir() {
            cmd.env("JAVA_HOME", home);
        }
    }
}
