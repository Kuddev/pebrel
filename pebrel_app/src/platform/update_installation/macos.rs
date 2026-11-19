//! macOS bundle I/O. Transaction authorization stays in update_download::handoff.
use std::ffi::{CStr, CString};
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

pub(crate) fn spawn(directory: &Path, plan: &Path) -> Result<Child, String> {
    let helper = directory.join("handoff");
    std::fs::copy(std::env::current_exe().map_err(|e| e.to_string())?, &helper)
        .map_err(|e| e.to_string())?;
    Command::new(helper)
        .arg("--internal-macos-update")
        .arg(plan)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())
}

pub(crate) fn run(command: &mut Command) -> Result<Output, String> {
    // Regular files cannot fill a pipe while we poll for process completion.
    let stdout = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    let stderr = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(stdout.reopen().map_err(|e| e.to_string())?)
        .stderr(stderr.reopen().map_err(|e| e.to_string())?)
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(120);
    while child.try_wait().map_err(|e| e.to_string())?.is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("macOS update command timed out".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let read_output = |file: &tempfile::NamedTempFile| -> Result<Vec<u8>, String> {
        if file.as_file().metadata().map_err(|e| e.to_string())?.len() > 2 * 1024 * 1024 {
            return Err("macOS update command output exceeded its limit".into());
        }
        std::fs::read(file.path()).map_err(|e| e.to_string())
    };
    let output = Output {
        status: child.wait().map_err(|e| e.to_string())?,
        stdout: read_output(&stdout)?,
        stderr: read_output(&stderr)?,
    };
    if !output.status.success() {
        return Err(format!(
            "macOS update command failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).chars().take(2048).collect::<String>()
        ));
    }
    Ok(output)
}

pub(crate) fn property(bundle: &Path, key: &str) -> Result<String, String> {
    let result = run(Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(bundle.join("Contents/Info.plist")))?;
    String::from_utf8(result.stdout).map(|s| s.trim().to_owned()).map_err(|e| e.to_string())
}

pub(crate) fn bundle(executable: &Path) -> Result<PathBuf, String> {
    let macos = executable.parent().ok_or("Missing executable directory")?;
    let contents = macos.parent().ok_or("Missing bundle contents")?;
    let bundle = contents.parent().ok_or("Missing application bundle")?;
    if executable.file_name().is_none_or(|n| n != "pebrel")
        || macos.file_name().is_none_or(|n| n != "MacOS")
        || contents.file_name().is_none_or(|n| n != "Contents")
        || bundle.extension().is_none_or(|e| e != "app")
    {
        return Err("Automatic installation requires a packaged Pebrel .app. Install it from the Releases page first.".into());
    }
    if !matches!(
        property(bundle, "CFBundleIdentifier")?.as_str(),
        "io.github.kuddev.pebrel" | "io.github.kuddev.pebrel.preview"
    ) || property(bundle, "CFBundleExecutable")? != "pebrel"
    {
        return Err("This is not an official Pebrel application bundle".into());
    }
    if bundle.starts_with("/Volumes")
        || bundle.components().any(|p| p.as_os_str() == "AppTranslocation")
    {
        return Err(
            "Move Pebrel to a writable Applications folder before installing updates".into()
        );
    }
    Ok(bundle.to_owned())
}

pub(crate) fn process_created(pid: u32) -> io::Result<Option<u64>> {
    // SAFETY: buffer is an initialized proc_bsdinfo of exactly the requested size.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as i32;
    let read = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    if read == size {
        // Zombies have already stopped using their application bundle.
        if info.pbi_status == 5 {
            return Ok(None);
        }
        return Ok(Some(info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec));
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) { Ok(None) } else { Err(error) }
}

pub(crate) fn running_copies(executable: &Path) -> Result<Vec<u32>, String> {
    // proc_listallpids returns a count, unlike proc_listpids' byte count.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err("Could not inspect running applications".into());
    }
    let mut pids = vec![0i32; count as usize + 1024];
    let read = unsafe {
        libc::proc_listallpids(
            pids.as_mut_ptr().cast(),
            (pids.len() * std::mem::size_of::<i32>()) as i32,
        )
    };
    if read <= 0 || read as usize >= pids.len() {
        return Err("Process snapshot was incomplete".into());
    }
    let mut result = Vec::new();
    for pid in pids.into_iter().take(read as usize).filter(|pid| *pid > 0) {
        let mut buffer = vec![0u8; 4096];
        let read =
            unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
        if read > 0 {
            let path = CStr::from_bytes_until_nul(&buffer).map_err(|e| e.to_string())?;
            if Path::new(std::ffi::OsStr::from_bytes(path.to_bytes())) == executable {
                result.push(pid as u32);
            }
        }
    }
    Ok(result)
}

