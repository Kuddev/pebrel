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
//! per pane.

use std::collections::HashMap;
use std::io::{Read as _, Seek as _, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use crate::shell_detect;

/// The probe may have to start the distribution; a cold WSL 2 VM takes seconds.
const PROBE_BUDGET: Duration = Duration::from_secs(10);
/// How long a spawn waits for its guest's first verdict. Waiting costs no prompt
/// time (the pane's own login shell waits for the same guest start), but the
/// caller runs on the UI thread, so a guest that is still booting starts this
/// pane without zsh reports and later panes use the verdict.
const SPAWN_WAIT: Duration = Duration::from_secs(2);
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

/// Whether a WSL launch should receive the host zsh bootstrap as `ZDOTDIR`.
///
/// An explicit guest command decides by itself (`wsl htop`, `-e bash`: never;
/// `-e zsh`: only when the guest can read the bootstrap). A launch of the login
/// shell needs the guest's word on both, through `guest`, which is
/// [`verified`] in production and a double in tests. Unknown stays untouched:
/// a guest that did not answer keeps its own startup environment.
pub(crate) fn takes_zsh_bootstrap(
    program: &str,
    args: &[String],
    guest: impl FnOnce(&Target) -> Option<GuestShell>,
) -> bool {
    if !shell_detect::is_wsl_launcher(program) {
        return false;
    }
    let command = shell_detect::wsl_launch_command(program, args);
    if command.is_some_and(|command| !shell_detect::is_zsh_program(command)) {
        return false;
    }
    let Some(target) = Target::from_launch(program, args) else { return false };
    let Some(shell) = guest(&target) else { return false };
    shell.bootstrap_readable
        && (command.is_some() || shell_detect::is_zsh_program(&shell.login_shell))
}

type Slot = Arc<(Mutex<Option<Option<GuestShell>>>, Condvar)>;

struct Entry {
    slot: Slot,
    started: Instant,
}

fn cache() -> &'static Mutex<HashMap<Target, Entry>> {
    static CACHE: OnceLock<Mutex<HashMap<Target, Entry>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The cached verdict for a guest, probing it on first sight. Waits at most
/// [`SPAWN_WAIT`] for a probe that is still running and returns `None` until it
/// answers; a failed probe stays `None` for [`RETRY_AFTER`].
pub(crate) fn verified(target: &Target, bootstrap: &Path) -> Option<GuestShell> {
    let slot = {
        let mut cache = cache().lock().unwrap_or_else(PoisonError::into_inner);
        let expired = cache.get(target).is_some_and(|entry| {
            let verdict = entry.slot.0.lock().unwrap_or_else(PoisonError::into_inner);
            matches!(*verdict, Some(None)) && entry.started.elapsed() >= RETRY_AFTER
        });
        if expired {
            cache.remove(target);
        }
        match cache.get(target) {
            Some(entry) => entry.slot.clone(),
            None => {
                if cache.len() >= MAX_CACHED {
                    cache.clear();
                }
                let slot: Slot = Arc::new((Mutex::new(None), Condvar::new()));
                let worker = {
                    let (slot, target, bootstrap) =
                        (slot.clone(), target.clone(), bootstrap.to_owned());
                    std::thread::Builder::new().name("pebrel-wsl-guest-probe".into()).spawn(
                        move || {
                            let verdict = probe(&target, &bootstrap);
                            let (result, ready) = &*slot;
                            *result.lock().unwrap_or_else(PoisonError::into_inner) = Some(verdict);
                            ready.notify_all();
                        },
                    )
                };
                if let Err(error) = worker {
                    log::warn!("Could not probe the WSL guest shell: {error}");
                    return None;
                }
                cache.insert(target.clone(), Entry { slot: slot.clone(), started: Instant::now() });
                slot
            },
        }
    };
    let (result, ready) = &*slot;
    let deadline = Instant::now() + SPAWN_WAIT;
    let mut verdict = result.lock().unwrap_or_else(PoisonError::into_inner);
    while verdict.is_none() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            log::info!(
                "WSL guest {} has not answered the shell probe yet; this pane starts without zsh reports",
                target.distro
            );
            return None;
        }
        verdict = ready.wait_timeout(verdict, remaining).unwrap_or_else(PoisonError::into_inner).0;
    }
    verdict.clone().flatten()
}

