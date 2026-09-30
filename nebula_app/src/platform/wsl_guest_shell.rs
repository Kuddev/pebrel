//! What a WSL launch starts in the guest, and whether that guest user can read
//! the host zsh bootstrap: one `sh` probe per (distribution, user), cached for
//! the process. The spawn path on the UI thread only reads the cache ([`verified`]);
//! see `architecture/notes/nebula_app/platform/` for why neither fact is guessed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use nebula_terminal::tty;

use crate::shell_detect;

/// The probe may have to start the distribution; a cold WSL 2 VM takes seconds.
const PROBE_BUDGET: Duration = Duration::from_secs(10);
/// A failed or timed-out probe is not repeated before this.
const RETRY_AFTER: Duration = Duration::from_secs(300);
const MAX_OUTPUT: u64 = 4 * 1024;

const PROBE_SCRIPT: &str = include_str!("../../res/shell/wsl-guest-probe.sh");

/// The guest a launch enters: the spawn-time distribution and the explicit user.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Target {
    distro: String,
    user: Option<String>,
}

impl Target {
    /// `--distribution-id` and `--system` launches name no distribution the probe
    /// could enter and stay unverified.
    pub(crate) fn from_launch(program: &str, args: &[String]) -> Option<Self> {
        let distro = shell_detect::wsl_spawn_distro(program, args)?;
        let user = shell_detect::wsl_launch_user(program, args).map(str::to_owned);
        Some(Self { distro, user })
    }

    /// The script goes on stdin; the bootstrap travels through `WSLENV` `/p`, the
    /// translation the pane uses, so the guest tests the path it will receive.
    fn command(&self, bootstrap: &Path) -> Command {
        let mut command = Command::new("wsl.exe");
        command.args(["--distribution", &self.distro]);
        if let Some(user) = &self.user {
            command.args(["--user", user]);
        }
        command.args(["--exec", "sh", "-s"]);
        command.env("NEBULA_ZSH_INTEGRATION", bootstrap);
        command.env("WSLENV", "NEBULA_ZSH_INTEGRATION/p");
        crate::platform::process::hidden_command(&mut command);
        command
    }
}

/// The guest's answer for one [`Target`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GuestShell {
    /// The login shell from the guest's passwd entry; empty when it could not tell.
    pub(crate) login_shell: String,
    /// The bootstrap, when this guest user can read all three files through it.
    pub(crate) bootstrap: Option<PathBuf>,
}

impl GuestShell {
    /// Both `shell=` and `bootstrap=` lines are required; anything else is ignored.
    fn parse(output: &str, bootstrap: &Path) -> Option<Self> {
        let mut login_shell = None;
        let mut readable = None;
        for line in output.lines() {
            let line = line.trim_end_matches('\r');
            if let Some(value) = line.strip_prefix("shell=") {
                login_shell = Some(value.trim().to_owned());
            } else if let Some(value) = line.strip_prefix("bootstrap=") {
                readable = Some(value.trim() == "readable");
            }
        }
        Some(Self {
            login_shell: login_shell?,
            bootstrap: readable?.then(|| bootstrap.to_path_buf()),
        })
    }
}

/// The host zsh bootstrap a WSL launch should receive as `ZDOTDIR`, or `None`.
/// Cheap refusals come first (a guest command other than zsh, a host `WSLENV`
/// that forwards the user's own `ZDOTDIR`); then the guest's cached word must say
/// readable, and for a login shell also zsh. A guest that has not answered keeps
/// its own startup environment.
pub(crate) fn takes_zsh_bootstrap(
    program: &str,
    args: &[String],
    wslenv: &str,
    guest: impl FnOnce(&Target) -> Option<GuestShell>,
) -> Option<PathBuf> {
    let command = shell_detect::wsl_launch(program, args)?.command;
    if command.is_some_and(|command| !shell_detect::is_zsh_program(command)) {
        return None;
    }
    if shell_detect::wslenv_forwards(wslenv, "ZDOTDIR") {
        return None;
    }
    let shell = guest(&Target::from_launch(program, args)?)?;
    let runs_zsh = command.is_some() || shell_detect::is_zsh_program(&shell.login_shell);
    shell.bootstrap.filter(|_| runs_zsh)
}

