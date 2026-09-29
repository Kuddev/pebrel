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

/// Launch for a split of a pane. A WSL pane (explicit or default shell) keeps its
/// distribution/user and passes the guest cwd through `--cd`; Windows would
/// otherwise resolve `/home/x` against the current drive. A WSL pane without a
/// reported guest cwd (fish, or before the first prompt) still stays in its guest.
/// Other panes keep the current-default-shell behavior with the host cwd.
pub(super) fn split_launch(
    session: &LaunchSession,
    wsl_distro: Option<&str>,
    raw_cwd: &str,
    host_cwd: Option<std::path::PathBuf>,
) -> crate::gpui_shell::terminal::view::TerminalLaunch {
    if wsl_program_args(session).is_none() {
        return crate::gpui_shell::terminal::view::TerminalLaunch::Local {
            cwd: host_cwd,
            shell: None,
            shell_name: None,
        };
    }
    let mut launch = session.clone();
    if let Some(distro) = wsl_distro {
        pin_distribution(&mut launch, distro);
    }
    if inherit_guest_directory(&mut launch, raw_cwd) {
        return NebulaWorkspace::terminal_launch_from_session(&launch, None);
    }
    // Without a guest report the pane still shows its spawn-time host directory.
    // Replay that spawn as is: the launch's own `--cd` or `~` won there too.
    let host_cwd = host_cwd.filter(|_| crate::shell_detect::wsl_guest_cwd(raw_cwd).is_none());
    NebulaWorkspace::terminal_launch_from_session(&launch, host_cwd)
}

/// [`split_launch`] for a live pane.
pub(super) fn pane_split(
    view: &crate::gpui_shell::terminal::view::TerminalView,
) -> crate::gpui_shell::terminal::view::TerminalLaunch {
    split_launch(&view.session_launch, view.wsl_distro.as_deref(), &view.cwd, view.local_cwd())
}

/// The guest a WSL pane runs in: its spawn-time distribution and explicit user.
#[derive(Clone, Copy, Debug)]
pub(super) struct FocusedGuest<'a> {
    pub distro: &'a str,
    pub user: Option<&'a str>,
}

/// Launch and host directory for a new default-shell tab opened from a pane.
/// When the default shell enters the focused pane's WSL distribution as the same
/// user, the guest cwd travels through `--cd`; otherwise only a host-visible
/// directory is used. A target that chooses its own directory keeps it, as a
/// profile `cwd` does for host shells.
pub(super) fn new_tab_launch(
    mut launch: LaunchSession,
    focused: Option<FocusedGuest<'_>>,
    raw_cwd: &str,
    host_cwd: impl FnOnce() -> Option<std::path::PathBuf>,
) -> (LaunchSession, Option<std::path::PathBuf>) {
    let same_guest =
        wsl_program_args(&launch).zip(focused).is_some_and(|((program, args), focused)| {
            let own_directory = args.first().is_some_and(|arg| arg == "~")
                || args.iter().any(|arg| arg == "--cd" || arg.starts_with("--cd="))
                || matches!(&launch, LaunchSession::Profile { cwd: Some(_), .. });
            !own_directory
                && crate::shell_detect::wsl_spawn_distro(program, args)
                    .is_some_and(|target| target.eq_ignore_ascii_case(focused.distro))
                && crate::shell_detect::wsl_launch_user(program, args) == focused.user
        });
    if same_guest && inherit_guest_directory(&mut launch, raw_cwd) {
        return (launch, None);
    }
    (launch, host_cwd())
}

