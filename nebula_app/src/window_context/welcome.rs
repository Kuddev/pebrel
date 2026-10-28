//! Nebula 新 tab 欢迎页。
//!
//! Windows 上渲染彩色字符 logo + fastfetch 风格的信息卡（通过一个缓存的
//! PowerShell 脚本秒出）；非 Windows 上回退到 `fastfetch`/`uname`。
//! 欢迎内容与 [`WindowContext`](super::WindowContext) 的状态解耦；缓存只保留
//! 脚本字节，临时文件按需重建。入口是 [`nebula_fastfetch_intro_command`]。

#[cfg(windows)]
use crate::display::NebulaShell;
#[cfg(any(windows, test))]
use std::sync::Mutex;

/// 欢迎内容的一份落盘副本：内容在 Rust 侧生成一次，只按需重建文件。
#[cfg(any(windows, test))]
struct CachedIntroFile {
    bytes: Vec<u8>,
    needs_write: bool,
}

#[cfg(any(windows, test))]
fn nebula_fastfetch_text(narrow: bool) -> String {
    let cyan = "\x1b[38;2;0;229;255m";
    let green = "\x1b[38;2;0;205;64m";
    let magenta = "\x1b[38;2;190;45;220m";
    let white = "\x1b[97m";
    let dim = "\x1b[38;2;180;186;200m";
    let icon = "\x1b[38;2;150;140;220m";
    let reset = "\x1b[0m";
    // Keycap styling for shortcut combos: grey pill with builtin powerline round
    // caps (e0b6/e0b4) so the background reads as a rounded keycap, light ink.
    let kbd = |k: &str| {
        format!(
            "\x1b[38;2;48;52;68m\u{e0b6}\x1b[48;2;48;52;68m\x1b[38;2;224;228;242m{k}\x1b[0m\x1b[38;2;48;52;68m\u{e0b4}\x1b[0m"
        )
    };

    let cwd = std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| ".".to_owned());
    let art = super::nebula_fetch_art::NEBULA_STAR_ART;
    let info = [
        format!("{white}Welcome to {green}{}{white},{reset}", crate::brand::NAME),
        format!(
            "{white}a fast terminal workspace for {cyan}tabs{white}, {cyan}splits{white} and shells.{reset}"
        ),
        String::new(),
        format!(
            "{icon}\u{f02b}{reset}  {dim}Version:{reset} {green}{}{reset}",
            env!("CARGO_PKG_VERSION")
        ),
        format!(
            "{icon}\u{f09b}{reset}  {dim}GitHub:{reset} {cyan}https://github.com/Kuddev/pebrel{reset}"
        ),
        format!(
            "{icon}\u{f11c}{reset}  {dim}Shortcuts:{reset}  {a}{white} new tab · {b}{white} switch tabs{reset}",
            a = kbd("Ctrl+Shift+T"),
            b = kbd("Ctrl+1..5"),
        ),
        format!(
            "{icon}\u{f0db}{reset}  {dim}Split:{reset}  {a}{white} / {b}{white} split · drag to resize{reset}",
            a = kbd("Ctrl+Shift+D"),
            b = kbd("Ctrl+Shift+S"),
        ),
        format!(
            "{icon}\u{f009}{reset}  {dim}Panes:{reset}  {a}{white} focus · {b}{white} zoom · {c}{white} close{reset}",
            a = kbd("Ctrl+Alt+Arrows"),
            b = kbd("Ctrl+Shift+Enter"),
            c = kbd("Ctrl+Shift+W"),
        ),
        format!(
            "{icon}\u{f1fc}{reset}  {dim}Theme:{reset} {white}use the gear menu for chrome and completion settings{reset}"
        ),
        format!("{icon}\u{f07b}{reset}  {dim}Current Directory:{reset} {magenta}{cwd}{reset}"),
        String::new(),
        // Faint horizontal rule before the closing line, like the reference.
        format!("\x1b[38;2;58;63;84m{}{reset}", "─".repeat(56)),
        String::new(),
        format!("{icon}\u{f135}{reset}  {green}Startup Time:{reset} {white}ready{reset}"),
    ];

    let rows = art.len().max(info.len());
    let info_col = 65; // the density-mapped galaxy art is 62 visible columns wide, +3 gap
    // Disable auto-wrap (DECAWM) while the intro prints: on narrow windows a
    // long info row must clip at the right edge, not wrap into the galaxy art.
    let mut text = String::from("\x1b[?7l");
    if narrow {
        // Narrow pane (e.g. after a split): stack the info below the art
        // instead of the two-column layout, which needs ~150 columns.
        for logo in art {
            text.push_str(logo);
            text.push_str("\r\n");
        }
        text.push_str("\r\n");
        for line in info.iter().filter(|l| !l.is_empty()) {
            text.push_str(line);
            text.push_str("\r\n");
        }
        text.push_str("\x1b[?7h");
        return text;
    }
    for row in 0..rows {
        if let Some(logo) = art.get(row) {
            text.push_str(logo); // pre-coloured cells, already terminated with reset
        }
        let right = info.get(row).map(String::as_str).unwrap_or("");
        if !right.is_empty() {
            // 不写填充空格，直接把光标移动到右栏；否则 logo 中间的空行会
            // 在终端选择时变成一整条空白带，看起来比设计稿宽很多。
            text.push_str(&format!("\x1b[{info_col}G"));
            text.push_str(right);
        }
        text.push_str("\r\n");
    }
    text.push_str("\x1b[?7h"); // restore auto-wrap for the shell session
    text
}

