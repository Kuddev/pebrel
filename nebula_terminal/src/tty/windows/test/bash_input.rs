//! Edit real Git Bash input through the product PTY and VT parser.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

use crate::Term;
use crate::event::{Event, EventListener, WindowSize};
use crate::index::{Column, Line};
use crate::term::{Config, TermMode, test::TermSize};
use crate::tty::{self, EventedReadWrite};
use vte::ansi;

#[derive(Clone, Default)]
struct Replies(Arc<Mutex<Vec<String>>>);

impl EventListener for Replies {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(data) = event {
            self.0.lock().unwrap().push(data);
        }
    }
}

struct BashSession {
    pty: tty::Pty,
    term: Term<Replies>,
    parser: ansi::Processor,
    replies: Replies,
    locale: &'static str,
    last_input: Vec<u8>,
    last_reply: Option<String>,
    output_tail: VecDeque<u8>,
}

impl BashSession {
    fn send(&mut self, bytes: &[u8]) {
        self.last_input = bytes.to_vec();
        write_pty(&mut self.pty, bytes);
    }

    fn edit(&mut self, key: EditKey, count: usize) {
        let (virtual_key, character, modifiers, legacy) = match key {
            EditKey::Backspace => (0x08, 0x08, 0, 0x7f),
            EditKey::Home => (0x41, 0x01, 0x08, 0x01), // Ctrl+A
            EditKey::End => (0x45, 0x05, 0x08, 0x05),  // Ctrl+E
            EditKey::KillLine => (0x55, 0x15, 0x08, 0x15), // Ctrl+U
            EditKey::Redraw => (0x4c, 0x0c, 0x08, 0x0c), // Ctrl+L
        };
        if self.term.mode().contains(TermMode::WIN32_INPUT_MODE) {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
                MAPVK_VK_TO_VSC, MapVirtualKeyW,
            };

            // Match the product's negotiated special-key path, including key-up.
            // The GPUI and runtime encoders obtain the scan code from Windows.
            // Keep repeated presses in one write to retain burst coverage.
            let scan_code = unsafe { MapVirtualKeyW(virtual_key, MAPVK_VK_TO_VSC) };
            let press = format!(
                "\x1b[{virtual_key};{scan_code};{character};1;{modifiers};1_\x1b[{virtual_key};{scan_code};{character};0;{modifiers};1_"
            );
            self.send(press.repeat(count).as_bytes());
        } else {
            self.send(&vec![legacy; count]);
        }
    }

    fn receive(&mut self, bytes: &[u8]) {
        self.output_tail.extend(bytes);
        while self.output_tail.len() > 1024 {
            self.output_tail.pop_front();
        }
        self.parser.advance(&mut self.term, bytes);
        for reply in self.replies.0.lock().unwrap().drain(..) {
            self.last_reply = Some(reply.clone());
            write_pty(&mut self.pty, reply.as_bytes());
        }
    }

    fn diagnostic_snapshot(&self) -> String {
        format!(
            "cursor {:?}, rows {:?} / {:?}; last input {:?}, last reply {:?}, output tail {:?}",
            self.term.grid().cursor.point,
            row(&self.term, 0),
            row(&self.term, 1),
            String::from_utf8_lossy(&self.last_input),
            self.last_reply,
            String::from_utf8_lossy(&self.output_tail.iter().copied().collect::<Vec<_>>()),
        )
    }

    fn redraw_after_failure(&mut self) -> std::io::Result<()> {
        self.output_tail.clear();
        self.last_reply = None;
        // A single redraw distinguishes stale display from retained Readline
        // input without deleting any more characters or retrying the assertion.
        self.edit(EditKey::Redraw, 1);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut buffer = [0; 8192];
        while Instant::now() < deadline {
            match self.pty.reader().read(&mut buffer) {
                Ok(count) if count > 0 => self.receive(&buffer[..count]),
                Ok(_) => (),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(error) => return Err(error),
            }
            thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }

    fn wait_for(&mut self, label: &str, expected: impl Fn(&Term<Replies>) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut buffer = [0; 8192];
        loop {
            match self.pty.reader().read(&mut buffer) {
                Ok(count) if count > 0 => {
                    self.receive(&buffer[..count]);
                    if expected(&self.term) {
                        return;
                    }
                },
                Ok(_) => (),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(error) => panic!("{label}: PTY read failed: {error}"),
            }
            let before_deadline = Instant::now() < deadline;
            if !before_deadline {
                let original = self.diagnostic_snapshot();
                // Even a diagnostic I/O failure must retain the original
                // assertion and its screen/input/output evidence.
                let redraw = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    self.redraw_after_failure()
                }));
                let redraw_result = match redraw {
                    Ok(Ok(())) => "captured for 2 seconds".to_owned(),
                    Ok(Err(error)) => format!("PTY read failed: {error}"),
                    Err(_) => "diagnostic panicked".to_owned(),
                };
                assert!(
                    before_deadline,
                    "{} / {label}: original failure: {original}; Ctrl-L redraw ({redraw_result}): {}",
                    self.locale,
                    self.diagnostic_snapshot(),
                );
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}

