//! What a WSL launch starts in the guest, and whether that guest user can read
//! the host zsh bootstrap.
//!
//! Neither fact is visible from the launch arguments. `wsl.exe` picks the login
//! shell from the guest's `/etc/passwd`, and the bootstrap reaches the guest only
//! through automount and `WSLENV` `/p` translation, which `[automount]
//! enabled=false`, a failed translation or drvfs permissions for another `-u`
//! user can all defeat. Handing out `ZDOTDIR` on a guess fails in both
//! directions: a zsh that cannot read the bootstrap also skips the user's own
//! startup files (zsh reads user files from `$ZDOTDIR` and falls back to `$HOME`
//! only when it is unset), and a fish or nushell login keeps a `ZDOTDIR` that
//! nothing restores. One `sh` probe per (distribution, user) answers both; the
//! verdict is cached for the process, so guest I/O happens once per guest, not
//! per pane. Nothing here waits: a spawn runs on the UI thread and takes the
//! verdict only when it is already known ([`verified`]), so [`warm_up`] asks
//! for the default shell's guest at process start.

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
/// Custom launchers can invent users; the cache stays bounded regardless.
const MAX_CACHED: usize = 32;

const PROBE_SCRIPT: &str = include_str!("../../res/shell/wsl-guest-probe.sh");

/// The guest a launch enters: the spawn-time distribution and the explicit user.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Target {
    distro: String,
    user: Option<String>,
}

impl Target {
    /// From a WSL launch's own option region: an explicit `-d`, otherwise the
    /// registry default at this moment. `--distribution-id` and `--system`
    /// launches name no distribution the probe could enter and stay unverified.
    pub(crate) fn from_launch(program: &str, args: &[String]) -> Option<Self> {
        let distro = shell_detect::wsl_spawn_distro(program, args)?;
        let user = shell_detect::wsl_launch_user(program, args).map(str::to_owned);
        Some(Self { distro, user })
    }

    /// `wsl.exe --distribution <d> [--user <u>] --exec sh -s`, the script on
    /// stdin: `wsl.exe` splits its command line itself and cannot receive a `"`
    /// inside an argument. The bootstrap travels exactly as the pane will send
    /// it, through `WSLENV` `/p`, so the guest tests the translated path.
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
    /// The login shell from the guest's passwd entry (`/usr/bin/zsh`); empty when
    /// the guest could not tell.
    pub(crate) login_shell: String,
    /// This guest user can read all three bootstrap files at the translated path.
    pub(crate) bootstrap_readable: bool,
}

impl GuestShell {
    /// Both `shell=` and `bootstrap=` lines are required; anything else is ignored.
    fn parse(output: &str) -> Option<Self> {
        let mut login_shell = None;
        let mut bootstrap = None;
        for line in output.lines() {
            let line = line.trim_end_matches('\r');
            if let Some(value) = line.strip_prefix("shell=") {
                login_shell = Some(value.trim().to_owned());
            } else if let Some(value) = line.strip_prefix("bootstrap=") {
                bootstrap = Some(value.trim() == "readable");
            }
        }
        Some(Self { login_shell: login_shell?, bootstrap_readable: bootstrap? })
    }
}

/// The host zsh bootstrap a WSL launch should receive as `ZDOTDIR`, or `None`.
///
/// An explicit guest command decides by itself (`wsl htop`, `-e bash`: never,
/// without touching the bootstrap; `-e zsh`: only when the guest can read it).
/// A launch of the login shell needs the guest's word on both. `bootstrap`
/// materialises the host directory (a file write on first use) only once the
/// launch shape allows a takeover; `guest` is [`verified`] in production and a
/// double in tests. Unknown stays untouched: a guest that has not answered
/// keeps its own startup environment.
pub(crate) fn takes_zsh_bootstrap(
    program: &str,
    args: &[String],
    bootstrap: impl FnOnce() -> Option<PathBuf>,
    guest: impl FnOnce(&Target, &Path) -> Option<GuestShell>,
) -> Option<PathBuf> {
    if !shell_detect::is_wsl_launcher(program) {
        return None;
    }
    let command = shell_detect::wsl_launch_command(program, args);
    if command.is_some_and(|command| !shell_detect::is_zsh_program(command)) {
        return None;
    }
    let target = Target::from_launch(program, args)?;
    let directory = bootstrap()?;
    let shell = guest(&target, &directory)?;
    let runs_zsh = command.is_some() || shell_detect::is_zsh_program(&shell.login_shell);
    (shell.bootstrap_readable && runs_zsh).then_some(directory)
}

/// `None` while the probe runs, then its verdict (`Some(None)` for a failure).
type Slot = Arc<Mutex<Option<Option<GuestShell>>>>;

struct Entry {
    slot: Slot,
    started: Instant,
}