#[cfg(any(windows, test))]
fn nebula_fastfetch_script_bytes(narrow: bool) -> Vec<u8> {
    let text = nebula_fastfetch_text(narrow).replace("\r\n", "\n").replace('\r', "\n");
    let script = format!(
        "$OutputEncoding = [System.Text.Encoding]::UTF8\r\n\
                 [Console]::OutputEncoding = [System.Text.Encoding]::UTF8\r\n\
                 [Console]::Write(@'\r\n{text}\r\n'@)\r\n"
    );

    // Windows PowerShell 5.1 会把无 BOM UTF-8 当作本地 ANSI；这里写 BOM，
    // 保证字符 logo 与 ANSI 序列在脚本文件中稳定解析。
    let mut bytes = Vec::with_capacity(3 + script.len());
    bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    bytes.extend_from_slice(script.as_bytes());
    bytes
}

#[cfg(any(windows, test))]
fn cached_intro_file(
    cell: &Mutex<Option<CachedIntroFile>>,
    path: std::path::PathBuf,
    bytes: impl FnOnce() -> Vec<u8>,
    write_file: impl FnOnce(&std::path::Path, &[u8]) -> std::io::Result<()>,
) -> Option<std::path::PathBuf> {
    // Keep the bytes, not the lifetime of a file in the system temp directory.
    // Serialize recreation so callers in this process cannot read a partial write.
    let mut cached = cell.lock().ok()?;
    let file = cached.get_or_insert_with(|| CachedIntroFile { bytes: bytes(), needs_write: true });
    if file.needs_write || !path.is_file() {
        // Initial requests replace files left by older processes. Keep this flag
        // set after an error, including a failed write that leaves a partial file.
        file.needs_write = true;
        write_file(&path, &file.bytes).ok()?;
        file.needs_write = false;
    }
    Some(path)
}

#[cfg(any(windows, test))]
fn cached_fastfetch_script_path(
    cell: &Mutex<Option<CachedIntroFile>>,
    path: std::path::PathBuf,
    narrow: bool,
) -> Option<std::path::PathBuf> {
    cached_fastfetch_script_path_with(cell, path, narrow, |path, bytes| std::fs::write(path, bytes))
}