/// One guest round trip, bounded by time and output size. Temporary files stand
/// in for pipes so no reader thread outlives a guest that keeps a handle open.
fn probe(target: &Target, bootstrap: &Path) -> Option<GuestShell> {
    let run = || -> std::io::Result<Option<GuestShell>> {
        let mut input = tempfile::tempfile()?;
        // A guest sh treats CR as part of each command; checkout bytes must not leak in.
        input.write_all(PROBE_SCRIPT.replace("\r\n", "\n").as_bytes())?;
        input.rewind()?;
        let mut output = tempfile::tempfile()?;
        let mut child = target
            .command(bootstrap)
            .stdin(input)
            .stdout(output.try_clone()?)
            .stderr(Stdio::null())
            .spawn()?;
        let deadline = Instant::now() + PROBE_BUDGET;
        let status = loop {
            match child.try_wait()? {
                Some(status) => break status,
                None if Instant::now() < deadline && output.metadata()?.len() <= MAX_OUTPUT => {
                    std::thread::sleep(Duration::from_millis(10));
                },
                None => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(std::io::Error::other("WSL guest probe exceeded its budget"));
                },
            }
        };
        if !status.success() {
            return Err(std::io::Error::other(format!("WSL guest probe exited with {status}")));
        }
        output.rewind()?;
        let mut text = String::new();
        output.take(MAX_OUTPUT).read_to_string(&mut text)?;
        Ok(GuestShell::parse(&text))
    };
    match run() {
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

    fn answer(shell: &str, readable: bool) -> impl FnOnce(&Target) -> Option<GuestShell> {
        let login_shell = shell.to_owned();
        move |_| Some(GuestShell { login_shell, bootstrap_readable: readable })
    }

    #[test]
    fn probe_enters_the_launch_guest_and_forwards_the_bootstrap_path() {
        let launch = args(&["-d", "Debian custom", "-u", "alice", "--cd", "/work/my project"]);
        let target = Target::from_launch("wsl.exe", &launch).expect("a named guest");
        assert_eq!(target, Target { distro: "Debian custom".into(), user: Some("alice".into()) });
        let bootstrap = Path::new(r"C:\Users\me\AppData\Roaming\Pebrel\wsl-zsh");
        let command = target.command(bootstrap);
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
        assert_eq!(env["NEBULA_ZSH_INTEGRATION"].as_deref(), bootstrap.to_str());
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

    /// A guest command decides by itself; only a login shell needs the guest's word.
    #[test]
    fn guest_commands_other_than_zsh_are_never_probed() {
        for launch in [
            args(&["-e", "htop"]),
            args(&["-d", "Ubuntu", "--", "bash"]),
            args(&["--exec", "fish", "-l"]),
            args(&["sh", "-c", "curl install.sh | sh"]),
            args(&["--system"]),
        ] {
            let probed = Cell::new(false);
            let taken = takes_zsh_bootstrap("wsl.exe", &launch, |_| {
                probed.set(true);
                Some(GuestShell { login_shell: "/usr/bin/zsh".into(), bootstrap_readable: true })
            });
            assert!(!taken, "{launch:?}");
            assert!(!probed.get(), "{launch:?} must not start a guest probe");
        }
        assert!(!takes_zsh_bootstrap("pwsh.exe", &[], |_| unreachable!()));
    }

    #[test]
    fn an_explicit_zsh_still_needs_a_readable_bootstrap() {
        let explicit = args(&["-d", "Ubuntu", "-e", "/usr/bin/zsh", "-l"]);
        assert!(takes_zsh_bootstrap("wsl.exe", &explicit, answer("/usr/bin/fish", true)));
        assert!(!takes_zsh_bootstrap("wsl.exe", &explicit, answer("/usr/bin/zsh", false)));
        assert!(!takes_zsh_bootstrap("wsl.exe", &explicit, |_| None));
    }

    /// The host cannot see the guest's login shell: fish, nushell, bash and an
    /// unanswered probe all keep their own startup environment.
    #[test]
    fn a_login_shell_takes_the_bootstrap_only_when_the_guest_says_zsh_can_read_it() {
        let login = args(&["-d", "Ubuntu", "--cd", "/srv"]);
        assert!(takes_zsh_bootstrap("wsl.exe", &login, answer("/usr/bin/zsh", true)));
        assert!(takes_zsh_bootstrap("wsl.exe", &login, answer("/bin/zsh-5.9", true)));
        assert!(!takes_zsh_bootstrap("wsl.exe", &login, answer("/usr/bin/zsh", false)));
        assert!(!takes_zsh_bootstrap("wsl.exe", &login, answer("/usr/bin/fish", true)));
        assert!(!takes_zsh_bootstrap("wsl.exe", &login, answer("/usr/bin/nu", true)));
        assert!(!takes_zsh_bootstrap("wsl.exe", &login, answer("/bin/bash", true)));
        assert!(!takes_zsh_bootstrap("wsl.exe", &login, answer("", true)));
        assert!(!takes_zsh_bootstrap("wsl.exe", &login, |_| None));
        // The probe sees the launch's guest, user included.
        let as_alice = args(&["-d", "Ubuntu", "-u", "alice"]);
        let seen = Cell::new(None);
        takes_zsh_bootstrap("wsl.exe", &as_alice, |target| {
            seen.set(Some(target.clone()));
            None
        });
        assert_eq!(
            seen.take(),
            Some(Target { distro: "Ubuntu".into(), user: Some("alice".into()) })
        );
    }
}
