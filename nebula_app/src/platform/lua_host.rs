//! Platform conventions the Lua 5.4 configuration compatibility layer reproduces.
//!
//! Lua 5.4 inherited these from the C runtime: `popen`/`system` use the platform
//! shell, text-mode files translate CRLF on Windows, and `package.path` uses the
//! platform directory separator and default search path.

use std::path::Path;
use std::process::{Command, ExitStatus};

/// `package.config`'s directory separator.
pub(crate) const DIRECTORY_SEPARATOR: &str = if cfg!(windows) { "\\" } else { "/" };

/// A hidden command running `command` through the platform shell, as C `popen` does.
pub(crate) fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    let mut process = {
        use std::os::windows::process::CommandExt as _;
        let mut process =
            Command::new(std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into()));
        process.raw_arg("/c").raw_arg(command);
        process
    };
    #[cfg(not(windows))]
    let mut process = {
        let mut process = Command::new("/bin/sh");
        process.arg("-c").arg(command);
        process
    };
    super::process::hidden_command(&mut process);
    process
}

/// The terminating signal of a Unix child; always `None` elsewhere.
pub(crate) fn exit_signal(status: ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    return std::os::unix::process::ExitStatusExt::signal(&status);
    #[cfg(not(unix))]
    {
        let _ = status;
        None
    }
}

/// Reads text-mode data: Windows C runtimes turn CRLF into LF.
pub(crate) fn text_mode_read(data: Vec<u8>) -> Vec<u8> {
    if !cfg!(windows) || !data.contains(&b'\r') {
        return data;
    }
    let mut translated = Vec::with_capacity(data.len());
    let mut bytes = data.iter().peekable();
    while let Some(&byte) = bytes.next() {
        if byte != b'\r' || bytes.peek() != Some(&&b'\n') {
            translated.push(byte);
        }
    }
    translated
}

/// Writes text-mode data: Windows C runtimes turn LF into CRLF.
pub(crate) fn text_mode_write(data: Vec<u8>) -> Vec<u8> {
    if !cfg!(windows) {
        return data;
    }
    String::from_utf8_lossy(&data).replace('\n', "\r\n").into_bytes()
}

/// Lua 5.4's built-in `package.path` when `LUA_PATH` is unset.
pub(crate) fn lua54_default_path() -> String {
    if cfg!(windows) {
        let program = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_owned))
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        [
            "!\\lua\\?.lua",
            "!\\lua\\?\\init.lua",
            "!\\?.lua",
            "!\\?\\init.lua",
            "!\\..\\share\\lua\\5.4\\?.lua",
            "!\\..\\share\\lua\\5.4\\?\\init.lua",
            ".\\?.lua",
            ".\\?\\init.lua",
        ]
        .map(|template| template.replace('!', &program))
        .join(";")
    } else {
        [
            "/usr/local/share/lua/5.4/?.lua",
            "/usr/local/share/lua/5.4/?/init.lua",
            "/usr/local/lib/lua/5.4/?.lua",
            "/usr/local/lib/lua/5.4/?/init.lua",
            "./?.lua",
            "./?/init.lua",
        ]
        .join(";")
    }
}