#[cfg(any(windows, test))]
fn cached_fastfetch_script_path_with(
    cell: &Mutex<Option<CachedIntroFile>>,
    path: std::path::PathBuf,
    narrow: bool,
    write_script: impl FnOnce(&std::path::Path, &[u8]) -> std::io::Result<()>,
) -> Option<std::path::PathBuf> {
    cached_intro_file(cell, path, || nebula_fastfetch_script_bytes(narrow), write_script)
}

/// 纯文本版欢迎内容，给 cmd 的 `type` 与 POSIX shell 的 `cat` 读。
///
/// 不带 BOM（`cat` 会把 BOM 原样打到屏幕上），换行用 CRLF：cmd 的 `type` 不
/// 做换行转换，单独一个 LF 只会让光标回到行首、覆盖上一行。
#[cfg(any(windows, test))]
fn nebula_fastfetch_text_bytes(narrow: bool) -> Vec<u8> {
    nebula_fastfetch_text(narrow)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
        .into_bytes()
}

#[cfg(any(windows, test))]
fn cached_fastfetch_text_path(narrow: bool) -> Option<std::path::PathBuf> {
    static WIDE: Mutex<Option<CachedIntroFile>> = Mutex::new(None);
    static NARROW: Mutex<Option<CachedIntroFile>> = Mutex::new(None);
    let (cell, file) = if narrow {
        (&NARROW, "nebula_fastfetch_intro_narrow.txt")
    } else {
        (&WIDE, "nebula_fastfetch_intro.txt")
    };
    cached_intro_file(
        cell,
        std::env::temp_dir().join(file),
        || nebula_fastfetch_text_bytes(narrow),
        |path, bytes| std::fs::write(path, bytes),
    )
}

/// 欢迎屏要发给哪一种 shell。
///
/// 只有把命令写对，欢迎屏才不依赖用户恰好用 PowerShell：cmd 认 `cls`/`type`，
/// POSIX shell 认 `printf`/`cat`，谁都不认识对方的 `Clear-Host; & '…ps1'`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IntroShell {
    PowerShell,
    Cmd,
    Posix,
}

impl IntroShell {
    /// 按 spawn 的可执行文件名判断（`cmd.exe` / `wsl.exe` / `bash.exe` …）。
    pub(crate) fn from_program(program: &str) -> Self {
        let name = program.rsplit(['\\', '/']).next().unwrap_or(program);
        let name = name.to_ascii_lowercase();
        let name = name.strip_suffix(".exe").unwrap_or(name.as_str());
        match name {
            "powershell" | "pwsh" => Self::PowerShell,
            "cmd" => Self::Cmd,
            "wsl" | "bash" | "zsh" | "sh" | "dash" | "fish" | "nu" | "nushell" | "ksh" | "ash"
            | "elvish" | "xonsh" => Self::Posix,
            // 未知 shell 维持既有行为：Windows 上按 PowerShell 注入。
            _ => {
                if cfg!(windows) {
                    Self::PowerShell
                } else {
                    Self::Posix
                }
            },
        }
    }
}

#[cfg(windows)]
fn nebula_fastfetch_script_path(narrow: bool) -> Option<std::path::PathBuf> {
    static WIDE: Mutex<Option<CachedIntroFile>> = Mutex::new(None);
    static NARROW: Mutex<Option<CachedIntroFile>> = Mutex::new(None);
    let (cell, file) = if narrow {
        (&NARROW, "nebula_fastfetch_intro_narrow.ps1")
    } else {
        (&WIDE, "nebula_fastfetch_intro.ps1")
    };
    cached_fastfetch_script_path(cell, std::env::temp_dir().join(file), narrow)
}

