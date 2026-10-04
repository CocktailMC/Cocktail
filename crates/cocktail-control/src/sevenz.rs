use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

const RUNTIME_DIR: &str = "data/runtime/7z";

#[cfg(all(windows, target_arch = "aarch64"))]
const EMBEDDED: Option<(&[u8], &str)> = Some((
    include_bytes!("../vendor/7z/windows-arm64/7za.exe"),
    "7za.exe",
));

#[cfg(all(windows, not(target_arch = "aarch64")))]
const EMBEDDED: Option<(&[u8], &str)> = Some((
    include_bytes!("../vendor/7z/windows-x64/7za.exe"),
    "7za.exe",
));

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const EMBEDDED: Option<(&[u8], &str)> =
    Some((include_bytes!("../vendor/7z/linux-x64/7zzs"), "7zzs"));

#[cfg(not(any(windows, all(target_os = "linux", target_arch = "x86_64"))))]
const EMBEDDED: Option<(&[u8], &str)> = None;

pub fn ensure_bin() -> anyhow::Result<PathBuf> {
    if let Some((bytes, name)) = EMBEDDED {
        let dir = PathBuf::from(RUNTIME_DIR);
        fs::create_dir_all(&dir)?;
        let dest = dir.join(name);
        let stale = match fs::metadata(&dest) {
            Ok(meta) => meta.len() != bytes.len() as u64,
            Err(_) => true,
        };
        if stale {
            fs::write(&dest, bytes)?;
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
    which_system().ok_or_else(|| anyhow::anyhow!("当前平台未内置 7z，且 PATH 中找不到 7za/7zz/7z"))
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

pub fn extract(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    let bin = ensure_bin()?;
    fs::create_dir_all(dest)?;
    let mut cmd = std::process::Command::new(&bin);
    cmd.arg("x")
        .arg(archive)
        .arg(format!("-o{}", dest.display()))
        .args(["-y", "-aoa", "-spe"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::wincompat::hide_console_std(&mut cmd);
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

pub fn is_supported_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".7z")
        || n.ends_with(".zip")
        || n.ends_with(".tar")
        || n.ends_with(".tar.gz")
        || n.ends_with(".tgz")
        || n.ends_with(".tar.xz")
        || n.ends_with(".tar.bz2")
        || n.ends_with(".gz")
        || n.ends_with(".xz")
        || n.ends_with(".bz2")
        || n.ends_with(".jar")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_7za_extracts_roundtrip() {
        let bin = ensure_bin().expect("bundled 7z");
        let root = std::env::temp_dir().join(format!("cocktail-7z-{}", uuid::Uuid::new_v4()));
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
        crate::wincompat::hide_console_std(&mut cmd);
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

    #[test]
    fn accepts_common_pack_names() {
        assert!(is_supported_name("server.7z"));
        assert!(is_supported_name("pack.TAR.GZ"));
        assert!(!is_supported_name("notes.txt"));
    }
}
