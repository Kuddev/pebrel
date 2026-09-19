//! Edit real Git Bash input through the product PTY and VT parser.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

use crate::Term;
use crate::event::{Event, EventListener, WindowSize};
use crate::index::{Column, Line};
use crate::term::{Config, test::TermSize};
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
}

impl BashSession {
    fn send(&mut self, bytes: &[u8]) {
        write_pty(&mut self.pty, bytes);
    }

    fn wait_for(&mut self, label: &str, expected: impl Fn(&Term<Replies>) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut buffer = [0; 8192];
        loop {
            match self.pty.reader().read(&mut buffer) {
                Ok(count) if count > 0 => {
                    self.parser.advance(&mut self.term, &buffer[..count]);
                    for reply in self.replies.0.lock().unwrap().drain(..) {
                        write_pty(&mut self.pty, reply.as_bytes());
                    }
                    if expected(&self.term) {
                        return;
                    }
                },
                Ok(_) => (),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => (),
                Err(error) => panic!("{label}: PTY read failed: {error}"),
            }
            assert!(
                Instant::now() < deadline,
                "{label}: cursor {:?}, rows {:?} / {:?}",
                self.term.grid().cursor.point,
                row(&self.term, 0),
                row(&self.term, 1)
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
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
            };
            session.wait_for(locale, |term| blank_input(term, mark));
            session.send(b"relay/package.json");
            session.wait_for("short input", |term| {
                row(term, 0).trim_end() == format!("{mark} relay/package.json")
                    && term.grid().cursor.point.column == Column(20)
            });
            session.send(&[127; 18]);
            session.wait_for("short backspace", |term| blank_input(term, mark));

            let long = b"relay/package.jsonrelay/package.jsonrelay/package.jsonrelay/package.json";
            for erase in [vec![127; long.len()], vec![21]] {
                session.send(long);
                session.wait_for("wrapped input", |term| {
                    term.grid().cursor.point.line == Line(1)
                        && term.grid().cursor.point.column == Column(34)
                });
                session.send(&[1]);
                session.wait_for("wrapped Home", |term| {
                    term.grid().cursor.point.line == Line(0)
                        && term.grid().cursor.point.column == Column(2)
                });
                session.send(&[5]);
                session.wait_for("wrapped End", |term| {
                    term.grid().cursor.point.line == Line(1)
                        && term.grid().cursor.point.column == Column(34)
                });
                session.send(&erase);
                session.wait_for("erase wrapped input", |term| blank_input(term, mark));
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
