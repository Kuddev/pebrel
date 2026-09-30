//! Duplicate launch identity and location without sharing a live session.

use gpui::{Context, Window};

use super::NebulaWorkspace;
use crate::session::LaunchSession;

pub(super) fn inherit_guest_directory(launch: &mut LaunchSession, cwd: &str) -> bool {
    update_wsl_args(launch, |program, args| crate::shell_detect::wsl_args_at(program, args, cwd))
}

/// Pin the spawn-time distribution so a bare `wsl` copy cannot follow a later
/// default change into another guest.
fn pin_distribution(launch: &mut LaunchSession, distro: &str) {
    update_wsl_args(launch, |program, args| {
        crate::shell_detect::wsl_args_pinned(program, args, distro)
    });
}

fn update_wsl_args(
    launch: &mut LaunchSession,
    update: impl FnOnce(&str, &[String]) -> Option<Vec<String>>,
) -> bool {
    let (program, args) = match launch {
        LaunchSession::Shell { program, args, .. } => (program, args),
        LaunchSession::Profile { command, args, .. } => (command, args),
        _ => return false,
    };
    let Some(updated) = update(program, args) else {
        return false;
    };
    *args = updated;
    true
}

fn wsl_program_args(launch: &LaunchSession) -> Option<(&str, &[String])> {
    match launch {
        LaunchSession::Shell { program, args, .. }
        | LaunchSession::Profile { command: program, args, .. }
            if crate::shell_detect::is_wsl_launcher(program) =>
        {
            Some((program, args))
        },
        _ => None,
    }
}

/// Launch for a split of a pane: a WSL pane is duplicated into its own guest (see
/// [`duplicate_launch`]), even without a reported guest cwd (fish, or before the
/// first prompt); other panes open the current default shell in the host cwd.
pub(super) fn split_launch(
    session: &LaunchSession,
    focused: Option<FocusedGuest<'_>>,
    raw_cwd: &str,
    host_cwd: impl FnOnce() -> Option<std::path::PathBuf>,
) -> crate::gpui_shell::terminal::view::TerminalLaunch {
    if wsl_program_args(session).is_none() {
        let cwd = host_cwd();
        return crate::gpui_shell::terminal::view::TerminalLaunch::Local {
            cwd,
            shell: None,
            shell_name: None,
        };
    }
    let (launch, cwd) = duplicate_launch(session.clone(), focused, raw_cwd, host_cwd);
    NebulaWorkspace::terminal_launch_from_session(&launch, cwd)
}

/// [`split_launch`] for a live pane.
pub(super) fn pane_split(
    view: &crate::gpui_shell::terminal::view::TerminalView,
) -> crate::gpui_shell::terminal::view::TerminalLaunch {
    split_launch(&view.session_launch, focused_guest(view), &view.cwd, || host_visible_cwd(view))
}

/// The guest a WSL pane runs in: its spawn-time distribution and explicit user.
#[derive(Clone, Copy, Debug)]
pub(super) struct FocusedGuest<'a> {
    pub distro: &'a str,
    pub user: Option<&'a str>,
}

/// The guest cwd travels through `--cd` only when `launch` enters the focused
/// pane's guest as the same user; otherwise the copy gets the host directory.
fn follow_guest(
    mut launch: LaunchSession,
    focused: Option<FocusedGuest<'_>>,
    raw_cwd: &str,
    host_cwd: impl FnOnce() -> Option<std::path::PathBuf>,
) -> (LaunchSession, Option<std::path::PathBuf>) {
    let same_guest =
        wsl_program_args(&launch).zip(focused).is_some_and(|((program, args), focused)| {
            crate::shell_detect::wsl_spawn_distro(program, args)
                .is_some_and(|distro| distro.eq_ignore_ascii_case(focused.distro))
                && crate::shell_detect::wsl_launch_user(program, args) == focused.user
        });
    if same_guest && inherit_guest_directory(&mut launch, raw_cwd) {
        return (launch, None);
    }
    (launch, host_cwd())
}