struct Entry {
    /// Empty while the probe runs; `None` inside for a failure.
    verdict: Arc<OnceLock<Option<GuestShell>>>,
    started: Instant,
}

fn cache() -> &'static Mutex<HashMap<Target, Entry>> {
    static CACHE: OnceLock<Mutex<HashMap<Target, Entry>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The cached verdict for a guest, or `None` while it is unknown. A guest seen
/// for the first time, or one whose probe failed [`RETRY_AFTER`] ago, starts a
/// probe worker (it writes the bootstrap, then asks the guest); the caller never
/// waits for it.
pub(crate) fn verified(target: &Target) -> Option<GuestShell> {
    let mut cache = cache().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(entry) = cache.get(target) {
        match entry.verdict.get() {
            Some(Some(shell)) => return Some(shell.clone()),
            Some(None) if entry.started.elapsed() >= RETRY_AFTER => {},
            _ => return None,
        }
    }
    let verdict: Arc<OnceLock<Option<GuestShell>>> = Arc::default();
    let worker = {
        let (verdict, target) = (verdict.clone(), target.clone());
        std::thread::Builder::new().name("pebrel-wsl-guest-probe".into()).spawn(move || {
            let _ = verdict.set(
                crate::platform::shell_integration::wsl_zsh_directory()
                    .and_then(|bootstrap| probe(&target, &bootstrap)),
            );
        })
    };
    match worker {
        Ok(_) => {
            cache.insert(target.clone(), Entry { verdict, started: Instant::now() });
        },
        Err(error) => log::warn!("Could not probe the WSL guest shell: {error}"),
    }
    None
}

/// Ask the guest of the first pane's shell at process start, so a running
/// distribution has usually answered before that pane spawns. `resolve` runs on
/// the worker (it reads the registry and the shell list); a shell that is not a
/// zsh candidate never starts a guest.
pub(crate) fn warm_up(resolve: impl FnOnce() -> Option<tty::Shell> + Send + 'static) {
    let worker =
        std::thread::Builder::new().name("pebrel-wsl-guest-warmup".into()).spawn(move || {
            let Some(shell) = resolve() else { return };
            let wslenv = std::env::var("WSLENV").unwrap_or_default();
            takes_zsh_bootstrap(shell.program(), shell.args(), &wslenv, verified);
        });
    if let Err(error) = worker {
        log::warn!("Could not start the WSL guest warm-up: {error}");
    }
}

