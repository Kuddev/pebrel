//! End-to-end evidence that the Nebula-integrated Git Bash reports its cwd
//! through the `NEBULA|` title in the **host drive form** (`C:\Windows`), not
//! the MSYS `/c/Windows` form: new-tab inheritance (`TerminalView::local_cwd`)
//! and sidebar grouping both parse that field on the host side, where a POSIX
//! string is treated as a guest path (user 10-11). Skipped without Git Bash.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use nebula_terminal::event::{Event, EventListener, Notify, WindowSize};
use nebula_terminal::event_loop::{EventLoop, EventLoopSender, Notifier};
use nebula_terminal::sync::FairMutex;
use nebula_terminal::term::{Config, Term};
use nebula_terminal::tty::{self, Options};

const SIZE: WindowSize = WindowSize { num_lines: 24, num_cols: 80, cell_width: 8, cell_height: 16 };

/// 同产品层 `nebula_find_bash` 的候选序：显式覆盖 → 常见安装位 → PATH。
fn find_git_bash() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("NEBULA_BASH").map(PathBuf::from)
        && path.is_file()
    {
        return Some(path);
    }
    [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
        r"C:\msys64\usr\bin\bash.exe",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
    .or_else(|| {
        std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path).map(|dir| dir.join("bash.exe")).find(|p| p.is_file())
        })
    })
}

/// 收集全部终端事件；测试只关心 `NEBULA|` 标题的 cwd 字段。
#[derive(Clone)]
struct Collector(mpsc::Sender<Event>);

impl EventListener for Collector {
    fn send_event(&self, event: Event) {
        self.0.send(event).ok();
    }
}

fn spawn_integrated_git_bash(bash: &Path) -> (Notifier, mpsc::Receiver<Event>) {
    tty::setup_env();
    let mut options = Options {
        // 产品同款集成注入：`--rcfile <pebrel_bashrc> -i`。这里 rc 少发盘符形态
        // 的标题 cwd（历史上是 MSYS `/d/…`），本测试即红。
        shell: Some(tty::bash_with_nebula_integration(bash.display().to_string(), Vec::new())),
        ..Default::default()
    };
    for (name, value) in [
        ("TERM", "xterm-256color"),
        ("COLORTERM", "truecolor"),
        ("TERM_PROGRAM", "pebrel"),
        ("VTE_VERSION", "6003"),
    ] {
        options.env.insert(name.to_owned(), value.to_owned());
    }
    let (tx, rx) = mpsc::channel::<Event>();
    let term = Arc::new(FairMutex::new(Term::new(Config::default(), &SIZE, Collector(tx.clone()))));
    let pty = tty::new(&options, SIZE, 0).expect("create native ConPTY");
    let sender: EventLoopSender = {
        let mut event_loop =
            EventLoop::new(Arc::clone(&term), Collector(tx), pty, false, false).expect("loop");
        let sender = event_loop.channel();
        event_loop.spawn();
        sender
    };
    (Notifier(sender), rx)
}

/// `NEBULA|` 标题的 cwd 字段（首个 `|` 之前）。
fn title_cwd(title: &str) -> Option<&str> {
    title.strip_prefix("NEBULA|").and_then(|rest| rest.split('|').next())
}

fn is_host_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\'
}

/// 读事件流直到某个 `NEBULA|` 标题的 cwd 字段命中谓词或超时。
fn wait_for_title(
    rx: &mpsc::Receiver<Event>,
    timeout: Duration,
    what: &str,
    probe: impl Fn(&str) -> bool,
) -> String {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(event) => {
                eprintln!("event: {event:?}");
                if let Event::Title(title) = &event
                    && let Some(cwd) = title_cwd(title)
                    && probe(cwd)
                {
                    return cwd.to_owned();
                }
            },
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
        }
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn integrated_git_bash_reports_host_drive_cwds_through_the_title_channel() {
    let Some(bash) = find_git_bash() else {
        eprintln!("skipping: Git Bash is not installed");
        return;
    };
    let (notifier, rx) = spawn_integrated_git_bash(&bash);
    // 首个提示符：标题 cwd 必须已是宿主盘符形态，不能是 MSYS 的 `/d/…`。
    let first = wait_for_title(&rx, Duration::from_secs(20), "the first drive-form title", |cwd| {
        is_host_drive_path(cwd) && !cwd.starts_with('/')
    });
    assert!(is_host_drive_path(&first), "first title cwd: {first:?}");
    // cd 到 SystemRoot（任意 Windows 机器都有）：标题必须精确回报该目录。
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
    let root = system_root.trim_end_matches('\\').to_owned();
    let (drive, rest) = root.split_at(2);
    let msys = format!("/{}/{}", drive[..1].to_ascii_lowercase(), rest.replace('\\', "/"));
    notifier.notify(format!("cd {msys}\r").into_bytes());
    wait_for_title(&rx, Duration::from_secs(20), "the cd title", |cwd| cwd == root);
}