/// A new default-shell tab opened from a pane; a target that chooses its own
/// directory keeps it, as a profile `cwd` does for host shells.
pub(super) fn new_tab_launch(
    launch: LaunchSession,
    focused: Option<FocusedGuest<'_>>,
    raw_cwd: &str,
    host_cwd: impl FnOnce() -> Option<std::path::PathBuf>,
) -> (LaunchSession, Option<std::path::PathBuf>) {
    let own_directory = matches!(&launch, LaunchSession::Profile { cwd: Some(_), .. })
        || wsl_program_args(&launch)
            .and_then(|(program, args)| crate::shell_detect::wsl_launch(program, args))
            .is_some_and(|options| options.chooses_directory);
    follow_guest(launch, focused.filter(|_| !own_directory), raw_cwd, host_cwd)
}

/// A duplicate (or fork) of a tab's identity, which may differ from the focused
/// pane's. A bare WSL identity is pinned to the pane's spawn-time distribution.
pub(super) fn duplicate_launch(
    mut launch: LaunchSession,
    focused: Option<FocusedGuest<'_>>,
    raw_cwd: &str,
    host_cwd: impl FnOnce() -> Option<std::path::PathBuf>,
) -> (LaunchSession, Option<std::path::PathBuf>) {
    if let Some(focused) = focused {
        pin_distribution(&mut launch, focused.distro);
    }
    follow_guest(launch, focused, raw_cwd, host_cwd)
}

pub(super) fn focused_guest(
    view: &crate::gpui_shell::terminal::view::TerminalView,
) -> Option<FocusedGuest<'_>> {
    view.wsl_distro.as_deref().map(|distro| FocusedGuest {
        distro,
        user: wsl_program_args(&view.session_launch)
            .and_then(|(program, args)| crate::shell_detect::wsl_launch_user(program, args)),
    })
}

/// The pane's directory as a host launch may use it: a WSL guest path maps only
/// from `/mnt/<drive>` (Windows resolves `/` against the current drive, and a UNC
/// probe would block the UI thread).
pub(super) fn host_visible_cwd(
    view: &crate::gpui_shell::terminal::view::TerminalView,
) -> Option<std::path::PathBuf> {
    let is_wsl = view.wsl_distro.is_some() || wsl_program_args(&view.session_launch).is_some();
    match crate::shell_detect::wsl_guest_cwd(&view.cwd).filter(|_| is_wsl) {
        Some(guest) => crate::shell_detect::wsl_mounted_host_cwd(&crate::shell_detect::WslCwd {
            distro: String::new(),
            guest: guest.to_owned(),
        }),
        None => view.local_cwd(),
    }
}

impl NebulaWorkspace {
    /// New-tab identity from the focused pane; see [`host_visible_cwd`].
    pub(super) fn new_tab_from_focused(
        &self,
        cx: &gpui::App,
    ) -> (LaunchSession, Option<std::path::PathBuf>) {
        let launch = super::shell_launch::configured_local_launch(cx);
        let Some(view) = self.tabs.get(self.active).and_then(super::WorkspaceTab::focused_view)
        else {
            return (launch, None);
        };
        let view = view.read(cx);
        new_tab_launch(launch, focused_guest(view), &view.cwd, || host_visible_cwd(view))
    }