/// Launch and host directory for a duplicate of a tab's identity. A bare WSL
/// identity is pinned to the focused pane's spawn-time distribution; the guest
/// cwd travels only when the copy then enters that guest as the same user. A
/// pane without a snapshot (`--distribution-id`, `--system`) qualifies only when
/// the identity is its own launch apart from the directory.
pub(super) fn duplicate_launch(
    mut launch: LaunchSession,
    focused: Option<FocusedGuest<'_>>,
    pane_launch: &LaunchSession,
    raw_cwd: &str,
    host_cwd: impl FnOnce() -> Option<std::path::PathBuf>,
) -> (LaunchSession, Option<std::path::PathBuf>) {
    let same_guest = match focused {
        Some(focused) => {
            pin_distribution(&mut launch, focused.distro);
            wsl_program_args(&launch).is_some_and(|(program, args)| {
                crate::shell_detect::wsl_launch_distro(program, args)
                    .is_some_and(|distro| distro.eq_ignore_ascii_case(focused.distro))
                    && crate::shell_detect::wsl_launch_user(program, args) == focused.user
            })
        },
        None => {
            let identity = |launch| {
                wsl_program_args(launch).and_then(|(program, args)| {
                    crate::shell_detect::wsl_args_in_host_directory(program, args)
                })
            };
            identity(&launch).is_some_and(|args| Some(args) == identity(pane_launch))
        },
    };
    if same_guest && inherit_guest_directory(&mut launch, raw_cwd) {
        return (launch, None);
    }
    (launch, host_cwd())
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

/// The pane's directory as a host launch may use it. A WSL pane's guest path
/// never reaches the host `is_dir` check (Windows resolves `/` against the
/// current drive), even without a distribution snapshot; only `/mnt/<drive>`
/// maps, since a UNC probe would block the UI thread and cmd or another guest
/// cannot start in `\\wsl.localhost\…`.
pub(super) fn host_visible_cwd(
    view: &crate::gpui_shell::terminal::view::TerminalView,
) -> Option<std::path::PathBuf> {
    let is_wsl = view.wsl_distro.is_some() || wsl_program_args(&view.session_launch).is_some();
    match crate::shell_detect::wsl_guest_cwd(&view.cwd).filter(|_| is_wsl) {
        Some(guest) => crate::shell_detect::wsl_mounted_host_cwd(&crate::shell_detect::WslCwd {
            distro: view.wsl_distro.clone().unwrap_or_default(),
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
                duplicate_launch(
                    launch,
                    focused_guest(view),
                    &view.session_launch,
                    &view.cwd,
                    || host_visible_cwd(view),
                )
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

    #[test]
    fn split_of_wsl_pane_keeps_distribution_and_guest_directory() {
        use crate::gpui_shell::terminal::view::TerminalLaunch;

        // Default-shell case: startup snapshots the resolved WSL shell as the identity.
        let session = LaunchSession::Shell {
            name: "wsl:Ubuntu".into(),
            program: r"C:\Windows\System32\wsl.exe".into(),
            args: ["-d", "Ubuntu", "-u", "dev"].map(String::from).to_vec(),
        };
        let TerminalLaunch::Local { cwd, shell: Some(shell), .. } =
            split_launch(&session, Some("Ubuntu"), "/home/dev/project", None)
        else {
            panic!("a split of a WSL pane must launch the same WSL shell");
        };
        assert!(cwd.is_none(), "the guest cwd must not be passed as a host directory");
        assert_eq!(shell.args(), ["--cd", "/home/dev/project", "-d", "Ubuntu", "-u", "dev"]);
    }

    fn split_args(
        session: &LaunchSession,
        raw_cwd: &str,
        host_cwd: Option<std::path::PathBuf>,
    ) -> (Option<std::path::PathBuf>, Vec<String>) {
        use crate::gpui_shell::terminal::view::TerminalLaunch;

        let TerminalLaunch::Local { cwd, shell: Some(shell), .. } =
            split_launch(session, Some("Ubuntu"), raw_cwd, host_cwd)
        else {
            panic!("a split of a WSL pane must launch WSL");
        };
        (cwd, shell.args().to_vec())
    }

    #[test]
    fn split_of_bare_wsl_pane_pins_the_snapshotted_distribution() {
        let bare = LaunchSession::Shell {
            name: "wsl".into(),
            program: "wsl.exe".into(),
            args: vec!["~".into()],
        };
        let (cwd, args) = split_args(&bare, "/home/dev", None);
        assert!(cwd.is_none());
        assert_eq!(args, ["--cd", "/home/dev", "-d", "Ubuntu"]);
    }

    #[test]
    fn split_of_wsl_pane_without_guest_cwd_stays_in_the_guest() {
        let session = wsl("Ubuntu");
        // fish or a split before the first prompt: the pane still reports a host cwd.
        // A Windows host path, as WSL panes report; a Unix temp dir would read as a guest path.
        let host = std::path::PathBuf::from(r"C:\Users\dev\project");
        let (cwd, args) = split_args(&session, &host.to_string_lossy(), Some(host.clone()));
        assert_eq!(cwd, Some(host.clone()));
        assert_eq!(args, ["-d", "Ubuntu"]);
        let (cwd, args) = split_args(&session, "", None);
        assert!(cwd.is_none());
        assert_eq!(args, ["-d", "Ubuntu"]);
        // Before the first report the launch's own directory still wins, as at spawn.
        let own = LaunchSession::Shell {
            name: "work".into(),
            program: "wsl.exe".into(),
            args: ["-d", "Ubuntu", "--cd", "~/work"].map(String::from).to_vec(),
        };
        let (_, args) = split_args(&own, &host.to_string_lossy(), Some(host.clone()));
        assert_eq!(args, ["-d", "Ubuntu", "--cd", "~/work"]);
    }

    #[test]
    fn duplicate_follows_the_guest_only_into_the_same_guest() {
        let bare = LaunchSession::Shell {
            name: "wsl".into(),
            program: "wsl.exe".into(),
            args: Vec::new(),
        };
        let (launch, cwd) =
            duplicate_launch(bare, guest("Ubuntu"), &LaunchSession::Default, "/srv", || {
                panic!("the same guest needs no host directory")
            });
        assert!(cwd.is_none());
        let LaunchSession::Shell { args, .. } = launch else { panic!("shell identity lost") };
        assert_eq!(args, ["--cd", "/srv", "-d", "Ubuntu"]);

        // A tab identity in another distribution or shell never receives the guest path.
        let host = Some(std::path::PathBuf::from(r"D:\work"));
        let (launch, cwd) = duplicate_launch(
            wsl("Debian"),
            guest("Ubuntu"),
            &LaunchSession::Default,
            "/tmp",
            || host.clone(),
        );
        assert_eq!((launch, cwd), (wsl("Debian"), host.clone()));
        let focused = Some(FocusedGuest { distro: "Ubuntu", user: Some("root") });
        let (launch, cwd) =
            duplicate_launch(wsl("Ubuntu"), focused, &LaunchSession::Default, "/root", || None);
        assert_eq!((launch, cwd), (wsl("Ubuntu"), None));
        let pwsh = LaunchSession::Shell {
            name: "pwsh".into(),
            program: "pwsh.exe".into(),
            args: Vec::new(),
        };
        let (launch, cwd) =
            duplicate_launch(pwsh.clone(), guest("Ubuntu"), &LaunchSession::Default, "/", || None);
        assert_eq!((launch, cwd), (pwsh, None));

        // Without a snapshot the guest cwd follows only the pane's own identity.
        let by_id = |cd: &[&str]| LaunchSession::Shell {
            name: "id".into(),
            program: "wsl.exe".into(),
            args: [&["--distribution-id", "{0000}"][..], cd]
                .concat()
                .into_iter()
                .map(String::from)
                .collect(),
        };
        let (launch, cwd) =
            duplicate_launch(by_id(&[]), None, &by_id(&["--cd", "/a"]), "/b", || {
                panic!("the same identity needs no host directory")
            });
        assert!(cwd.is_none());
        let LaunchSession::Shell { args, .. } = launch else { panic!("shell identity lost") };
        assert_eq!(args, ["--cd", "/b", "--distribution-id", "{0000}"]);
        let (launch, cwd) = duplicate_launch(by_id(&[]), None, &wsl("Ubuntu"), "/b", || None);
        assert_eq!((launch, cwd), (by_id(&[]), None));
    }

    fn wsl(distro: &str) -> LaunchSession {
        LaunchSession::Shell {
            name: format!("wsl:{distro}"),
            program: "wsl.exe".into(),
            args: ["-d", distro].map(String::from).to_vec(),
        }
    }

    fn guest(distro: &str) -> Option<FocusedGuest<'_>> {
        Some(FocusedGuest { distro, user: None })
    }

    #[test]
    fn new_tab_in_the_same_wsl_distribution_keeps_the_guest_directory() {
        let (launch, cwd) = new_tab_launch(wsl("Ubuntu"), guest("ubuntu"), "/home/dev/app", || {
            panic!("the host directory is not needed for the same guest")
        });
        assert!(cwd.is_none(), "the guest cwd must not become a host directory");
        let LaunchSession::Shell { args, .. } = launch else { panic!("shell identity lost") };
        assert_eq!(args, ["--cd", "/home/dev/app", "-d", "Ubuntu"]);
    }

    #[test]
    fn new_tab_in_another_shell_only_uses_the_host_visible_directory() {
        let host = Some(std::path::PathBuf::from(r"D:\work"));
        // Another distribution: no guest path from Ubuntu may leak into Debian.
        let (launch, cwd) =
            new_tab_launch(wsl("Debian"), guest("Ubuntu"), "/mnt/d/work", || host.clone());
        assert_eq!(launch, wsl("Debian"));
        assert_eq!(cwd, host);
        // A host default shell opened from a WSL pane.
        let pwsh = LaunchSession::Shell {
            name: "pwsh".into(),
            program: "pwsh.exe".into(),
            args: Vec::new(),
        };
        let (launch, cwd) = new_tab_launch(pwsh.clone(), guest("Ubuntu"), "/home/dev", || None);
        assert_eq!(launch, pwsh);
        assert!(cwd.is_none());
    }

    #[test]
    fn new_tab_as_another_guest_user_does_not_inherit_the_guest_directory() {
        let focused = Some(FocusedGuest { distro: "Ubuntu", user: Some("root") });
        let (launch, cwd) = new_tab_launch(wsl("Ubuntu"), focused, "/root/secret", || None);
        assert_eq!(launch, wsl("Ubuntu"));
        assert!(cwd.is_none());
    }

    #[test]
    fn new_tab_profile_with_its_own_directory_keeps_it() {
        let profile = LaunchSession::Profile {
            name: "Ubuntu work".into(),
            command: "wsl.exe".into(),
            args: ["-d", "Ubuntu", "--cd", "~/work"].map(String::from).to_vec(),
            cwd: None,
            shell_id: None,
        };
        let (launch, _) = new_tab_launch(profile.clone(), guest("Ubuntu"), "/tmp", || None);
        assert_eq!(launch, profile);
        let profile = LaunchSession::Profile {
            name: "Ubuntu".into(),
            command: "wsl.exe".into(),
            args: ["-d", "Ubuntu"].map(String::from).to_vec(),
            cwd: None,
            shell_id: None,
        };
        let (launch, _) = new_tab_launch(profile, guest("Ubuntu"), "/tmp", || None);
        let LaunchSession::Profile { args, .. } = launch else { panic!("profile identity lost") };
        assert_eq!(args, ["--cd", "/tmp", "-d", "Ubuntu"]);
    }

    #[test]
    fn split_of_host_pane_keeps_default_shell_and_host_directory() {
        use crate::gpui_shell::terminal::view::TerminalLaunch;

        let session = LaunchSession::Shell {
            name: "pwsh".into(),
            program: "pwsh.exe".into(),
            args: Vec::new(),
        };
        let host = std::env::temp_dir();
        let TerminalLaunch::Local { cwd, shell, .. } =
            split_launch(&session, None, &host.to_string_lossy(), Some(host.clone()))
        else {
            panic!("a split of a host pane stays local");
        };
        assert!(shell.is_none());
        assert_eq!(cwd, Some(host));
    }
}
