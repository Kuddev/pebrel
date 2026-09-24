//! Native paths and process identity for the platform installer handoff.
//! Transaction persistence and commit authority remain in `update_download`.
use std::io;
use std::path::{Path, PathBuf};
use std::process::Child;

#[cfg(target_os = "macos")]
pub(crate) mod macos;

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

/// Native creation identity prevents a recycled PID from authorizing a handoff.
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
    #[cfg(target_os = "macos")]
    return macos::process_created(std::process::id())?
        .ok_or_else(|| io::Error::other("Process identity unavailable"));
    #[cfg(not(any(windows, target_os = "macos")))]
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

pub(crate) fn installation_directory(executable: &Path) -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    return macos::bundle(executable);
    #[cfg(not(target_os = "macos"))]
    {
        let directory = executable.parent().ok_or("Missing application directory")?;
        if !directory.join("unins000.exe").is_file() {
            return Err(
                "This copy is portable. Use the download page to replace its package.".into()
            );
        }
        Ok(directory.to_owned())
    }
}

pub(crate) fn guard_base(executable: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Ok(bundle) = macos::bundle(executable) {
        if let (Some(parent), Some(name)) = (bundle.parent(), bundle.file_name()) {
            use sha2::{Digest as _, Sha256};
            use std::os::unix::ffi::OsStrExt as _;
            let identity: String =
                Sha256::digest(name.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
            return parent.join(format!(".pebrel-update-{identity}"));
        }
    }
    executable.parent().expect("canonical executable parent").join(".pebrel-update")
}

/// Select and materialize the native helper; transaction authority stays with the caller.
pub(crate) fn spawn_prepared_helper(directory: &Path, plan: &Path) -> Result<Child, String> {
    #[cfg(target_os = "macos")]
    return macos::spawn(directory, plan);
    #[cfg(not(target_os = "macos"))]
    {
        let helper = directory.join("handoff.ps1");
        crate::atomic_file::write(&helper, include_bytes!("../update_download/handoff.ps1"))
            .map_err(|error| error.to_string())?;
        spawn_helper(&helper, plan).map_err(|error| error.to_string())
    }
}
