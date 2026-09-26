//! Windows UAC launch adapter. The requested program is always hosted by Pebrel.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::Path;

pub(crate) const SUPPORTED: bool = cfg!(windows);

/// A failed token query must not publish a potentially privileged control plane.
pub(crate) fn requires_isolation() -> bool {
    is_elevated().unwrap_or(true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaunchOutcome {
    Started,
    Cancelled,
}

/// CLI arguments, rather than a shell command: shell metacharacters stay literal.
pub(crate) fn arguments(program: &str, args: &[String], cwd: Option<&Path>) -> Vec<OsString> {
    let mut result = vec![OsString::from("--gpui")];
    if let Some(cwd) = cwd {
        result.push("--working-directory".into());
        result.push(cwd.as_os_str().to_owned());
    }
    result.push("-e".into());
    result.push(program.into());
    result.extend(args.iter().map(OsString::from));
    result
}

#[cfg(windows)]
pub(crate) fn is_elevated() -> io::Result<bool> {
    use std::sync::OnceLock;
    static ELEVATED: OnceLock<Result<bool, i32>> = OnceLock::new();
    ELEVATED.get_or_init(query_token).clone().map_err(io::Error::from_raw_os_error)
}

#[cfg(windows)]
fn query_token() -> Result<bool, i32> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // SAFETY: buffers have their declared sizes; the token is closed on every path.
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(GetLastError() as i32);
        }
        let mut info: TOKEN_ELEVATION = std::mem::zeroed();
        let mut returned = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut info as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let error = GetLastError();
        CloseHandle(token);
        if ok == 0 { Err(error as i32) } else { Ok(info.TokenIsElevated != 0) }
    }
}

#[cfg(not(windows))]
pub(crate) fn is_elevated() -> io::Result<bool> {
    Ok(false)
}

#[cfg(windows)]
fn quoted_argument(argument: &OsStr) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    let mut result = vec![b'"' as u16];
    let mut backslashes = 0;
    for ch in argument.encode_wide() {
        if ch == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "argument contains NUL"));
        }
        if ch == b'\\' as u16 {
            backslashes += 1;
            continue;
        }
        let count = if ch == b'"' as u16 { backslashes * 2 + 1 } else { backslashes };
        result.extend(std::iter::repeat_n(b'\\' as u16, count));
        result.push(ch);
        backslashes = 0;
    }
    result.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2));
    result.push(b'"' as u16);
    Ok(result)
}

/// Runs on a worker: UAC may wait for the user and must not block the UI loop.
#[cfg(windows)]
pub(crate) fn launch(args: &[OsString]) -> io::Result<LaunchOutcome> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{ERROR_CANCELLED, GetLastError};
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize,
    };
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let executable = std::env::current_exe()?;
    let file: Vec<u16> = executable.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut parameters = Vec::new();
    for argument in args {
        if !parameters.is_empty() {
            parameters.push(b' ' as u16);
        }
        parameters.extend(quoted_argument(argument)?);
    }
    parameters.push(0);
    let verb: Vec<u16> = "runas\0".encode_utf16().collect();
    // SAFETY: all strings and the initialized structure outlive ShellExecuteExW.
    // SEE_MASK_NOASYNC completes the shell's launch handshake before COM teardown.
    unsafe {
        let com = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = parameters.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        let ok = ShellExecuteExW(&mut info);
        let error = GetLastError();
        if com >= 0 {
            CoUninitialize();
        }
        if ok != 0 {
            Ok(LaunchOutcome::Started)
        } else if error == ERROR_CANCELLED {
            Ok(LaunchOutcome::Cancelled)
        } else {
            Err(io::Error::from_raw_os_error(error as i32))
        }
    }
}

#[cfg(not(windows))]
pub(crate) fn launch(_args: &[OsString]) -> io::Result<LaunchOutcome> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "UAC is only available on Windows"))
}

#[cfg(test)]
mod tests;
