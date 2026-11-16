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

#[cfg(any(windows, test))]
struct CachedIntroScript {
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
fn cached_fastfetch_script_path(
    cell: &Mutex<Option<CachedIntroScript>>,
    path: std::path::PathBuf,
    narrow: bool,
) -> Option<std::path::PathBuf> {
    cached_fastfetch_script_path_with(cell, path, narrow, |path, bytes| std::fs::write(path, bytes))
}

#[cfg(any(windows, test))]
fn cached_fastfetch_script_path_with(
    cell: &Mutex<Option<CachedIntroScript>>,
    path: std::path::PathBuf,
    narrow: bool,
    write_script: impl FnOnce(&std::path::Path, &[u8]) -> std::io::Result<()>,
) -> Option<std::path::PathBuf> {
    // Keep the bytes, not the lifetime of a file in the system temp directory.
    // Serialize recreation so callers in this process cannot read a partial write.
    let mut cached = cell.lock().ok()?;
    let script = cached.get_or_insert_with(|| CachedIntroScript {
        bytes: nebula_fastfetch_script_bytes(narrow),
        needs_write: true,
    });
    if script.needs_write || !path.is_file() {
        // Initial requests replace files left by older processes. Keep this flag
        // set after an error, including a failed write that leaves a partial file.
        script.needs_write = true;
        write_script(&path, &script.bytes).ok()?;
        script.needs_write = false;
    }
    Some(path)
}

#[cfg(windows)]
fn nebula_fastfetch_script_path(narrow: bool) -> Option<std::path::PathBuf> {
    static WIDE: Mutex<Option<CachedIntroScript>> = Mutex::new(None);
    static NARROW: Mutex<Option<CachedIntroScript>> = Mutex::new(None);
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

// The interactive shell may be fish (including when launched through WSL).
// Keep the outer command portable and let sh interpret the POSIX script.
fn unix_intro_command() -> Vec<u8> {
    b"sh -c 'clear; if command -v fastfetch >/dev/null 2>&1; then fastfetch; else printf \"\\033[36mPebrel\\033[0m\\n\"; uname -a; fi'\n".to_vec()
}

/// Bytes to send to a freshly spawned shell so it prints the Nebula intro.
#[cfg(windows)]
pub(super) fn nebula_fastfetch_intro_command() -> Vec<u8> {
    nebula_fastfetch_intro_command_for(usize::MAX, NebulaShell::PowerShell)
}

/// Width-aware intro: the two-column layout needs ~150 columns (62-col art +
/// info at column 65); anything narrower gets the stacked layout instead.
/// Also used to *re-print* the intro after a resize reflows a pristine pane.
/// `pub(crate)`：GPUI 壳的新会话欢迎屏走同一入口（fetch=1 设置）。
#[cfg(windows)]
pub(crate) fn nebula_fastfetch_intro_command_for(columns: usize, shell: NebulaShell) -> Vec<u8> {
    if shell == NebulaShell::Bash {
        return unix_intro_command();
    }

    // 新 tab 必须秒出：所有系统信息在 Rust 侧一次性缓存；交给 PowerShell 的
    // 只有执行一个纯输出脚本，避免把 ANSI/Logo 当成用户输入逐行解析。
    let narrow = columns < 132;
    match nebula_fastfetch_script_path(narrow) {
        Some(path) => {
            format!("Clear-Host; & {}\r", powershell_single_quoted_path(&path)).into_bytes()
        },
        None => b"Clear-Host\r".to_vec(),
    }
}

/// Bytes to send to a freshly spawned shell so it prints the Nebula intro.
#[cfg(not(windows))]
pub(super) fn nebula_fastfetch_intro_command() -> Vec<u8> {
    unix_intro_command()
}

/// Width-aware variant; the Unix intro delegates to fastfetch, which already
/// adapts to the terminal width, so `columns` is unused there.
#[cfg(not(windows))]
pub(crate) fn nebula_fastfetch_intro_command_for(
    _columns: usize,
    _shell: crate::display::NebulaShell,
) -> Vec<u8> {
    nebula_fastfetch_intro_command()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use super::*;

    #[cfg(unix)]
    #[test]
    fn unix_intro_runs_in_fish_and_posix_shells_with_or_without_fastfetch() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        use std::process::Command;

        let root = tempfile::tempdir().unwrap();
        symlink("/bin/sh", root.path().join("sh")).unwrap();
        for (name, body) in [("clear", "exit 0"), ("uname", "echo FALLBACK")] {
            let path = root.path().join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let command = String::from_utf8(unix_intro_command()).unwrap();
        let search_path = std::env::var_os("PATH").unwrap();
        for shell in ["sh", "bash", "fish"] {
            let Some(executable) = std::env::split_paths(&search_path)
                .map(|directory| directory.join(shell))
                .find(|path| path.is_file())
            else {
                assert_ne!(shell, "sh", "sh is required for the welcome command");
                continue;
            };
            for fastfetch in [false, true] {
                let fastfetch_path = root.path().join("fastfetch");
                if fastfetch {
                    fs::write(&fastfetch_path, "#!/bin/sh\necho FASTFETCH\n").unwrap();
                    fs::set_permissions(&fastfetch_path, fs::Permissions::from_mode(0o755))
                        .unwrap();
                } else if fastfetch_path.exists() {
                    fs::remove_file(&fastfetch_path).unwrap();
                }
                let output = Command::new(&executable)
                    .args(["-c", &command])
                    .env("PATH", root.path())
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{shell}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let stdout = String::from_utf8(output.stdout).unwrap();
                if fastfetch {
                    assert_eq!(stdout, "FASTFETCH\n");
                } else {
                    assert!(
                        stdout.contains("Pebrel") && stdout.contains("FALLBACK"),
                        "{shell}: {stdout:?}"
                    );
                }
            }
        }
    }

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
