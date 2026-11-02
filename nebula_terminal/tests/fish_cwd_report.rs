//! End-to-end evidence that a nested Cygwin fish's OSC 7 cwd reports survive
//! the real ConPTY pipeline (EventLoop + StreamProcessor, the product path):
//! default pwsh (with the PTY-layer integration) → type `fish` → cd.
//! Skipped on machines without Cygwin fish.
#![cfg(windows)]

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use nebula_terminal::event::{Event, EventListener, Notify, WindowSize};
use nebula_terminal::event_loop::{EventLoop, EventLoopSender, Notifier};
use nebula_terminal::grid::Dimensions as _;
use nebula_terminal::index::{Column, Line, Point as TermPoint};
use nebula_terminal::sync::FairMutex;
use nebula_terminal::term::{Config, Term};
use nebula_terminal::tty::{self, Options};

const SIZE: WindowSize = WindowSize { num_lines: 24, num_cols: 80, cell_width: 8, cell_height: 16 };

const FISH: &str = r"C:\cygwin64\bin\fish.exe";

/// 收集全部终端事件；测试只关心 `CwdReport` 是否到达。
#[derive(Clone)]
struct Collector(mpsc::Sender<Event>);

impl EventListener for Collector {
    fn send_event(&self, event: Event) {
        self.0.send(event).ok();
    }
}

struct Fixture {
    term: Arc<FairMutex<Term<Collector>>>,
    notifier: Notifier,
    rx: mpsc::Receiver<Event>,
}

fn spawn_default_powershell() -> Fixture {
    tty::setup_env();
    let mut options = Options::default();
    // 复刻应用 local_options 的子进程环境注入（platform::environment 的
    // 能力通告 + 身份契约）：这里少一个变量（历史上是缺 VTE_VERSION），
    // 嵌套 fish 就会停发 OSC 7，本测试即红。
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
    Fixture { term, notifier: Notifier(sender), rx }
}

impl Fixture {
    fn send_line(&self, line: &str) {
        self.notifier.notify(format!("{line}\r").into_bytes());
    }

    fn screen(&self) -> String {
        let term = self.term.lock();
        let (lines, columns) = (term.grid().total_lines(), term.grid().columns());
        let _ = &term;
        if lines == 0 || columns == 0 {
            return String::new();
        }
        term.bounds_to_string(
            TermPoint::new(Line(0), Column(0)),
            TermPoint::new(Line(lines as i32 - 1), Column(columns - 1)),
        )
    }

    /// 等一个条件（读事件流直到谓词命中或超时）；谓词可消费已见事件。
    fn wait_until(&self, timeout: Duration, what: &str, probe: impl Fn(&Event) -> bool) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match self.rx.recv_timeout(Duration::from_millis(200)) {
                Ok(event) => {
                    eprintln!("event: {event:?}");
                    if probe(&event) {
                        return;
                    }
                },
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
            }
        }
        panic!("timed out waiting for {what}; screen:\n{}", self.screen());
    }
}

#[test]
fn nested_fish_cwd_reports_flow_through_the_product_pipeline() {
    if !Path::new(FISH).exists() {
        eprintln!("skipping: {FISH} is not installed");
        return;
    }
    let fixture = spawn_default_powershell();
    // 默认 pwsh（带 PTY 层集成）就绪：NEBULA| 标题或首个提示符。
    fixture.wait_until(
        Duration::from_secs(20),
        "the default prompt",
        |event| matches!(event, Event::Title(title) if title.starts_with("NEBULA|")),
    );
    // 键入 fish（真实字节进 PTY）。
    fixture.send_line("fish");
    fixture.wait_until(
        Duration::from_secs(20),
        "fish's startup OSC 7",
        |event| matches!(event, Event::CwdReport(cwd) if cwd.starts_with("/cygdrive/")),
    );
    // fish 提示符内 cd：下一个提示符的 OSC 7 必须带回新目录。
    fixture.send_line("cd /cygdrive/d");
    fixture.wait_until(
        Duration::from_secs(20),
        "the cd report",
        |event| matches!(event, Event::CwdReport(cwd) if cwd == "/cygdrive/d"),
    );
}