#[derive(Clone, Copy)]
enum EditKey {
    Backspace,
    Home,
    End,
    KillLine,
    Redraw,
}

fn write_pty(pty: &mut tty::Pty, mut bytes: &[u8]) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !bytes.is_empty() {
        match pty.writer().write(bytes) {
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => (),
            Err(error) => panic!("PTY write failed: {error}"),
        }
        assert!(Instant::now() < deadline, "PTY input remained blocked");
        if !bytes.is_empty() {
            thread::sleep(Duration::from_millis(5));
        }
    }
}

fn row(term: &Term<Replies>, line: i32) -> String {
    (0..40).map(|column| term.grid()[Line(line)][Column(column)].c).collect()
}

fn blank_input(term: &Term<Replies>, mark: char) -> bool {
    term.grid().cursor.point.line == Line(0)
        && term.grid().cursor.point.column == Column(2)
        && row(term, 0).trim_end() == mark.to_string()
        && row(term, 1).trim().is_empty()
}

#[test]
fn git_bash_wrapped_input_can_be_fully_erased_in_byte_and_utf8_locales() {
    let Some(program) = super::nebula_find_bash() else {
        assert!(env::var_os("CI").is_none(), "native Windows CI requires Git Bash");
        eprintln!("Git Bash is not installed; native Windows CI runs this regression");
        return;
    };
    let directory = env::temp_dir().join(format!("pebrel-bash-edit-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("pebrel_settings.txt"), "powerline=1\n").unwrap();
    let outcome = std::panic::catch_unwind(|| {
        for (locale, mark) in [("C", '>'), ("C.UTF-8", '❯')] {
            let mut environment = HashMap::from([
                ("LC_ALL".into(), locale.into()),
                ("TERM".into(), "xterm-256color".into()),
                ("NEBULA_BASHRC_SOURCED".into(), "1".into()),
                ("INPUTRC".into(), "/dev/null".into()),
                ("HISTFILE".into(), "/dev/null".into()),
            ]);
            for key in ["PEBREL_CONFIG_DIR", "NEBULA_CONFIG_DIR"] {
                environment.insert(key.into(), directory.to_string_lossy().into_owned());
            }
            let options = tty::Options {
                shell: Some(tty::bash_with_nebula_integration(program.clone(), Vec::new())),
                working_directory: Some(directory.clone()),
                env: environment,
                ..Default::default()
            };
            let pty = tty::new(
                &options,
                WindowSize { num_lines: 24, num_cols: 40, cell_width: 8, cell_height: 16 },
                0,
            )
            .expect("create actual Git Bash PTY");
            let replies = Replies::default();
            // Match the application: OpenConsole's first DA1 was pre-answered
            // by the PTY bootstrap, so replying again would type into Bash.
            let config = Config {
                suppress_bringup_da1: tty::conpty_sideload_enabled(),
                conpty_resize: true,
                ..Default::default()
            };
            let mut session = BashSession {
                pty,
                term: Term::new(config, &TermSize::new(40, 24), replies.clone()),
                parser: ansi::Processor::new(),
                replies,
                locale,
                last_input: Vec::new(),
                last_reply: None,
                output_tail: VecDeque::new(),
            };
            session.wait_for(locale, |term| blank_input(term, mark));
            session.send(b"relay/package.json");
            session.wait_for("short input", |term| {
                row(term, 0).trim_end() == format!("{mark} relay/package.json")
                    && term.grid().cursor.point.column == Column(20)
            });
            session.edit(EditKey::Backspace, 18);
            session.wait_for("short backspace", |term| blank_input(term, mark));

            let long = b"relay/package.jsonrelay/package.jsonrelay/package.jsonrelay/package.json";
            let text = std::str::from_utf8(long).unwrap();
            let content_matches = |term: &Term<Replies>| {
                row(term, 0) == format!("{mark} {}", &text[..38])
                    && row(term, 1).trim_end() == &text[38..]
            };
            for (erase_name, erase, count) in
                [("backspace", EditKey::Backspace, long.len()), ("Ctrl-U", EditKey::KillLine, 1)]
            {
                session.send(long);
                // 光标位置可能先于一次重绘的全部字符到达；等文本也完整后再发送下一项编辑。
                session.wait_for(&format!("{erase_name}: wrapped input"), |term| {
                    content_matches(term)
                        && term.grid().cursor.point.line == Line(1)
                        && term.grid().cursor.point.column == Column(34)
                });
                session.edit(EditKey::Home, 1);
                session.wait_for(&format!("{erase_name}: wrapped Home"), |term| {
                    content_matches(term)
                        && term.grid().cursor.point.line == Line(0)
                        && term.grid().cursor.point.column == Column(2)
                });
                session.edit(EditKey::End, 1);
                session.wait_for(&format!("{erase_name}: wrapped End"), |term| {
                    content_matches(term)
                        && term.grid().cursor.point.line == Line(1)
                        && term.grid().cursor.point.column == Column(34)
                });
                session.edit(erase, count);
                session.wait_for(&format!("{erase_name}: erase wrapped input"), |term| {
                    blank_input(term, mark)
                });
            }
            // No probe command or user history is executed; dropping owns this PTY.
            session.send(b"exit\r");
        }
    });
    // The closed host can briefly retain Bash's working directory handle.
    let deadline = Instant::now() + Duration::from_secs(5);
    while let Err(error) = fs::remove_dir_all(&directory) {
        if Instant::now() < deadline
            && (error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(32))
        {
            thread::sleep(Duration::from_millis(25));
        } else {
            panic!("cannot remove probe directory {}: {error}", directory.display());
        }
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn prompt_width_tracks_locale_changes_without_overriding_user_configuration() {
    super::run_bash_integration_case(
        "NEBULA_BASHRC_SOURCED=1\nHOME=/__pebrel_missing_home__\nPROMPT_COMMAND=",
        r#"
for mode in 0 1; do
    __nebula_setting() { printf '%s' "$mode"; }
    export LC_ALL=C
    __nebula_precmd >/dev/null
    [[ $PS1 == *'> '* && $PS1 != *'❯'* && $LC_ALL == C ]] || exit 61
    export LC_ALL=C.UTF-8
    __nebula_precmd >/dev/null
    [[ $PS1 == *'❯ '* && $LC_ALL == C.UTF-8 ]] || exit 62
done
export LC_ALL=C
__nebula_user_ps1=1
PS1='user prompt> '
__nebula_precmd >/dev/null
[[ $PS1 == 'user prompt> ' && $LC_ALL == C ]] || exit 63
"#,
    );
}
