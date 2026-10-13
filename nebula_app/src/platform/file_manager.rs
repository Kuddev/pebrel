use std::path::Path;
use std::process::{Command, Stdio};

use super::Platform;

/// How an "open" request is dispatched. Windows must go through the Shell's
/// default `open` verb so the user's configured folder handler is honored;
/// spawning `explorer.exe` would bypass that choice.
enum OpenLaunch {
    DefaultHandler,
    Command(Command),
}

/// Open a path with the platform's default handler.
///
/// On Windows this synchronously asks the Shell, which may wait for a shell
/// extension or a slow network path; UI callers should run it on a worker.
pub fn open(path: &Path) -> std::io::Result<()> {
    let res = open_launch(Platform::current(), path).run(path);
    #[cfg(target_os = "linux")]
    if res.is_err() {
        if command("gio").arg("open").arg(path).spawn().is_ok() {
            return Ok(());
        }
    }
    res
}

fn open_launch(platform: Platform, path: &Path) -> OpenLaunch {
    match platform {
        Platform::Windows => OpenLaunch::DefaultHandler,
        Platform::MacOS | Platform::Linux => OpenLaunch::Command(open_command(platform, path)),
    }
}

impl OpenLaunch {
    fn run(self, path: &Path) -> std::io::Result<()> {
        match self {
            Self::DefaultHandler => open_default_handler(path),
            Self::Command(mut command) => command.spawn().map(|_| ()),
        }
    }
}

/// Hand the directory to the shell's `open` verb; `ShellExecuteExW` resolves the
/// user's default folder handler instead of hard-coding a file manager.
///
/// This call may wait for a shell extension or a slow network redirector, so UI
/// callers should run it on a worker thread (the tab menu does).
#[cfg(windows)]
fn open_default_handler(path: &Path) -> std::io::Result<()> {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let (verb, file) = shell_open_request(path);
    // Background executor threads do not inherit the UI thread's COM apartment.
    // A non-default folder handler can host shell extensions that require STA.
    let com = unsafe {
        CoInitializeEx(std::ptr::null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32)
    };
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.nShow = SW_SHOWNORMAL;
    // SAFETY: the structure and both UTF-16 buffers outlive the call.
    let launched = unsafe { ShellExecuteExW(&mut info) };
    let error = unsafe { GetLastError() };
    if com >= 0 {
        unsafe { CoUninitialize() };
    }
    shell_execute_result(launched != 0, error)
}

#[cfg(any(windows, test))]
fn shell_execute_result(launched: bool, error: u32) -> std::io::Result<()> {
    if launched { Ok(()) } else { Err(std::io::Error::from_raw_os_error(error as i32)) }
}

/// Only the Windows build can select [`OpenLaunch::DefaultHandler`]; the other
/// targets keep the branch compilable without linking the Shell API.
#[cfg(not(windows))]
fn open_default_handler(_path: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "the Windows Shell handler is unavailable on this platform",
    ))
}

#[cfg(windows)]
fn shell_open_request(path: &Path) -> (Vec<u16>, Vec<u16>) {
    use std::os::windows::ffi::OsStrExt;

    let verb = "open\0".encode_utf16().collect();
    let file = path.as_os_str().encode_wide().chain(Some(0)).collect();
    (verb, file)
}

pub fn reveal(path: &Path) -> std::io::Result<()> {
    match reveal_command(Platform::current(), path) {
        Some(mut command) => command.spawn().map(|_| ()),
        None => Ok(()),
    }
}

fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    command
}

fn open_command(platform: Platform, path: &Path) -> Command {
    let program = match platform {
        Platform::Windows => "explorer.exe",
        Platform::MacOS => "open",
        Platform::Linux => "xdg-open",
    };
    let mut command = command(program);
    command.arg(path);
    command
}

fn reveal_command(platform: Platform, path: &Path) -> Option<Command> {
    let mut command = match platform {
        Platform::Windows => return Some(windows_reveal_command(path)),
        Platform::MacOS => command("open"),
        Platform::Linux => return path.parent().map(|parent| open_command(platform, parent)),
    };
    command.arg("-R").arg(path);
    Some(command)
}

