//! TTY related functionality.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::{Arc, LazyLock};
use std::{env, io};

use polling::{Event, PollMode, Poller};

#[cfg(not(windows))]
mod unix;
#[cfg(not(windows))]
pub use self::unix::*;

#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
pub use self::windows::*;

/// Shared SSH/WSL execution reports for local and bootstrapped bash/zsh shells.
pub fn connection_shell() -> &'static str {
    static SCRIPT: LazyLock<Cow<'static, str>> = LazyLock::new(|| {
        Cow::Owned(format!(
            "{}\n{}\n{}\n{}",
            shell_line_endings(include_str!("connection.sh")),
            shell_line_endings(include_str!("completion.sh")),
            shell_line_endings(include_str!("pointer.sh")),
            shell_line_endings(include_str!("command.sh"))
        ))
    });
    &SCRIPT
}

/// Native PSReadLine callbacks shared by the Unix and Windows shell bootstraps.
pub fn powershell_editor() -> String {
    format!("{}\n{}", include_str!("completion.ps1"), include_str!("pointer.ps1"))
}

/// Script text as a POSIX shell must see it: CRLF checkout line endings become LF.
pub fn shell_line_endings(script: &str) -> Cow<'_, str> {
    // include_str! preserves checkout bytes. POSIX shells treat CR in a CRLF
    // checkout as syntax, including when this script travels via PROMPT_COMMAND.
    if script.contains("\r\n") {
        Cow::Owned(script.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(script)
    }
}

/// Configuration for the `Pty` interface.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Options {
    /// Shell options.
    ///
    /// [`None`] will use the default shell.
    pub shell: Option<Shell>,

    /// Shell startup directory.
    pub working_directory: Option<PathBuf>,

    /// Drain the child process output before exiting the terminal.
    pub drain_on_exit: bool,

    /// Extra environment variables.
    pub env: HashMap<String, String>,

    /// 此环境表是完整子进程环境，不是需要与当前进程环境合并的增量覆盖。
    #[cfg(target_os = "windows")]
    pub env_is_complete: bool,

    /// Specifies whether the Windows shell arguments should be escaped.
    ///
    /// - When `true`: Arguments will be escaped according to the standard C runtime rules.
    /// - When `false`: Arguments will be passed raw without additional escaping.
    #[cfg(target_os = "windows")]
    pub escape_args: bool,
}

/// Shell options.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Shell {
    /// Path to a shell program to run on startup.
    pub(crate) program: String,
    /// Arguments passed to shell.
    pub(crate) args: Vec<String>,
}

impl Shell {
    pub fn new(program: String, args: Vec<String>) -> Self {
        Self { program, args }
    }

    /// Path to the shell program (session persistence reads it back).
    pub fn program(&self) -> &str {
        &self.program
    }

    /// Arguments passed to the shell.
    pub fn args(&self) -> &[String] {
        &self.args
    }
}

/// Stream read and/or write behavior.
///
/// This defines an abstraction over polling's interface in order to allow either
/// one read/write object or a separate read and write object.
pub trait EventedReadWrite {
    type Reader: io::Read;
    type Writer: io::Write;

    /// # Safety
    ///
    /// The underlying sources must outlive their registration in the `Poller`.
    unsafe fn register(&mut self, _: &Arc<Poller>, _: Event, _: PollMode) -> io::Result<()>;
    fn reregister(&mut self, _: &Arc<Poller>, _: Event, _: PollMode) -> io::Result<()>;
    fn deregister(&mut self, _: &Arc<Poller>) -> io::Result<()>;

    fn reader(&mut self) -> &mut Self::Reader;
    fn writer(&mut self) -> &mut Self::Writer;
}

/// Events concerning TTY child processes.
#[derive(Debug, PartialEq, Eq)]
pub enum ChildEvent {
    /// Indicates the child has exited.
    Exited(Option<ExitStatus>),
}

/// A pseudoterminal (or PTY).
///
/// This is a refinement of EventedReadWrite that also provides a channel through which we can be
/// notified if the PTY child process does something we care about (other than writing to the TTY).
/// In particular, this allows for race-free child exit notification on UNIX (cf. `SIGCHLD`).
pub trait EventedPty: EventedReadWrite {
    /// Tries to retrieve an event.
    ///
    /// Returns `Some(event)` on success, or `None` if there are no events to retrieve.
    fn next_child_event(&mut self) -> Option<ChildEvent>;

    /// PID of the direct child attached to this PTY, when the backend knows
    /// it. Windows uses it for the ConPTY cursor-realign probe
    /// (`AttachConsole` + `GetConsoleScreenBufferInfo`); other backends have
    /// no equivalent and keep the default.
    fn child_pid(&self) -> Option<u32> {
        None
    }
}