pub(crate) fn exchange(first: &Path, second: &Path) -> Result<(), String> {
    let first = CString::new(first.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let second = CString::new(second.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    // Same-volume atomic swap: the original app never disappears, even on crash.
    let result = unsafe {
        libc::renameatx_np(
            libc::AT_FDCWD,
            first.as_ptr(),
            libc::AT_FDCWD,
            second.as_ptr(),
            libc::RENAME_SWAP,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(())
}

pub(crate) fn verify_bundle(bundle: &Path, version: &str) -> Result<(), String> {
    let id = property(bundle, "CFBundleIdentifier")?;
    if !matches!(id.as_str(), "io.github.kuddev.pebrel" | "io.github.kuddev.pebrel.preview")
        || property(bundle, "CFBundleExecutable")? != "pebrel"
        || property(bundle, "CFBundleShortVersionString")? != version
    {
        return Err("The downloaded bundle identity or version does not match the release".into());
    }
    let executable = bundle.join("Contents/MacOS/pebrel");
    if std::fs::canonicalize(&executable).map_err(|e| e.to_string())? != executable {
        return Err("The bundle executable must not redirect outside the application".into());
    }
    run(Command::new("/usr/bin/codesign").args(["--verify", "--deep", "--strict"]).arg(bundle))?;
    let arch = if std::env::consts::ARCH == "aarch64" { "arm64" } else { "x86_64" };
    let architectures = run(Command::new("/usr/bin/lipo").arg("-archs").arg(&executable))?;
    if !String::from_utf8_lossy(&architectures.stdout).split_whitespace().any(|v| v == arch) {
        return Err("The update does not contain this Mac's architecture".into());
    }
    let output = run(Command::new(&executable).arg("--version"))?;
    let output = String::from_utf8_lossy(&output.stdout);
    if output.split_whitespace().nth(1) != Some(version) {
        return Err("The update executable reports a different version".into());
    }
    Ok(())
}

fn team(bundle: &Path) -> Result<Option<String>, String> {
    let output = run(Command::new("/usr/bin/codesign").args(["-d", "--verbose=4"]).arg(bundle))?;
    Ok(String::from_utf8_lossy(&output.stderr)
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier="))
        .filter(|team| *team != "not set")
        .map(str::to_owned))
}

pub(crate) fn stage(
    installer: &Path,
    original: &Path,
    staged: &Path,
    mount: &Path,
    version: &str,
) -> Result<(), String> {
    std::fs::create_dir(mount).map_err(|e| e.to_string())?;
    let outcome: Result<(), String> = (|| {
        run(Command::new("/usr/bin/hdiutil")
            .args(["attach", "-readonly", "-nobrowse", "-noautoopen", "-verify", "-mountpoint"])
            .arg(mount)
            .arg(installer))?;
        let bundles: Vec<_> = std::fs::read_dir(mount)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().is_some_and(|e| e == "app") && path.is_dir() && !path.is_symlink()
            })
            .collect();
        if bundles.len() != 1 {
            return Err("The disk image must contain exactly one application".into());
        }
        // Copy before checking/executing; all subsequent verification uses the local staged tree.
        run(Command::new("/usr/bin/ditto").arg(&bundles[0]).arg(staged))?;
        if let Some(expected) = team(original)?
            && team(staged)?.as_deref() != Some(expected.as_str())
        {
            return Err("The update's signing team differs from the installed application".into());
        }
        verify_bundle(staged, version)?;
        Ok(())
    })();
    let detached = run(Command::new("/usr/bin/hdiutil").args(["detach"]).arg(mount));
    let _ = std::fs::remove_dir(mount);
    outcome?;
    detached?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_output_larger_than_a_pipe_is_drained() {
        let output = run(Command::new("/bin/sh")
            .args(["-c", "head -c 262144 /dev/zero; head -c 262144 /dev/zero >&2"]))
        .unwrap();
        assert_eq!(output.stdout.len(), 262144);
        assert_eq!(output.stderr.len(), 262144);
        assert!(run(Command::new("/bin/sh").args(["-c", "exit 7"])).is_err());
    }

    #[test]
    fn atomic_exchange_keeps_a_complete_original_and_can_roll_back() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("Pebrel.app");
        let new = dir.path().join("staged.app");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        std::fs::write(old.join("marker"), "old").unwrap();
        std::fs::write(new.join("marker"), "new").unwrap();
        exchange(&old, &new).unwrap();
        assert_eq!(std::fs::read_to_string(old.join("marker")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(new.join("marker")).unwrap(), "old");
        exchange(&old, &new).unwrap();
        assert_eq!(std::fs::read_to_string(old.join("marker")).unwrap(), "old");
        assert!(exchange(&old, &dir.path().join("missing")).is_err());
        assert_eq!(std::fs::read_to_string(old.join("marker")).unwrap(), "old");
    }
    #[test]
    fn developer_executables_are_not_treated_as_installable_bundles() {
        assert!(bundle(Path::new("/tmp/target/debug/pebrel")).is_err());
        assert!(bundle(Path::new("/tmp/Pebrel.app/Contents/MacOS/not-pebrel")).is_err());
    }
    #[test]
    fn process_identity_and_executable_inspection_find_this_process() {
        let created = process_created(std::process::id()).unwrap().unwrap();
        assert!(created > 0);
        assert_eq!(process_created(std::process::id()).unwrap(), Some(created));
        let executable = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        assert!(running_copies(&executable).unwrap().contains(&std::process::id()));
    }
}
