//! Native paths and process identity for the Windows installer handoff.
//! Transaction persistence and commit authority remain in `update_download`.
use std::io;
use std::path::{Path, PathBuf};
use std::process::Child;

pub(crate) fn canonical(path: &Path) -> io::Result<PathBuf> {
    let path = std::fs::canonicalize(path)?;
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            return Ok(PathBuf::from(format!(r"\\{unc}")));
        }
        if let Some(local) = text.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(local));
        }
    }
    Ok(path)
}

/// FILETIME identity is compared with the exact process handle by the helper.
pub(crate) fn current_process_created() -> io::Result<u64> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
        let mut created: FILETIME = unsafe { std::mem::zeroed() };
        let mut exited = created;
        let mut kernel = created;
        let mut user = created;
        // SAFETY: the pseudo-handle is valid and each output is a live FILETIME.
        if unsafe {
            GetProcessTimes(GetCurrentProcess(), &mut created, &mut exited, &mut kernel, &mut user)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
    #[cfg(not(windows))]
    Err(io::Error::new(io::ErrorKind::Unsupported, "Windows process identity is unavailable"))
}

pub(crate) fn spawn_helper(helper: &Path, plan: &Path) -> io::Result<Child> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        use std::process::{Command, Stdio};
        let powershell = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "Windows directory is unavailable")
            })?
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        Command::new(powershell)
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(helper)
            .arg("-PlanPath")
            .arg(plan)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
    }
    #[cfg(not(windows))]
    {
        let _ = (helper, plan);
        Err(io::Error::new(io::ErrorKind::Unsupported, "Windows installation is unavailable"))
    }
}