/// explorer 自己解析命令行，不遵守 CommandLineToArgvW：`/select,<path>` 整体被
/// 标准 quoting 包成 `"/select,<path>"` 后，它会在路径的第一个空格处截断，
/// 目标不存在就回退到仍然存在的祖先目录（实测 `D:\…\My Projects\…\file`
/// 打开了 `D:\Documents`）。引号必须只包住路径，即 `/select,"<path>"`，
/// 所以用 `raw_arg` 跳过标准 quoting。Windows 路径本身不能含 `"`，拼接安全。
#[cfg(windows)]
fn windows_reveal_command(path: &Path) -> Command {
    use std::os::windows::process::CommandExt;
    let mut command = command("explorer.exe");
    command.raw_arg(windows_select_arg(path));
    command
}

/// 非 Windows 构建只用于跨平台测试：让 argv 内容与 Windows 上 `raw_arg`
/// 写入的原始串完全一致，测试因此不需要按平台分叉。
#[cfg(not(windows))]
fn windows_reveal_command(path: &Path) -> Command {
    let mut command = command("explorer.exe");
    command.arg(windows_select_arg(path));
    command
}

fn windows_select_arg(path: &Path) -> std::ffi::OsString {
    let mut select = std::ffi::OsString::from("/select,\"");
    select.push(path.as_os_str());
    select.push(std::ffi::OsStr::new("\""));
    select
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    #[test]
    fn open_uses_the_shell_default_handler_on_windows() {
        let path = Path::new("project with spaces");
        assert!(matches!(open_launch(Platform::Windows, path), OpenLaunch::DefaultHandler));
    }

    #[test]
    fn shell_execute_failures_surface_the_windows_error_code() {
        assert!(shell_execute_result(true, 0).is_ok());
        let error = shell_execute_result(false, 5).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(5));
        let error = shell_execute_result(false, 0).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(0));
    }

    #[test]
    fn open_keeps_the_path_as_one_argument_off_windows() {
        let path = Path::new("project with spaces").join("file.txt");
        for (platform, program) in [(Platform::MacOS, "open"), (Platform::Linux, "xdg-open")] {
            let OpenLaunch::Command(command) = open_launch(platform, &path) else {
                panic!("expected a command launch outside Windows");
            };
            assert_eq!(command.get_program(), program);
            assert_eq!(command.get_args().collect::<Vec<_>>(), [path.as_os_str()]);
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_open_requests_the_shell_open_verb_for_the_directory() {
        let (verb, file) = shell_open_request(Path::new(r"D:\projects\demo"));
        let text = |buffer: &[u16]| String::from_utf16(&buffer[..buffer.len() - 1]).unwrap();
        assert_eq!(text(&verb), "open");
        assert_eq!(text(&file), r"D:\projects\demo");
    }

    #[test]
    fn reveal_preserves_platform_selection_semantics() {
        let path = Path::new("project with spaces").join("file.txt");
        for (platform, program, arguments) in [
            (Platform::Windows, "explorer.exe", vec![windows_select_arg(&path)]),
            (Platform::MacOS, "open", vec![OsString::from("-R"), path.clone().into_os_string()]),
            (Platform::Linux, "xdg-open", vec![path.parent().unwrap().as_os_str().to_owned()]),
        ] {
            let command = reveal_command(platform, &path).unwrap();
            assert_eq!(command.get_program(), program);
            assert_eq!(command.get_args().collect::<Vec<_>>(), arguments);
        }
        assert!(reveal_command(Platform::Linux, Path::new("")).is_none());
    }

    #[test]
    fn windows_reveal_quotes_only_the_path_after_the_select_switch() {
        let arg = windows_select_arg(Path::new("D:\\Documents\\HTA\\My Projects\\file.txt"));
        assert_eq!(arg, OsString::from("/select,\"D:\\Documents\\HTA\\My Projects\\file.txt\""));
    }
}
