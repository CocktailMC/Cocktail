//! Windows process / console / path helpers (no-ops on other platforms).

use std::path::Path;

#[cfg(windows)]
mod ffi {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn SetConsoleOutputCP(w_code_page_id: u32) -> i32;
        pub fn SetConsoleCP(w_code_page_id: u32) -> i32;
    }
}

/// Hide the extra console window that Windows allocates for child processes.
pub fn hide_console(cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd;
}

pub fn hide_console_std(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd;
}

pub fn enable_utf8_console() {
    #[cfg(windows)]
    unsafe {
        let _ = ffi::SetConsoleOutputCP(65001);
        let _ = ffi::SetConsoleCP(65001);
    }
}

pub fn is_admin() -> bool {
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("net");
        cmd.args(["session"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        hide_console_std(&mut cmd);
        return cmd.status().map(|s| s.success()).unwrap_or(false);
    }
    #[cfg(unix)]
    {
        return unsafe { libc::geteuid() == 0 };
    }
    #[cfg(not(any(windows, unix)))]
    {
        false
    }
}

pub fn command_exists(name: &str) -> bool {
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("where.exe");
        cmd.arg(name)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        hide_console_std(&mut cmd);
        return cmd.status().map(|s| s.success()).unwrap_or(false);
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("sh")
            .args(["-c", &format!("command -v {name} >/dev/null 2>&1")])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

pub fn paths_equal(a: &Path, b: &Path) -> bool {
    fn norm(p: &Path) -> String {
        let s = p.to_string_lossy();
        let s = s
            .strip_prefix(r"\\?\")
            .or_else(|| s.strip_prefix("//?/"))
            .unwrap_or(s.as_ref());
        s.replace('/', "\\")
            .trim_end_matches(['\\', '/'])
            .to_ascii_lowercase()
    }
    if a == b {
        return true;
    }
    norm(a) == norm(b)
}
