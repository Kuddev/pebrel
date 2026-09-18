use std::path::Path;
use std::process::{Command, Stdio};

use super::Platform;

pub fn open(path: &Path) -> std::io::Result<()> {
    open_command(Platform::current(), path).spawn().map(|_| ())
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
    fn open_keeps_the_path_as_one_argument_on_every_platform() {
        let path = Path::new("project with spaces").join("file.txt");
        for (platform, program) in [
            (Platform::Windows, "explorer.exe"),
            (Platform::MacOS, "open"),
            (Platform::Linux, "xdg-open"),
        ] {
            let command = open_command(platform, &path);
            assert_eq!(command.get_program(), program);
            assert_eq!(command.get_args().collect::<Vec<_>>(), [path.as_os_str()]);
        }
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
