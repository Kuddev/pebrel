//! What a WSL launch starts in the guest, and whether that guest user can read
//! the host zsh bootstrap: one `sh` probe per (distribution, user), cached for
//! the process. The spawn path on the UI thread only reads the cache ([`verified`]);
//! see `architecture/notes/nebula_app/platform/` for why neither fact is guessed.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use nebula_terminal::tty;

use crate::shell_detect;

/// The probe may have to start the distribution; a cold WSL 2 VM takes seconds.
const PROBE_BUDGET: Duration = Duration::from_secs(10);
/// A failed or timed-out probe is not repeated before this.
const RETRY_AFTER: Duration = Duration::from_secs(300);
const MAX_OUTPUT: usize = 4 * 1024;

const PROBE_SCRIPT: &str = include_str!("../../res/shell/wsl-guest-probe.sh");

/// The guest a launch enters: the spawn-time distribution and the explicit user.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Target {
    distro: String,
    user: Option<String>,
}

impl Target {
    /// The script goes on stdin; the bootstrap travels through `WSLENV` `/p`, the
    /// translation the pane uses, so the guest tests the path it will receive.
    fn command(&self, bootstrap: &Path) -> Command {
        let mut command = shell_detect::wsl_exec_command(&self.distro, self.user.as_deref());
        command.args(["sh", "-s"]);
        command.env("NEBULA_ZSH_INTEGRATION", bootstrap);
        command.env("WSLENV", "NEBULA_ZSH_INTEGRATION/p");
        crate::platform::process::hidden_command(&mut command);
        command
    }
}

/// The guest's answer for one [`Target`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct GuestShell {
    /// The login shell from the guest's passwd entry is zsh.
    pub(crate) login_zsh: bool,
    /// This guest user can read all three bootstrap files.
    pub(crate) readable: bool,
}

impl GuestShell {
    /// Both `shell=` and `bootstrap=` lines are required; anything else is ignored.
    fn parse(output: &str) -> Option<Self> {
        let mut login_zsh = None;
        let mut readable = None;
        for line in output.lines() {
            let line = line.trim_end_matches('\r');
            if let Some(value) = line.strip_prefix("shell=") {
                login_zsh = Some(shell_detect::is_zsh_program(value.trim()));
            } else if let Some(value) = line.strip_prefix("bootstrap=") {
                readable = Some(value.trim() == "readable");
            }
        }
        Some(Self { login_zsh: login_zsh?, readable: readable? })
    }
}

