//! Windows stdin broker so console commands survive control-plane restart.
//!
//! Linux uses a filesystem FIFO. Windows has no equivalent, so a helper process
//! owns Java's stdin and accepts writers on a named pipe. After the control
//! plane restarts it reconnects to the same pipe.

use std::path::{Path, PathBuf};

pub fn pipe_name_path(workdir: &str) -> PathBuf {
    Path::new(workdir).join(".cocktail").join("stdin.pipe")
}

#[cfg(windows)]
pub fn spawn_bridge(workdir: &str) -> anyhow::Result<std::process::ChildStdout> {
    use std::process::{Command, Stdio};

    let pipe = format!(
        r"\\.\pipe\cocktail-stdin-{}",
        uuid::Uuid::new_v4().simple()
    );
    let dir = Path::new(workdir).join(".cocktail");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(pipe_name_path(workdir), &pipe)?;

    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.args(["--stdin-bridge", &pipe])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::wincompat::hide_console_std(&mut cmd);
    let mut child = cmd.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("stdin bridge has no stdout"))?;
    std::mem::forget(child);
    // Give ConnectNamedPipe a moment before the first client attaches.
    std::thread::sleep(std::time::Duration::from_millis(80));
    Ok(stdout)
}

#[cfg(not(windows))]
pub fn spawn_bridge(_workdir: &str) -> anyhow::Result<std::process::ChildStdout> {
    anyhow::bail!("stdin bridge is Windows-only")
}

#[cfg(windows)]
pub fn open_pipe_writer(workdir: &str) -> anyhow::Result<std::fs::File> {
    let raw = std::fs::read_to_string(pipe_name_path(workdir))?;
    let name = raw.trim();
    if name.is_empty() {
        anyhow::bail!("empty stdin pipe name");
    }
    let mut last = None;
    for _ in 0..25 {
        match std::fs::OpenOptions::new().write(true).open(name) {
            Ok(f) => return Ok(f),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(40));
            }
        }
    }
    Err(anyhow::anyhow!(
        "open stdin pipe: {}",
        last.map(|e| e.to_string()).unwrap_or_else(|| "timeout".into())
    ))
}

#[cfg(not(windows))]
pub fn open_pipe_writer(_workdir: &str) -> anyhow::Result<std::fs::File> {
    anyhow::bail!("stdin bridge is Windows-only")
}

/// Blocking helper: named-pipe server → stdout (Java stdin).
pub fn run_stdin_bridge(pipe_name: &str) -> anyhow::Result<()> {
    #[cfg(not(windows))]
    {
        let _ = pipe_name;
        anyhow::bail!("stdin bridge is Windows-only");
    }
    #[cfg(windows)]
    {
        run_windows_bridge(pipe_name)
    }
}

#[cfg(windows)]
fn run_windows_bridge(pipe_name: &str) -> anyhow::Result<()> {
    use std::io::{Read, Write};
    use std::os::windows::io::{FromRawHandle, RawHandle};

    const PIPE_ACCESS_INBOUND: u32 = 0x0000_0001;
    const PIPE_TYPE_BYTE: u32 = 0x0000_0000;
    const PIPE_WAIT: u32 = 0x0000_0000;
    const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x0000_0008;
    const FILE_FLAG_FIRST_PIPE_INSTANCE: u32 = 0x0008_0000;
    const INVALID_HANDLE_VALUE: isize = -1;
    const ERROR_PIPE_CONNECTED: u32 = 535;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateNamedPipeW(
            name: *const u16,
            open_mode: u32,
            pipe_mode: u32,
            max_instances: u32,
            out_buf: u32,
            in_buf: u32,
            timeout: u32,
            sec: *mut core::ffi::c_void,
        ) -> RawHandle;
        fn ConnectNamedPipe(pipe: RawHandle, overlapped: *mut core::ffi::c_void) -> i32;
        fn DisconnectNamedPipe(pipe: RawHandle) -> i32;
        fn GetLastError() -> u32;
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    let name = wide(pipe_name);
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            4096,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle as isize == INVALID_HANDLE_VALUE {
        anyhow::bail!("CreateNamedPipe failed ({})", unsafe { GetLastError() });
    }

    let mut stdout = std::io::stdout();
    loop {
        let ok = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            if err != ERROR_PIPE_CONNECTED {
                let _ = unsafe { DisconnectNamedPipe(handle) };
                continue;
            }
        }
        let mut file = unsafe { std::fs::File::from_raw_handle(handle) };
        let mut buf = [0u8; 4096];
        loop {
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if stdout.write_all(&buf[..n]).is_err() || stdout.flush().is_err() {
                        std::mem::forget(file);
                        return Ok(());
                    }
                }
                Err(_) => break,
            }
        }
        // Do not close the pipe handle; reuse it for the next control-plane client.
        std::mem::forget(file);
        let _ = unsafe { DisconnectNamedPipe(handle) };
    }
}