    pub(super) fn duplicate_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix) else { return };
        let Some(view) = tab.focused_view() else { return };
        let meta = self.meta(ix);
        let (ssh, remote_cwd, pane_id) = {
            let view = view.read(cx);
            (view.ssh_destination.clone(), view.remote_cwd(), view.pane_id)
        };
        if let Some(destination) = ssh {
            let remote_cwd = remote_cwd.or_else(|| {
                self.remote_browser.path_for(pane_id, &destination).map(ToOwned::to_owned)
            });
            self.add_ssh_terminal_at(destination, remote_cwd, window, cx);
        } else {
            // Old snapshots without an identity resolve the current default shell.
            let launch = match meta.launch {
                None | Some(LaunchSession::Default) => {
                    super::shell_launch::configured_local_launch(cx)
                },
                Some(launch) => launch,
            };
            // The tab's identity may differ from the focused pane's (a split into
            // the default shell); a guest path follows only into the same guest.
            let (launch, cwd) = {
                let view = view.read(cx);
                duplicate_launch(launch, focused_guest(view), &view.cwd, || host_visible_cwd(view))
            };
            self.add_terminal_with(launch, cwd, None, window, cx);
        }
        if let Some(target) = self.tab_meta.get_mut(self.active) {
            target.custom_name = meta.custom_name;
            target.color = meta.color;
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_wsl_shell_uses_guest_directory_even_when_it_is_not_on_the_host() {
        use crate::gpui_shell::terminal::view::TerminalLaunch;

        let mut launch = LaunchSession::Shell {
            name: "Debian".into(),
            program: "wsl.exe".into(),
            args: vec!["-d".into(), "Debian".into(), "--cd".into(), "/old".into()],
        };
        assert!(inherit_guest_directory(&mut launch, "/home/guest/new project"));
        let TerminalLaunch::Local { cwd, shell: Some(shell), .. } =
            NebulaWorkspace::terminal_launch_from_session(&launch, None)
        else {
            panic!("duplicate must use a new local WSL process");
        };
        assert!(cwd.is_none(), "the guest cwd must not be passed as a host directory");
        assert_eq!(shell.program(), "wsl.exe");
        // The raw command line keeps a spaced guest path as one `--cd` value.
        assert_eq!(shell.args(), ["--cd", "\"/home/guest/new project\"", "-d", "Debian"]);
        let LaunchSession::Shell { args, .. } = launch else { panic!("shell identity lost") };
        assert_eq!(args, ["--cd", "\"/home/guest/new project\"", "-d", "Debian"]);
    }

    #[test]
    fn imported_wsl_profile_keeps_its_user_and_shell() {
        let mut launch = LaunchSession::Profile {
            name: "Debian Zsh".into(),
            command: "wsl.exe".into(),
            args: ["-d", "Debian", "-u", "guest", "--exec", "zsh", "-l"].map(String::from).to_vec(),
            cwd: None,
            shell_id: None,
        };
        assert!(inherit_guest_directory(&mut launch, "/home/guest"));
        let LaunchSession::Profile { args, .. } = launch else { panic!("profile identity lost") };
        assert_eq!(
            args,
            ["--cd", "/home/guest", "-d", "Debian", "-u", "guest", "--exec", "zsh", "-l"]
        );
    }

    #[test]
    fn invalid_guest_directory_leaves_the_original_profile_untouched() {
        let original = LaunchSession::Shell {
            name: "Debian".into(),
            program: "wsl.exe".into(),
            args: ["-d", "Debian", "--cd", "/original"].map(String::from).to_vec(),
        };
        let mut launch = original.clone();
        assert!(!inherit_guest_directory(&mut launch, "/home/guest\r"));
        assert_eq!(launch, original);
    }

    fn shell(name: &str, program: &str, args: &[&str]) -> LaunchSession {
        LaunchSession::Shell {
            name: name.into(),
            program: program.into(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        }
    }

    fn wsl(distro: &str) -> LaunchSession {
        shell(&format!("wsl:{distro}"), "wsl.exe", &["-d", distro])
    }

    fn guest(distro: &str) -> Option<FocusedGuest<'_>> {
        Some(FocusedGuest { distro, user: None })
    }

    fn args(launch: &LaunchSession) -> &[String] {
        match launch {
            LaunchSession::Shell { args, .. } | LaunchSession::Profile { args, .. } => args,
            _ => &[],
        }
    }

    /// A copy never carries a guest path into another distribution, user or shell;
    /// a duplicate pins a bare identity, a new tab follows the default and keeps a
    /// directory the target chooses itself (a guest command's `--cd` is not one).
    #[test]
    fn copies_follow_the_guest_only_into_the_same_guest() {
        let host = std::path::PathBuf::from(r"D:\work");
        let root = Some(FocusedGuest { distro: "Ubuntu", user: Some("root") });
        let bare = shell("wsl", "wsl.exe", &[]);
        let pwsh = shell("pwsh", "pwsh.exe", &[]);
        let own = shell("work", "wsl.exe", &["-d", "Ubuntu", "--cd", "~/work"]);
        let tool = shell("tool", "wsl.exe", &["-d", "Ubuntu", "-e", "tool", "--cd", "/t"]);
        type Case<'a> = (LaunchSession, Option<FocusedGuest<'a>>, &'a [&'a str], bool);
        let duplicates: [Case; 5] = [
            (bare, guest("Ubuntu"), &["--cd", "/srv", "-d", "Ubuntu"], true),
            (wsl("Debian"), guest("Ubuntu"), &["-d", "Debian"], false),
            (wsl("Ubuntu"), root, &["-d", "Ubuntu"], false),
            (pwsh.clone(), guest("Ubuntu"), &[], false),
            (wsl("Ubuntu"), None, &["-d", "Ubuntu"], false),
        ];
        let new_tabs: [Case; 5] = [
            (wsl("Ubuntu"), guest("ubuntu"), &["--cd", "/srv", "-d", "Ubuntu"], true),
            (wsl("Debian"), guest("Ubuntu"), &["-d", "Debian"], false),
            (pwsh, guest("Ubuntu"), &[], false),
            (own, guest("Ubuntu"), &["-d", "Ubuntu", "--cd", "~/work"], false),
            (
                tool,
                guest("Ubuntu"),
                &["--cd", "/srv", "-d", "Ubuntu", "-e", "tool", "--cd", "/t"],
                true,
            ),
        ];
        for (duplicate, cases) in [(true, duplicates), (false, new_tabs)] {
            for (target, focused, expected, inherits) in cases {
                let host = || Some(host.clone());
                let (launch, cwd) = if duplicate {
                    duplicate_launch(target, focused, "/srv", host)
                } else {
                    new_tab_launch(target, focused, "/srv", host)
                };
                assert_eq!(args(&launch), expected);
                assert_eq!(cwd.is_none(), inherits, "{expected:?}");
            }
        }
    }

    #[test]
    fn split_stays_in_the_guest_and_host_panes_keep_the_default_shell() {
        use crate::gpui_shell::terminal::view::TerminalLaunch;

        let host = std::path::PathBuf::from(r"C:\Users\dev\project");
        let split = |session, focused, raw: &str| match split_launch(&session, focused, raw, || {
            Some(host.clone())
        }) {
            TerminalLaunch::Local { cwd, shell, .. } => (cwd, shell.map(|s| s.args().to_vec())),
            TerminalLaunch::Ssh { .. } => panic!("a local split stays local"),
        };
        let (cwd, args) = split(shell("wsl", "wsl.exe", &["~"]), guest("Ubuntu"), "/home/dev");
        assert_eq!(
            (cwd, args.unwrap()),
            (None, vec!["--cd".into(), "/home/dev".into(), "-d".into(), "Ubuntu".into()])
        );
        // fish or a split before the first prompt: the pane still reports a host cwd,
        // and the launch's own directory still wins, as at spawn.
        let (cwd, args) = split(wsl("Ubuntu"), guest("Ubuntu"), r"C:\Users\dev\project");
        assert_eq!((cwd, args.unwrap()), (Some(host.clone()), vec!["-d".into(), "Ubuntu".into()]));
        let own = shell("work", "wsl.exe", &["-d", "Ubuntu", "--cd", "~/work"]);
        assert_eq!(split(own, guest("Ubuntu"), "").1.unwrap(), ["-d", "Ubuntu", "--cd", "~/work"]);
        assert_eq!(split(shell("pwsh", "pwsh.exe", &[]), None, "/x"), (Some(host.clone()), None));
    }
}
