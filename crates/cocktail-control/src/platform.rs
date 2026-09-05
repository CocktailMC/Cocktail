//! Host platform detection for health / UI (distro + kernel + logos).

use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct PlatformInfo {
    pub os: String,
    pub arch: String,
    pub family: String,
    pub hostname: String,
    /// Short id for logos: ubuntu, debian, windows, macos, linux, …
    pub distro_id: String,
    /// Human-readable name, e.g. "Ubuntu 24.04.1 LTS"
    pub distro_name: String,
    /// Distro version string when known
    pub distro_version: String,
    /// Kernel version (uname -r / Windows NT build)
    pub kernel: String,
    /// True when running under Windows Subsystem for Linux
    pub wsl: bool,
}

pub fn detect() -> PlatformInfo {
    let hostname = sysinfo::System::host_name().unwrap_or_else(|| "unknown".into());
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let family = std::env::consts::FAMILY.to_string();
    let kernel = sysinfo::System::kernel_version()
        .or_else(|| read_proc_version_short())
        .unwrap_or_else(|| "unknown".into());

    let mut info = PlatformInfo {
        os: os.clone(),
        arch,
        family,
        hostname,
        distro_id: os.clone(),
        distro_name: sysinfo::System::long_os_version()
            .or_else(sysinfo::System::name)
            .unwrap_or_else(|| os.clone()),
        distro_version: sysinfo::System::os_version().unwrap_or_default(),
        kernel,
        wsl: false,
    };

    #[cfg(target_os = "linux")]
    {
        enrich_linux(&mut info);
    }

    #[cfg(target_os = "windows")]
    {
        enrich_windows(&mut info);
    }

    #[cfg(target_os = "macos")]
    {
        info.distro_id = "macos".into();
        if info.distro_name.is_empty() || info.distro_name.eq_ignore_ascii_case("macos") {
            info.distro_name = if info.distro_version.is_empty() {
                "macOS".into()
            } else {
                format!("macOS {}", info.distro_version)
            };
        }
    }

    info
}

#[cfg(target_os = "linux")]
fn enrich_linux(info: &mut PlatformInfo) {
    info.wsl = detect_wsl();

    if let Some(release) = parse_os_release() {
        if let Some(id) = release.get("ID") {
            info.distro_id = normalize_distro_id(id);
        }
        if let Some(pretty) = release.get("PRETTY_NAME") {
            info.distro_name = pretty.clone();
        } else if let Some(name) = release.get("NAME") {
            let ver = release
                .get("VERSION")
                .or_else(|| release.get("VERSION_ID"))
                .cloned()
                .unwrap_or_default();
            info.distro_name = if ver.is_empty() {
                name.clone()
            } else {
                format!("{name} {ver}")
            };
        }
        if let Some(ver) = release
            .get("VERSION_ID")
            .or_else(|| release.get("VERSION"))
        {
            info.distro_version = ver.clone();
        }
    } else {
        let id = sysinfo::System::distribution_id();
        if let Some(id) = non_empty(id) {
            info.distro_id = normalize_distro_id(&id);
        }
    }

    if info.wsl && !info.distro_name.to_lowercase().contains("wsl") {
        info.distro_name = format!("{} (WSL)", info.distro_name);
    }
}

#[cfg(target_os = "windows")]
fn enrich_windows(info: &mut PlatformInfo) {
    info.distro_id = "windows".into();
    let long = sysinfo::System::long_os_version().unwrap_or_default();
    let ver = sysinfo::System::os_version().unwrap_or_default();
    if !long.is_empty() {
        info.distro_name = long;
    } else if !ver.is_empty() {
        info.distro_name = format!("Windows {ver}");
    } else {
        info.distro_name = "Windows".into();
    }
    info.distro_version = ver;
}

fn non_empty(s: String) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

fn normalize_distro_id(id: &str) -> String {
    let id = id.trim().to_lowercase().replace('"', "");
    match id.as_str() {
        "arch" | "archarm" => "archlinux".into(),
        "opensuse-leap" | "opensuse-tumbleweed" | "sles" => "opensuse".into(),
        "ol" => "oraclelinux".into(),
        "rhel" => "redhat".into(),
        "pop" | "pop_os" => "popos".into(),
        "linuxmint" | "mint" => "linuxmint".into(),
        "elementary" => "elementary".into(),
        "kali" => "kalilinux".into(),
        "alpine" => "alpinelinux".into(),
        "rocky" => "rockylinux".into(),
        "alma" | "almalinux" => "almalinux".into(),
        "zorin" => "zorinos".into(),
        other => other.to_string(),
    }
}

fn parse_os_release() -> Option<std::collections::HashMap<String, String>> {
    for path in ["/etc/os-release", "/usr/lib/os-release"] {
        if !Path::new(path).exists() {
            continue;
        }
        let raw = fs::read_to_string(path).ok()?;
        let mut map = std::collections::HashMap::new();
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim().trim_matches('"').to_string();
            map.insert(k.to_string(), v);
        }
        if !map.is_empty() {
            return Some(map);
        }
    }
    None
}

fn detect_wsl() -> bool {
    if std::env::var_os("WSL_DISTRO_NAME").is_some() || std::env::var_os("WSL_INTEROP").is_some()
    {
        return true;
    }
    if let Ok(ver) = fs::read_to_string("/proc/version") {
        let lower = ver.to_lowercase();
        if lower.contains("microsoft") || lower.contains("wsl") {
            return true;
        }
    }
    Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists()
}

fn read_proc_version_short() -> Option<String> {
    let ver = fs::read_to_string("/proc/version").ok()?;
    // "Linux version 5.15.0-xxx (build@) ..."
    let mut parts = ver.split_whitespace();
    if parts.next()? != "Linux" {
        return None;
    }
    if parts.next()? != "version" {
        return None;
    }
    Some(parts.next()?.to_string())
}
