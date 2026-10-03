//! Duplicate launch identity and location without sharing a live session.

use std::path::PathBuf;

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

/// How a copy relates to the pane it starts from.
#[derive(Clone, Copy, Debug)]
pub(super) enum CopyKind {
    /// A WSL pane is duplicated into its own guest, even without a reported guest
    /// cwd (fish, or before the first prompt); other panes open the current
    /// default shell in the host cwd.
    Split,
    /// A duplicate (or fork) of a tab's identity, which may differ from the
    /// pane's. A bare WSL identity is pinned to the pane's spawn-time distribution.
    Duplicate,
    /// A new default-shell tab; a target that chooses its own directory keeps it,
    /// as a profile `cwd` does for host shells.
    NewTab,
}

/// The guest a WSL pane runs in: its spawn-time distribution and explicit user.
#[derive(Clone, Copy, Debug)]
pub(super) struct FocusedGuest<'a> {
    pub distro: &'a str,
    pub user: Option<&'a str>,
}

/// The pane a copy starts from.
pub(super) struct PaneOrigin<'a> {
    pub guest: Option<FocusedGuest<'a>>,
    pub cwd: &'a str,
    /// The pane's directory as a host launch may use it.
    pub host_cwd: Option<PathBuf>,
}

impl<'a> PaneOrigin<'a> {
    /// A WSL guest path maps to the host only from `/mnt/<drive>` (Windows
    /// resolves `/` against the current drive, and a UNC probe would block the
    /// UI thread).
    pub(super) fn of(view: &'a crate::gpui_shell::terminal::view::TerminalView) -> Self {
        let context = view.exec_context.as_ref();
        let is_wsl = context.is_some_and(|context| context.wsl_distribution().is_some());
        let host_cwd = match crate::shell_detect::wsl_guest_cwd(&view.cwd).filter(|_| is_wsl) {
            Some(guest) => {
                crate::shell_detect::wsl_mounted_host_cwd(&crate::shell_detect::WslCwd {
                    distro: String::new(),
                    guest: guest.to_owned(),
                })
            },
            None => view.local_cwd(),
        };
        let guest = context.and_then(|context| {
            Some(FocusedGuest { distro: context.wsl_distribution()??, user: context.wsl_user() })
        });
        Self { guest, cwd: &view.cwd, host_cwd }
    }
}

/// The guest cwd travels through `--cd` only when the copy enters the origin
/// pane's guest as the same user; otherwise the copy gets the host directory.
pub(super) fn copy_launch(
    mut launch: LaunchSession,
    kind: CopyKind,
    origin: PaneOrigin<'_>,
) -> (LaunchSession, Option<PathBuf>) {
    let mut focused = origin.guest;
    match kind {
        CopyKind::Split if wsl_program_args(&launch).is_none() => {
            return (LaunchSession::Default, origin.host_cwd);
        },
        CopyKind::Split | CopyKind::Duplicate => {
            if let Some(focused) = focused {
                pin_distribution(&mut launch, focused.distro);
            }
        },
        CopyKind::NewTab => {
            let own_directory = matches!(&launch, LaunchSession::Profile { cwd: Some(_), .. })
                || wsl_program_args(&launch)
                    .and_then(|(program, args)| crate::shell_detect::wsl_launch(program, args))
                    .is_some_and(|options| options.chooses_directory);
            focused = focused.filter(|_| !own_directory);
        },
    }
    let same_guest =
        wsl_program_args(&launch).zip(focused).is_some_and(|((program, args), focused)| {
            crate::shell_detect::wsl_spawn_distro(program, args)
                .is_some_and(|distro| distro.eq_ignore_ascii_case(focused.distro))
                && crate::shell_detect::wsl_launch_user(program, args) == focused.user
        });
    if same_guest && inherit_guest_directory(&mut launch, origin.cwd) {
        return (launch, None);
    }
    (launch, origin.host_cwd)
}

impl NebulaWorkspace {
    /// New-tab identity from the focused pane; see [`PaneOrigin::of`].
    pub(super) fn new_tab_from_focused(&self, cx: &gpui::App) -> (LaunchSession, Option<PathBuf>) {
        let launch = super::shell_launch::configured_local_launch(cx);
        let Some(view) = self.tabs.get(self.active).and_then(super::WorkspaceTab::focused_view)
        else {
            return (launch, None);
        };
        copy_launch(launch, CopyKind::NewTab, PaneOrigin::of(view.read(cx)))
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
            let (launch, cwd) =
                copy_launch(launch, CopyKind::Duplicate, PaneOrigin::of(view.read(cx)));
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
        for (kind, cases) in [(CopyKind::Duplicate, duplicates), (CopyKind::NewTab, new_tabs)] {
            for (target, guest, expected, inherits) in cases {
                let origin = PaneOrigin { guest, cwd: "/srv", host_cwd: Some(host.clone()) };
                let (launch, cwd) = copy_launch(target, kind, origin);
                assert_eq!(args(&launch), expected);
                assert_eq!(cwd.is_none(), inherits, "{expected:?}");
            }
        }
    }

    #[test]
    fn split_stays_in_the_guest_and_host_panes_keep_the_default_shell() {
        use crate::gpui_shell::terminal::view::TerminalLaunch;

        let host = std::path::PathBuf::from(r"C:\Users\dev\project");
        let split = |session, guest, cwd| {
            let origin = PaneOrigin { guest, cwd, host_cwd: Some(host.clone()) };
            let (launch, cwd) = copy_launch(session, CopyKind::Split, origin);
            match NebulaWorkspace::terminal_launch_from_session(&launch, cwd) {
                TerminalLaunch::Local { cwd, shell, .. } => (cwd, shell.map(|s| s.args().to_vec())),
                TerminalLaunch::Ssh { .. } => panic!("a local split stays local"),
            }
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