impl Entry {
    fn verdict(&self) -> Option<Option<GuestShell>> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

fn cache() -> &'static Mutex<HashMap<Target, Entry>> {
    static CACHE: OnceLock<Mutex<HashMap<Target, Entry>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The cached verdict for a guest, or `None` while it is unknown. A guest seen
/// for the first time starts its probe on a worker thread and answers `None`
/// now: the caller spawns on the UI thread and must not wait for a child
/// process. A failed probe stays `None` for [`RETRY_AFTER`], then runs again.
pub(crate) fn verified(target: &Target, bootstrap: &Path) -> Option<GuestShell> {
    let mut cache = cache().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(entry) = cache.get(target) {
        match entry.verdict() {
            Some(Some(shell)) => return Some(shell),
            Some(None) if entry.started.elapsed() >= RETRY_AFTER => {},
            _ => return None,
        }
    }
    if cache.len() >= MAX_CACHED {
        // Never drop a probe that is still running: its verdict would be lost and
        // the next spawn would start a second guest process for the same target.
        cache.retain(|_, entry| entry.verdict().is_none());
    }
    let slot: Slot = Arc::default();
    let worker = {
        let (slot, target, bootstrap) = (slot.clone(), target.clone(), bootstrap.to_owned());
        std::thread::Builder::new().name("pebrel-wsl-guest-probe".into()).spawn(move || {
            let verdict = probe(&target, &bootstrap);
            *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(verdict);
        })
    };
    match worker {
        Ok(_) => {
            cache.insert(target.clone(), Entry { slot, started: Instant::now() });
        },
        Err(error) => log::warn!("Could not probe the WSL guest shell: {error}"),
    }
    None
}

/// Ask the guest of the default shell at process start, off the main thread,
/// so the first WSL pane can already carry zsh reports when the distribution
/// is running. A guest that is still booting simply answers later; a default
/// shell that is not WSL costs nothing.
pub(crate) fn warm_up(shell_id: Option<String>) {
    let worker =
        std::thread::Builder::new().name("pebrel-wsl-guest-warmup".into()).spawn(move || {
            let id = shell_id.or_else(|| nebula_settings::RuntimeSettings::load().shell);
            let Some(shell) = id.as_deref().and_then(shell_detect::resolve_id) else { return };
            let shell: tty::Shell = shell.shell();
            takes_zsh_bootstrap(
                shell.program(),
                shell.args(),
                crate::platform::shell_integration::wsl_zsh_directory,
                verified,
            );
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
    match answer.as_deref().map(GuestShell::parse) {
        Ok(Some(shell)) => {
            log::info!(
                "WSL guest {} login shell {:?}, zsh bootstrap {}",
                target.distro,
                shell.login_shell,
                if shell.bootstrap_readable { "readable" } else { "unreadable" }
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
    use std::cell::Cell;

    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn bootstrap() -> Option<PathBuf> {
        Some(PathBuf::from(r"C:\Users\me\AppData\Roaming\Pebrel\wsl-zsh"))
    }

    fn answer(shell: &str, readable: bool) -> impl FnOnce(&Target, &Path) -> Option<GuestShell> {
        let login_shell = shell.to_owned();
        move |_, _| Some(GuestShell { login_shell, bootstrap_readable: readable })
    }

    #[test]
    fn probe_enters_the_launch_guest_and_forwards_the_bootstrap_path() {
        let launch = args(&["-d", "Debian custom", "-u", "alice", "--cd", "/work/my project"]);
        let target = Target::from_launch("wsl.exe", &launch).expect("a named guest");
        assert_eq!(target, Target { distro: "Debian custom".into(), user: Some("alice".into()) });
        let directory = bootstrap().unwrap();
        let command = target.command(&directory);
        let got: Vec<_> =
            command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(
            got,
            ["--distribution", "Debian custom", "--user", "alice", "--exec", "sh", "-s"]
        );
        let env: HashMap<_, _> = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert_eq!(env["WSLENV"].as_deref(), Some("NEBULA_ZSH_INTEGRATION/p"));
        assert_eq!(env["NEBULA_ZSH_INTEGRATION"].as_deref(), directory.to_str());
        // The guest command's own options are not the guest identity.
        let nested = args(&["-d", "Ubuntu", "-e", "tool", "-u", "root"]);
        assert_eq!(Target::from_launch("wsl.exe", &nested).unwrap().user, None);
        assert!(Target::from_launch("wsl.exe", &args(&["--system"])).is_none());
        assert!(Target::from_launch("wsl.exe", &args(&["--distribution-id", "{1}"])).is_none());
        assert!(Target::from_launch("pwsh.exe", &args(&["-d", "Ubuntu"])).is_none());
    }

    #[test]
    fn probe_script_is_posix_text_that_tests_the_translated_bootstrap() {
        assert!(!PROBE_SCRIPT.replace("\r\n", "\n").contains('\r'));
        for file in [".zshenv", ".zprofile", ".zshrc"] {
            assert!(PROBE_SCRIPT.contains(&format!("$NEBULA_ZSH_INTEGRATION/{file}")), "{file}");
        }
        assert!(PROBE_SCRIPT.contains("printf 'shell=%s\\nbootstrap=%s\\n'"));
    }

    #[test]
    fn parse_needs_both_answers_and_ignores_the_rest() {
        assert_eq!(
            GuestShell::parse("shell=/usr/bin/zsh\r\nbootstrap=readable\n"),
            Some(GuestShell { login_shell: "/usr/bin/zsh".into(), bootstrap_readable: true })
        );
        assert_eq!(
            GuestShell::parse("motd\nshell=\nbootstrap=unreadable\n"),
            Some(GuestShell { login_shell: String::new(), bootstrap_readable: false })
        );
        assert_eq!(GuestShell::parse("shell=/bin/bash\n"), None);
        assert_eq!(GuestShell::parse("bootstrap=readable\n"), None);
        assert_eq!(GuestShell::parse(""), None);
    }

    /// A guest command decides by itself; only a login shell needs the guest's
    /// word, and only a possible takeover writes the bootstrap.
    #[test]
    fn guest_commands_other_than_zsh_are_never_probed() {
        for launch in [
            args(&["-e", "htop"]),
            args(&["-d", "Ubuntu", "--", "bash"]),
            args(&["--exec", "fish", "-l"]),
            args(&["sh", "-c", "curl install.sh | sh"]),
            args(&["--system"]),
        ] {
            let (written, probed) = (Cell::new(false), Cell::new(false));
            let taken = takes_zsh_bootstrap(
                "wsl.exe",
                &launch,
                || {
                    written.set(true);
                    bootstrap()
                },
                |_, _| {
                    probed.set(true);
                    Some(GuestShell {
                        login_shell: "/usr/bin/zsh".into(),
                        bootstrap_readable: true,
                    })
                },
            );
            assert!(taken.is_none(), "{launch:?}");
            assert!(!written.get(), "{launch:?} must not materialise the bootstrap");
            assert!(!probed.get(), "{launch:?} must not start a guest probe");
        }
        assert!(
            takes_zsh_bootstrap("pwsh.exe", &[], || unreachable!(), |_, _| unreachable!())
                .is_none()
        );
    }

    #[test]
    fn an_explicit_zsh_still_needs_a_readable_bootstrap() {
        let explicit = args(&["-d", "Ubuntu", "-e", "/usr/bin/zsh", "-l"]);
        assert_eq!(
            takes_zsh_bootstrap("wsl.exe", &explicit, bootstrap, answer("/usr/bin/fish", true)),
            bootstrap()
        );
        assert!(
            takes_zsh_bootstrap("wsl.exe", &explicit, bootstrap, answer("/usr/bin/zsh", false))
                .is_none()
        );
        assert!(takes_zsh_bootstrap("wsl.exe", &explicit, bootstrap, |_, _| None).is_none());
        // No host bootstrap (UNC data directory, failed write): nothing to probe for.
        assert!(
            takes_zsh_bootstrap("wsl.exe", &explicit, || None, |_, _| unreachable!()).is_none()
        );
    }

    /// The host cannot see the guest's login shell: fish, nushell, bash and an
    /// unanswered probe all keep their own startup environment.
    #[test]
    fn a_login_shell_takes_the_bootstrap_only_when_the_guest_says_zsh_can_read_it() {
        let login = args(&["-d", "Ubuntu", "--cd", "/srv"]);
        let takes = |guest: Box<dyn FnOnce(&Target, &Path) -> Option<GuestShell>>| {
            takes_zsh_bootstrap("wsl.exe", &login, bootstrap, guest).is_some()
        };
        assert!(takes(Box::new(answer("/usr/bin/zsh", true))));
        assert!(takes(Box::new(answer("/bin/zsh-5.9", true))));
        assert!(!takes(Box::new(answer("/usr/bin/zsh", false))));
        assert!(!takes(Box::new(answer("/usr/bin/fish", true))));
        assert!(!takes(Box::new(answer("/usr/bin/nu", true))));
        assert!(!takes(Box::new(answer("/bin/bash", true))));
        assert!(!takes(Box::new(answer("", true))));
        assert!(!takes(Box::new(|_, _| None)));
        // The probe sees the launch's guest, user included, and the bootstrap it must read.
        let as_alice = args(&["-d", "Ubuntu", "-u", "alice"]);
        let seen = Cell::new(None);
        takes_zsh_bootstrap("wsl.exe", &as_alice, bootstrap, |target, directory| {
            seen.set(Some((target.clone(), directory.to_owned())));
            None
        });
        assert_eq!(
            seen.take(),
            Some((
                Target { distro: "Ubuntu".into(), user: Some("alice".into()) },
                bootstrap().unwrap()
            ))
        );
    }
}