/// Setup environment variables.
pub fn setup_env() {
    // Prefer a 'nebula' terminfo entry if the user has installed one,
    // otherwise fall back to the universally supported 'xterm-256color'.
    // May be overridden by the user's config below.
    let terminfo = if terminfo_exists("nebula") { "nebula" } else { "xterm-256color" };
    unsafe { env::set_var("TERM", terminfo) };

    // Advertise 24-bit color support.
    unsafe { env::set_var("COLORTERM", "truecolor") };
}

/// Check if a terminfo entry exists on the system.
fn terminfo_exists(terminfo: &str) -> bool {
    // Get first terminfo character for the parent directory.
    let first = terminfo.get(..1).unwrap_or_default();
    let first_hex = format!("{:x}", first.chars().next().unwrap_or_default() as usize);

    // Return true if the terminfo file exists at the specified location.
    macro_rules! check_path {
        ($path:expr) => {
            if $path.join(first).join(terminfo).exists()
                || $path.join(&first_hex).join(terminfo).exists()
            {
                return true;
            }
        };
    }

    if let Some(dir) = env::var_os("TERMINFO") {
        check_path!(PathBuf::from(&dir));
    } else if let Some(home) = home::home_dir() {
        check_path!(home.join(".terminfo"));
    }

    if let Ok(dirs) = env::var("TERMINFO_DIRS") {
        for dir in dirs.split(':') {
            check_path!(PathBuf::from(dir));
        }
    }

    if let Ok(prefix) = env::var("PREFIX") {
        let path = PathBuf::from(prefix);
        check_path!(path.join("etc/terminfo"));
        check_path!(path.join("lib/terminfo"));
        check_path!(path.join("share/terminfo"));
    }

    check_path!(PathBuf::from("/etc/terminfo"));
    check_path!(PathBuf::from("/lib/terminfo"));
    check_path!(PathBuf::from("/usr/share/terminfo"));
    check_path!(PathBuf::from("/boot/system/data/terminfo"));

    // No valid terminfo path has been found.
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_script_survives_crlf_and_mixed_checkouts() {
        let source = connection_shell();
        assert!(!source.contains('\r'));
        assert!(matches!(shell_line_endings(source), Cow::Borrowed(_)));
        for input in [source.replace('\n', "\r\n"), source.replacen('\n', "\r\n", 4)] {
            assert_eq!(shell_line_endings(&input), source);
        }
        // Only line endings change; embedded/escaped carriage returns are data.
        assert_eq!(shell_line_endings("printf '\\r\\n'\r\n# a\rb\n"), "printf '\\r\\n'\n# a\rb\n");
    }

    #[test]
    #[cfg(unix)]
    fn command_report_never_reuses_ignored_history_or_evaluates_a_pipeline() {
        use base64::Engine as _;
        use std::io::Write as _;
        use std::process::{Command, Stdio};
        let mut child = Command::new("bash")
            .args(["--noprofile", "--norc", "-i"])
            .env("HISTFILE", "/dev/null")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start isolated Bash");
        let script = format!(
            "HISTCONTROL=ignoreboth\nPS1=''\nprintf '%s %s\\n' \"${{BASH_VERSINFO[0]}}\" \"${{BASH_VERSINFO[1]}}\"\n__pebrel_shell_token=YmFzaDox\n{}\nprintf 'alpha\\n'\nprintf 'alpha\\n'\n printf 'ignored\\n'\nprintf 'omega\\n' | cat\nexit\n",
            shell_line_endings(include_str!("command.sh"))
        );
        child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        let (version, body) = stdout.split_once('\n').expect("Bash version preamble");
        let (major, minor) = version.split_once(' ').expect("Bash major/minor version");
        let supports_ps0 = (major.parse::<u32>().unwrap(), minor.parse::<u32>().unwrap()) >= (4, 4);
        assert_eq!(body, "alpha\nalpha\nignored\nomega\n");
        let wire = String::from_utf8(output.stderr).unwrap();
        let reports = wire
            .split("\x1b]1337;SetUserVar=pebrel_command=")
            .skip(1)
            .map(|part| {
                String::from_utf8(
                    base64::engine::general_purpose::STANDARD
                        .decode(part.split('\x07').next().unwrap())
                        .unwrap(),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        // PS0 自 Bash 4.4 才提供；旧 Bash 必须保留普通输出，不能为了测试接管其输入钩子。
        if supports_ps0 {
            assert_eq!(
                reports,
                ["bash:1\nprintf 'alpha\\n'", "bash:1\nprintf 'omega\\n' | cat", "bash:1\nexit"],
                "Bash {version} must report eligible commands"
            );
        } else {
            assert!(reports.is_empty(), "Bash {version} must leave capture disabled");
        }
    }
}
