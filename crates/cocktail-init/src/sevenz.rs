//! 7z 二进制内嵌与解压：阶段 2 起 cocktail-init 接管用户空间 7z 操作。
//!
//! control 端通过 IPC `sevenz.ensure_bin` / `sevenz.extract` 调用本模块。
//! 7z 二进制以 `include_bytes!` 内嵌进 init 可执行文件，运行时落到
//! `data/runtime/7z/` 并按需校验大小后覆写，避免重复 IO。未内置的平台
//! 退化到 PATH 查找。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

pub use cocktail_shared::sevenz::is_supported_name;

const RUNTIME_DIR: &str = "data/runtime/7z";

/// 当前平台是否内置 7z 二进制。`pub` 供 RPC handler 上报给 control。
#[cfg(all(windows, target_arch = "aarch64"))]
pub const EMBEDDED: Option<(&[u8], &str)> = Some((
    include_bytes!("../vendor/7z/windows-arm64/7za.exe"),
    "7za.exe",
));

#[cfg(all(windows, not(target_arch = "aarch64")))]
pub const EMBEDDED: Option<(&[u8], &str)> = Some((
    include_bytes!("../vendor/7z/windows-x64/7za.exe"),
    "7za.exe",
));

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub const EMBEDDED: Option<(&[u8], &str)> =
    Some((include_bytes!("../vendor/7z/linux-x64/7zzs"), "7zzs"));

#[cfg(not(any(windows, all(target_os = "linux", target_arch = "x86_64"))))]
pub const EMBEDDED: Option<(&[u8], &str)> = None;

/// `sevenz.ensure_bin` RPC 返回结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsureBinResult {
    pub path: String,
    pub embedded: bool,
}

/// 落地 7z 二进制（若已内置）或退回 PATH 查找。
pub fn ensure_bin() -> anyhow::Result<PathBuf> {
    if let Some((bytes, name)) = EMBEDDED {
        let dir = PathBuf::from(RUNTIME_DIR);
        fs::create_dir_all(&dir).with_context(|| format!("create {RUNTIME_DIR}"))?;
        let dest = dir.join(name);
        let stale = match fs::metadata(&dest) {
            Ok(meta) => meta.len() != bytes.len() as u64,
            Err(_) => true,
        };
        if stale {
            fs::write(&dest, bytes)
                .with_context(|| format!("write {}", dest.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&dest)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&dest, perms)?;
            }
        }
        return Ok(dest);
    }
    which_system()
        .ok_or_else(|| anyhow::anyhow!("当前平台未内置 7z，且 PATH 中找不到 7za/7zz/7z"))
}

fn which_system() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["7za.exe", "7z.exe", "7zz.exe"]
    } else {
        &["7zzs", "7zz", "7za", "7z"]
    };
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 用内置/系统 7z 解压 archive 到 dest（自动创建 dest 目录）。
pub fn extract(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    let bin = ensure_bin()?;
    fs::create_dir_all(dest).with_context(|| format!("create {}", dest.display()))?;
    let mut cmd = std::process::Command::new(&bin);
    cmd.arg("x")
        .arg(archive)
        .arg(format!("-o{}", dest.display()))
        .args(["-y", "-aoa", "-spe"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console_std(&mut cmd);
    let out = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("无法启动内置 7z（{}）：{e}", bin.display()))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let detail = [stderr.trim(), stdout.trim()]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or("unknown error");
        anyhow::bail!("7z 解压失败：{detail}");
    }
    Ok(())
}

/// 隐藏 Windows 控制台窗口（避免 7z 子进程弹出黑窗）。
fn hide_console_std(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_7za_extracts_roundtrip() {
        let bin = match ensure_bin() {
            Ok(p) => p,
            Err(_) => {
                // 平台未内置 7z，跳过
                return;
            }
        };
        let root = std::env::temp_dir().join(format!("cocktail-init-7z-{}", uuid::Uuid::new_v4()));
        let src = root.join("src");
        let pack = root.join("pack.7z");
        let dest = root.join("out");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("server.jar"), b"fake-jar").unwrap();
        fs::write(src.join("eula.txt"), b"eula=true\n").unwrap();
        let mut cmd = std::process::Command::new(&bin);
        cmd.args(["a", "-t7z", "-y"])
            .arg(&pack)
            .arg(src.join("server.jar"))
            .arg(src.join("eula.txt"));
        hide_console_std(&mut cmd);
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "7za a failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        extract(&pack, &dest).unwrap();
        let found =
            dest.join("server.jar").is_file() || dest.join("src").join("server.jar").is_file();
        assert!(
            found,
            "extracted tree: {:?}",
            fs::read_dir(&dest).map(|d| d
                .flat_map(|e| e.ok().map(|x| x.file_name()))
                .collect::<Vec<_>>())
        );
        let _ = fs::remove_dir_all(&root);
    }
}