/// Whether a WSL launch should receive the host zsh bootstrap
/// ([`crate::platform::shell_integration::wsl_zsh_path`]) as `ZDOTDIR`.
/// Cheap refusals come first (a guest command other than zsh, a host `WSLENV`
/// that forwards the user's own `ZDOTDIR`); then the guest's cached word must say
/// readable, and for a login shell also zsh. A guest that has not answered keeps
/// its own startup environment. `--distribution-id` and `--system` launches name
/// no distribution the probe could enter and stay unverified.
pub(crate) fn takes_zsh_bootstrap(
    program: &str,
    args: &[String],
    wslenv: &str,
    guest: impl FnOnce(&Target) -> Option<GuestShell>,
) -> bool {
    let Some(options) = shell_detect::wsl_launch(program, args) else { return false };
    // `None` for the login shell, which only the guest can name.
    let command_zsh = options.command.map(shell_detect::is_zsh_program);
    if command_zsh == Some(false) || shell_detect::wslenv_forwards(wslenv, "ZDOTDIR") {
        return false;
    }
    let Some(distro) = options.spawn_distro(crate::platform::shell::default_wsl_distro) else {
        return false;
    };
    let target = Target { distro, user: options.user.map(str::to_owned) };
    guest(&target).is_some_and(|shell| shell.readable && command_zsh.unwrap_or(shell.login_zsh))
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
            Some(Some(shell)) => return Some(*shell),
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
    let answer = crate::platform::process_output::read_with_input(
        target.command(bootstrap),
        tty::shell_line_endings(PROBE_SCRIPT).as_bytes(),
        PROBE_BUDGET,
        MAX_OUTPUT,
        &|| false,
    );
    let answer = answer.map(|output| String::from_utf8_lossy(&output).into_owned());
    match answer.as_deref().map(|output| (output, GuestShell::parse(output))) {
        Ok((output, Some(shell))) => {
            log::info!("WSL guest {} answered {:?}: {shell:?}", target.distro, output.trim());
            Some(shell)
        },
        Ok((_, None)) => {
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
    use std::path::PathBuf;

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
        let mut target = None;
        takes_zsh_bootstrap("wsl.exe", &launch, "", |guest| {
            target = Some(guest.clone());
            None
        });
        let target = target.expect("a named guest is probed");
        assert_eq!(target, Target { distro: "Debian custom".into(), user: Some("alice".into()) });
        let command = target.command(&bootstrap());
        let got: Vec<_> =
            command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(got, ["-d", "Debian custom", "-u", "alice", "--exec", "sh", "-s"]);
        // In name order (std keeps them sorted), and nothing else is set.
        let env: Vec<_> = command.get_envs().collect();
        assert_eq!(
            env,
            [
                ("NEBULA_ZSH_INTEGRATION".as_ref(), Some(bootstrap().as_os_str())),
                ("WSLENV".as_ref(), Some("NEBULA_ZSH_INTEGRATION/p".as_ref())),
            ]
        );
    }

    #[test]
    fn parse_needs_both_answers_and_ignores_the_rest() {
        let parse = GuestShell::parse;
        let answer = |login_zsh, readable| Some(GuestShell { login_zsh, readable });
        assert_eq!(parse("shell=/usr/bin/zsh\r\nbootstrap=readable\n"), answer(true, true));
        assert_eq!(
            parse("motd\nshell=/usr/bin/fish\nbootstrap=unreadable\n"),
            answer(false, false)
        );
        assert_eq!(parse("shell=\nbootstrap=readable\n"), answer(false, true));
        assert_eq!(parse("shell=/bin/bash\n"), None);
        assert_eq!(parse("bootstrap=readable\n"), None);
        assert_eq!(parse(""), None);
    }

    /// A non-zsh guest command and the user's own forwarded `ZDOTDIR` never probe.
    #[test]
    fn launches_that_cannot_take_the_bootstrap_are_never_probed() {
        for (launch, wslenv) in [
            (&["-e", "htop"][..], ""),
            (&["--system"], ""),
            (&["-d", "Ubuntu"], "KEEP/u:ZDOTDIR/up"),
        ] {
            let taken = takes_zsh_bootstrap("wsl.exe", &args(launch), wslenv, |_| unreachable!());
            assert!(!taken, "{launch:?}");
        }
        assert!(!takes_zsh_bootstrap("pwsh.exe", &[], "", |_| unreachable!()));
    }

    /// The host cannot see the guest's login shell: an explicit zsh needs only a
    /// readable bootstrap, a login shell must also be zsh.
    #[test]
    fn the_guest_answer_decides_the_bootstrap() {
        let login = &["-d", "Ubuntu", "--cd", "/srv"][..];
        let explicit = &["-d", "Ubuntu", "-e", "/usr/bin/zsh", "-l"][..];
        let answer = |login_zsh, readable| Some(GuestShell { login_zsh, readable });
        for (launch, answer, takes) in [
            (login, answer(true, true), true),
            (login, answer(false, true), false),
            (login, answer(true, false), false),
            (login, None, false),
            (explicit, answer(false, true), true),
            (explicit, answer(true, false), false),
        ] {
            let taken = takes_zsh_bootstrap("wsl.exe", &args(launch), "KEEP/u", |_| answer);
            assert_eq!(taken, takes, "{launch:?} {answer:?}");
        }
    }
}