#[cfg(windows)]
fn powershell_single_quoted_path(path: &std::path::Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

/// cmd 的欢迎屏：切到 UTF-8 再打印纯文本，否则图标与框线按本地代码页解码成乱码。
#[cfg(windows)]
fn cmd_intro_command(path: &std::path::Path) -> String {
    format!("chcp 65001>nul & cls & type \"{}\"\r\n", path.display())
}

/// POSIX shell 的欢迎屏：装了 fastfetch 就让它出（与原行为一致），否则打印同一份
/// 欢迎卡；Windows 上先经 `wslpath`/`cygpath` 把路径翻译成来宾可读的形式。
#[cfg(windows)]
fn posix_intro_command(path: &std::path::Path) -> String {
    let quoted = path.to_string_lossy().replace('\'', "'\\''");
    format!(
        "printf '\\033[H\\033[2J'; if command -v fastfetch >/dev/null 2>&1; then fastfetch; else \
         p='{quoted}'; if command -v wslpath >/dev/null 2>&1; then p=$(wslpath -u \"$p\"); elif \
         command -v cygpath >/dev/null 2>&1; then p=$(cygpath -u \"$p\"); fi; cat \"$p\"; fi\n"
    )
}

/// Bytes to send to a freshly spawned shell so it prints the Nebula intro.
#[cfg(windows)]
pub(super) fn nebula_fastfetch_intro_command() -> Vec<u8> {
    nebula_intro_command_for(usize::MAX, IntroShell::PowerShell)
}

/// Width-aware intro: the two-column layout needs ~150 columns (62-col art +
/// info at column 65); anything narrower gets the stacked layout instead.
/// Also used to *re-print* the intro after a resize reflows a pristine pane.
/// `pub(crate)`：GPUI 壳的新会话欢迎屏走同一入口（fetch=1 设置）。
#[cfg(windows)]
pub(crate) fn nebula_intro_command_for(columns: usize, shell: IntroShell) -> Vec<u8> {
    let narrow = columns < 132;
    match shell {
        // 新 tab 必须秒出：所有系统信息在 Rust 侧一次性缓存；交给 PowerShell 的
        // 只有执行一个纯输出脚本，避免把 ANSI/Logo 当成用户输入逐行解析。
        IntroShell::PowerShell => match nebula_fastfetch_script_path(narrow) {
            Some(path) => {
                format!("Clear-Host; & {}\r", powershell_single_quoted_path(&path)).into_bytes()
            },
            None => b"Clear-Host\r".to_vec(),
        },
        IntroShell::Cmd => match cached_fastfetch_text_path(narrow) {
            Some(path) => cmd_intro_command(&path).into_bytes(),
            None => b"cls\r\n".to_vec(),
        },
        IntroShell::Posix => match cached_fastfetch_text_path(narrow) {
            Some(path) => posix_intro_command(&path).into_bytes(),
            None => b"printf '\\033[H\\033[2J'\n".to_vec(),
        },
    }
}

/// 旧壳（legacy-shell）按提示符主题挑欢迎屏，语义保持原样。
#[cfg(windows)]
pub(crate) fn nebula_fastfetch_intro_command_for(columns: usize, shell: NebulaShell) -> Vec<u8> {
    let shell = match shell {
        NebulaShell::PowerShell => IntroShell::PowerShell,
        NebulaShell::Bash => IntroShell::Posix,
    };
    nebula_intro_command_for(columns, shell)
}

/// Bytes to send to a freshly spawned shell so it prints the Nebula intro.
#[cfg(not(windows))]
pub(super) fn nebula_fastfetch_intro_command() -> Vec<u8> {
    b"clear; if command -v fastfetch >/dev/null 2>&1; then fastfetch; else printf '\\033[36mPebrel\\033[0m\\n'; uname -a; fi\n".to_vec()
}

/// Width-aware variant; the Unix intro delegates to fastfetch, which already
/// adapts to the terminal width, so `columns` and the shell family are unused.
#[cfg(not(windows))]
pub(crate) fn nebula_intro_command_for(_columns: usize, _shell: IntroShell) -> Vec<u8> {
    nebula_fastfetch_intro_command()
}

/// Width-aware variant kept for the legacy shell call sites.
#[cfg(not(windows))]
pub(crate) fn nebula_fastfetch_intro_command_for(
    columns: usize,
    _shell: crate::display::NebulaShell,
) -> Vec<u8> {
    nebula_intro_command_for(columns, IntroShell::Posix)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use super::*;

    #[test]
    fn deleted_scripts_are_recreated_for_both_layouts() {
        let root = tempfile::tempdir().unwrap();
        let mut layouts = Vec::new();
        for narrow in [false, true] {
            let cache = Default::default();
            let path = root.path().join(format!("intro-{narrow}.ps1"));
            assert_eq!(
                cached_fastfetch_script_path(&cache, path.clone(), narrow),
                Some(path.clone())
            );
            let original = fs::read(&path).unwrap();
            assert!(original.starts_with(&[0xEF, 0xBB, 0xBF]));
            let text = std::str::from_utf8(&original[3..]).unwrap();
            assert!(text.starts_with("$OutputEncoding = [System.Text.Encoding]::UTF8\r\n"));
            assert!(text.contains("[Console]::Write(@'\r\n"));
            assert!(text.ends_with("\r\n'@)\r\n"));
            fs::remove_file(&path).unwrap();
            assert_eq!(
                cached_fastfetch_script_path(&cache, path.clone(), narrow),
                Some(path.clone())
            );
            assert_eq!(fs::read(&path).unwrap(), original);
            layouts.push(original);
        }
        assert_ne!(layouts[0], layouts[1]);
    }

    #[test]
    fn failed_first_write_can_be_retried() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("scripts");
        fs::write(&parent, b"not a directory").unwrap();
        let path = parent.join("intro.ps1");
        let cache = Default::default();
        assert_eq!(cached_fastfetch_script_path(&cache, path.clone(), false), None);
        fs::remove_file(&parent).unwrap();
        fs::create_dir(&parent).unwrap();
        assert_eq!(cached_fastfetch_script_path(&cache, path.clone(), false), Some(path.clone()));
        assert!(fs::read(path).unwrap().starts_with(&[0xEF, 0xBB, 0xBF]));
    }

    #[test]
    fn first_request_replaces_a_previous_process_script() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("intro.ps1");
        fs::write(&path, b"old application script").unwrap();
        let cache = Default::default();
        assert_eq!(cached_fastfetch_script_path(&cache, path.clone(), false), Some(path.clone()));
        assert!(fs::read(path).unwrap().starts_with(&[0xEF, 0xBB, 0xBF]));
    }

    #[test]
    fn cached_request_keeps_an_existing_script() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("intro.ps1");
        let cache = Default::default();
        assert_eq!(cached_fastfetch_script_path(&cache, path.clone(), false), Some(path.clone()));
        let marker = b"existing cached script";
        fs::write(&path, marker).unwrap();
        assert_eq!(cached_fastfetch_script_path(&cache, path.clone(), false), Some(path.clone()));
        assert_eq!(fs::read(path).unwrap(), marker);
    }

    #[test]
    fn concurrent_requests_recreate_a_complete_cached_script() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("intro.ps1");
        let cache = Arc::new(Mutex::new(None));
        assert_eq!(cached_fastfetch_script_path(&cache, path.clone(), true), Some(path.clone()));
        let expected = Arc::new(fs::read(&path).unwrap());
        fs::remove_file(&path).unwrap();
        let barrier = Arc::new(Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let cache = Arc::clone(&cache);
                let path = path.clone();
                let expected = Arc::clone(&expected);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    assert_eq!(
                        cached_fastfetch_script_path(&cache, path.clone(), true),
                        Some(path.clone())
                    );
                    assert_eq!(fs::read(path).unwrap(), *expected);
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
    }

    #[test]
    fn failed_recreation_can_be_retried_without_losing_cached_bytes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("intro.ps1");
        let cache = Default::default();
        assert_eq!(cached_fastfetch_script_path(&cache, path.clone(), false), Some(path.clone()));
        let expected = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert_eq!(cached_fastfetch_script_path(&cache, path.clone(), false), None);
        fs::remove_dir(&path).unwrap();
        assert_eq!(cached_fastfetch_script_path(&cache, path.clone(), false), Some(path.clone()));
        assert_eq!(fs::read(path).unwrap(), expected);
    }

    #[test]
    fn intro_shell_is_detected_from_the_spawned_program() {
        for program in [
            "powershell",
            "powershell.exe",
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
            "pwsh.exe",
        ] {
            assert_eq!(IntroShell::from_program(program), IntroShell::PowerShell, "{program}");
        }
        for program in ["cmd", "cmd.exe", r"C:\Windows\system32\CMD.EXE"] {
            assert_eq!(IntroShell::from_program(program), IntroShell::Cmd, "{program}");
        }
        for program in
            ["wsl.exe", "bash", r"C:\Windows\system32\bash.exe", "/bin/zsh", "fish", "nu"]
        {
            assert_eq!(IntroShell::from_program(program), IntroShell::Posix, "{program}");
        }
    }

    /// cmd 与 POSIX shell 读的是同一份纯文本：不能带 BOM，换行必须是 CRLF
    /// （cmd 的 `type` 不做换行转换，只有 LF 会让每一行覆盖上一行）。
    #[test]
    fn cached_text_is_crlf_without_a_bom() {
        let bytes = nebula_fastfetch_text_bytes(false);
        assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        assert!(bytes.windows(2).any(|pair| pair == b"\r\n"));
        // 每个换行都必须是 CRLF：裸 LF（或裸 CR）会让 cmd 的 `type` 覆盖上一行。
        let bare: Vec<usize> = (0..bytes.len())
            .filter(|index| bytes[*index] == b'\n')
            .filter(|index| *index == 0 || bytes[*index - 1] != b'\r')
            .take(5)
            .collect();
        assert!(bare.is_empty(), "bare newlines at {bare:?}");
        let stray_carriage_returns = (0..bytes.len())
            .filter(|index| bytes[*index] == b'\r' && bytes.get(index + 1) != Some(&b'\n'));
        assert_eq!(stray_carriage_returns.count(), 0);
        assert!(std::str::from_utf8(&bytes).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn cmd_intro_switches_to_utf8_before_printing() {
        let command = cmd_intro_command(std::path::Path::new(r"C:\Temp\intro.txt"));
        assert_eq!(command, "chcp 65001>nul & cls & type \"C:\\Temp\\intro.txt\"\r\n");
    }

    #[cfg(windows)]
    #[test]
    fn posix_intro_prefers_fastfetch_and_can_translate_the_path() {
        let command = posix_intro_command(std::path::Path::new(r"C:\Temp\intro.txt"));
        assert!(command.starts_with("printf '\\033[H\\033[2J'; if command -v fastfetch"));
        assert!(command.contains("p='C:\\Temp\\intro.txt'"));
        assert!(command.contains("wslpath -u"));
        assert!(command.contains("cygpath -u"));
        assert!(command.ends_with("cat \"$p\"; fi\n"));
    }

    #[test]
    fn partial_write_failure_does_not_become_a_successful_cache_hit() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("intro.ps1");
        let cache = Default::default();
        assert_eq!(cached_fastfetch_script_path(&cache, path.clone(), false), Some(path.clone()));
        let expected = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        let result =
            cached_fastfetch_script_path_with(&cache, path.clone(), false, |path, bytes| {
                fs::write(path, &bytes[..7])?;
                Err(std::io::Error::other("synthetic partial write"))
            });
        assert_eq!(result, None);
        assert_eq!(fs::read(&path).unwrap(), &expected[..7]);
        assert_eq!(cached_fastfetch_script_path(&cache, path.clone(), false), Some(path.clone()));
        assert_eq!(fs::read(path).unwrap(), expected);
    }
}