/// One guest round trip, bounded by time and output size.
fn probe(target: &Target, bootstrap: &Path) -> Option<GuestShell> {
    // A guest sh treats CR as part of each command; checkout bytes must not leak in.
    let script = PROBE_SCRIPT.replace("\r\n", "\n");
    let answer = crate::platform::process::run_bounded(
        &mut target.command(bootstrap),
        script.as_bytes(),
        PROBE_BUDGET,
        MAX_OUTPUT,
    );
    match answer.as_deref().map(|output| GuestShell::parse(output, bootstrap)) {
        Ok(Some(shell)) => {
            log::info!(
                "WSL guest {} login shell {:?}, zsh bootstrap readable: {}",
                target.distro,
                shell.login_shell,
                shell.bootstrap.is_some()
            );
            Some(shell)
        },
        Ok(None) => {
            log::warn!("WSL guest {} gave no usable shell probe answer", target.distro);
            None
        },
        Err(error) => {
            log::warn!("Could not probe WSL guest {}: {error}", target.distro);
            None
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn bootstrap() -> PathBuf {
        PathBuf::from(r"C:\Users\me\AppData\Roaming\Pebrel\wsl-zsh")
    }

    #[test]
    fn probe_enters_the_launch_guest_and_forwards_the_bootstrap_path() {
        let launch = args(&["-d", "Debian custom", "-u", "alice", "--cd", "/work/my project"]);
        let target = Target::from_launch("wsl.exe", &launch).expect("a named guest");
        assert_eq!(target, Target { distro: "Debian custom".into(), user: Some("alice".into()) });
        let command = target.command(&bootstrap());
        let got: Vec<_> =
            command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(
            got,
            ["--distribution", "Debian custom", "--user", "alice", "--exec", "sh", "-s"]
        );
        let env: HashMap<_, _> = command
            .get_envs()
            .map(|(name, value)| (name.to_owned(), value.map(ToOwned::to_owned)))
            .collect();
        assert_eq!(
            env[std::ffi::OsStr::new("WSLENV")].as_deref(),
            Some("NEBULA_ZSH_INTEGRATION/p".as_ref())
        );
        assert_eq!(
            env[std::ffi::OsStr::new("NEBULA_ZSH_INTEGRATION")].as_deref(),
            Some(bootstrap().as_os_str())
        );
        assert!(Target::from_launch("wsl.exe", &args(&["--system"])).is_none());
    }

    #[test]
    fn parse_needs_both_answers_and_ignores_the_rest() {
        let parse = |output| GuestShell::parse(output, &bootstrap());
        assert_eq!(
            parse("shell=/usr/bin/zsh\r\nbootstrap=readable\n"),
            Some(GuestShell { login_shell: "/usr/bin/zsh".into(), bootstrap: Some(bootstrap()) })
        );
        assert_eq!(
            parse("motd\nshell=\nbootstrap=unreadable\n"),
            Some(GuestShell { login_shell: String::new(), bootstrap: None })
        );
        assert_eq!(parse("shell=/bin/bash\n"), None);
        assert_eq!(parse("bootstrap=readable\n"), None);
        assert_eq!(parse(""), None);
    }

    /// A non-zsh guest command and the user's own forwarded `ZDOTDIR` never probe.
    #[test]
    fn launches_that_cannot_take_the_bootstrap_are_never_probed() {
        for (launch, wslenv) in [
            (&["-e", "htop"][..], ""),
            (&["-d", "Ubuntu", "--", "bash"], ""),
            (&["--exec", "fish", "-l"], ""),
            (&["sh", "-c", "curl install.sh | sh"], ""),
            (&["--system"], ""),
            (&["-d", "Ubuntu"], "KEEP/u:ZDOTDIR/up"),
            (&["-d", "Ubuntu"], "ZDOTDIR"),
        ] {
            let taken = takes_zsh_bootstrap("wsl.exe", &args(launch), wslenv, |_| unreachable!());
            assert!(taken.is_none(), "{launch:?}");
        }
        assert!(takes_zsh_bootstrap("pwsh.exe", &[], "", |_| unreachable!()).is_none());
    }

    /// The host cannot see the guest's login shell: an explicit zsh needs only a
    /// readable bootstrap, a login shell must also be zsh.
    #[test]
    fn the_guest_answer_decides_the_bootstrap() {
        let login = &["-d", "Ubuntu", "--cd", "/srv"][..];
        let explicit = &["-d", "Ubuntu", "-e", "/usr/bin/zsh", "-l"][..];
        let readable = |shell: &str| {
            Some(GuestShell { login_shell: shell.into(), bootstrap: Some(bootstrap()) })
        };
        let unreadable = Some(GuestShell { login_shell: "/usr/bin/zsh".into(), bootstrap: None });
        for (launch, answer, takes) in [
            (login, readable("/usr/bin/zsh"), true),
            (login, readable("/bin/zsh-5.9"), true),
            (login, readable("/usr/bin/fish"), false),
            (login, readable("/usr/bin/nu"), false),
            (login, readable("/bin/bash"), false),
            (login, readable(""), false),
            (login, unreadable.clone(), false),
            (login, None, false),
            (explicit, readable("/usr/bin/fish"), true),
            (explicit, unreadable.clone(), false),
            (explicit, None, false),
        ] {
            let taken = takes_zsh_bootstrap("wsl.exe", &args(launch), "KEEP/u", |_| answer.clone());
            assert_eq!(taken.is_some(), takes, "{launch:?} {answer:?}");
        }
    }
}
