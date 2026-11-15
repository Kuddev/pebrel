//! Windows UAC launch adapter. The requested program is always hosted by Pebrel.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::Path;

pub(crate) const SUPPORTED: bool = cfg!(windows);

#[cfg(feature = "gpui-shell")]
pub(crate) mod handover;

/// 提升窗口隔离判定：进程提升而桌面 shell 非提升才隔离。
///
/// 旧判定「进程提升即隔离」在内置 Administrator 桌面会话下会退化为会话
/// 保存整体静默失效——那种会话关闭了 UAC Admin Approval Mode，explorer
/// 和一切用户进程同为提升，于是每一个 pebrel 实例都被当成「管理员窗口」，
/// session 永不落盘，恢复功能在整台机器上不可用。与桌面 shell 对齐后：
/// 管理员桌面会话正常保存/恢复；普通账户下 UAC runas 打开的提权窗口
/// 仍不覆盖普通工作区（原始设计意图）。
pub(crate) fn requires_isolation() -> bool {
    isolate(is_elevated().map_err(|_| ()), desktop_shell_elevated())
}

/// 真值表核心，独立成函数供单测。进程侧入参是**查询结果**（含失败）：
/// token 查询失败是「未知」而非「非提升」，不得进入与桌面 shell 的合成
/// 判定——桌面提升不能洗白一个状态未知的进程（失败同样可能发生在真正的
/// runas 提权进程上），fail-closed 直接隔离。`None` 表示查询不到桌面
/// shell（SSH/服务会话），按非提升处理：保守回退到旧的仅看进程语义。
fn isolate(process_elevated: Result<bool, ()>, desktop_shell: Option<bool>) -> bool {
    match process_elevated {
        Err(()) => true,
        Ok(elevated) => elevated && !desktop_shell.unwrap_or(false),
    }
}

/// 桌面 shell（explorer.exe）是否提升。结果进程内缓存：token 不随运行
/// 变化，而该判定在每个窗口的持久化路径上都会被问到。
#[cfg(windows)]
fn desktop_shell_elevated() -> Option<bool> {
    use std::sync::OnceLock;
    static ELEVATED: OnceLock<Option<bool>> = OnceLock::new();
    *ELEVATED.get_or_init(query_desktop_shell_elevation)
}

#[cfg(windows)]
fn query_desktop_shell_elevation() -> Option<bool> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_MANDATORY_LABEL,
        TOKEN_QUERY, TokenIntegrityLevel,
    };
    use windows_sys::Win32::System::SystemServices::SECURITY_MANDATORY_HIGH_RID;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};

    // SAFETY: handles are closed on every path; buffers have their declared
    // sizes; the two-pass GetTokenInformation length protocol is followed.
    unsafe {
        // 桌面 shell 窗口定位 explorer：SSH/服务会话没有它，返回 None 走回退。
        let shell = GetShellWindow();
        if shell.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(shell, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            log::debug!("desktop shell process query failed: {}", GetLastError());
            return None;
        }
        let mut token = std::ptr::null_mut();
        let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token);
        let open_error = GetLastError();
        CloseHandle(process);
        if opened == 0 {
            log::debug!("desktop shell token query failed: {open_error}");
            return None;
        }

        // TokenIntegrityLevel 返回变长的 TOKEN_MANDATORY_LABEL（SID 变长），
        // 按标准两段式协议先问长度再取值。
        let mut needed = 0u32;
        GetTokenInformation(token, TokenIntegrityLevel, std::ptr::null_mut(), 0, &mut needed);
        if needed == 0 {
            CloseHandle(token);
            return None;
        }
        let mut buffer = vec![0u8; needed as usize];
        let ok = GetTokenInformation(
            token,
            TokenIntegrityLevel,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        );
        let info_error = GetLastError();
        CloseHandle(token);
        if ok == 0 {
            log::debug!("desktop shell integrity query failed: {info_error}");
            return None;
        }
        let label = &*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>();
        let count = *GetSidSubAuthorityCount(label.Label.Sid);
        if count == 0 {
            return None;
        }
        // SID 最后一个 sub-authority 就是完整性级别 RID；High 及以上视作提升。
        let rid = *GetSidSubAuthority(label.Label.Sid, u32::from(count - 1));
        Some(rid >= SECURITY_MANDATORY_HIGH_RID as u32)
    }
}

#[cfg(not(windows))]
fn desktop_shell_elevated() -> Option<bool> {
    None
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
