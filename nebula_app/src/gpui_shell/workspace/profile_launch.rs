//! Restore and spawn mapping shared by tab creation and recovery.

use super::*;

impl NebulaWorkspace {
    /// 把一份冻结的会话 launch 还原为一次 GPUI PTY 启动。逐 pane 选择
    /// 与旧快照回退由 session_recovery 统一负责。
    pub(super) fn terminal_launch_from_session(
        launch: &crate::session::LaunchSession,
        cwd: Option<std::path::PathBuf>,
    ) -> crate::gpui_shell::terminal::view::TerminalLaunch {
        use crate::gpui_shell::terminal::view::TerminalLaunch;
        use crate::session::LaunchSession;

        match launch {
            LaunchSession::Default => TerminalLaunch::Local { cwd, shell: None, shell_name: None },
            LaunchSession::Shell { name, program, args } => {
                let own_directory = crate::shell_detect::wsl_launch(program, args)
                    .is_some_and(|options| options.chooses_directory);
                TerminalLaunch::Local {
                    cwd: (!own_directory).then_some(cwd).flatten(),
                    shell: Some(nebula_terminal::tty::Shell::new(program.clone(), args.clone())),
                    shell_name: Some(name.clone()),
                }
            },
            LaunchSession::Profile { .. } => {
                let profile = crate::profile_launch::resolve(launch).expect("profile variant");
                let own_guest_directory =
                    crate::shell_detect::wsl_launch(&profile.command, &profile.args)
                        .is_some_and(|options| options.chooses_directory);
                let cwd =
                    profile.cwd.clone().or_else(|| (!own_guest_directory).then_some(cwd).flatten());
                TerminalLaunch::Profile { cwd, profile }
            },
            LaunchSession::Ssh { host } => {
                TerminalLaunch::Ssh { destination: host.clone(), cwd: None }
            },
        }
    }
}

// New launch requests preserve the difference between an explicit directory
// and a host fallback; WSL can own its guest startup directory.
impl NebulaWorkspace {
    pub(super) fn add_terminal_at(
        &mut self,
        cwd: Option<std::path::PathBuf>,
        command: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> u64 {
        // 默认 shell 只在“创建新 Tab”的这一刻取样，并把实际 program/args
        // 一起冻结进 Tab launch。设置页随后改默认值只影响下一次创建；冷
        // 恢复也按本 Tab 的 launch 重建，不会把混合工作区抹成同一种 shell。
        let launch_session = shell_launch::configured_local_launch(cx);
        let launch_session = shell_launch::at_explicit_directory(launch_session, cwd.as_deref());
        self.add_terminal_with(launch_session, cwd, command, window, cx)
    }

    pub(super) fn launch_palette_shell(
        &mut self,
        detected: crate::shell_detect::DetectedShell,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_palette_state();
        let startup = Self::startup_directory();
        let shell = detected.shell_with_starting_directory(startup.is_some());
        let launch = crate::session::LaunchSession::Shell {
            name: detected.name.clone(),
            program: shell.program().to_owned(),
            args: shell.args().to_vec(),
        };
        let cwd = startup.or_else(|| {
            self.tabs
                .get(self.active)
                .and_then(WorkspaceTab::focused_view)
                .and_then(|view| view.read(cx).local_cwd())
        });
        self.add_terminal_with(launch, cwd, None, window, cx);
    }

    pub(super) fn launch_palette_profile(
        &mut self,
        mut profile: crate::config::ui_config::Profile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_palette_state();
        let startup = Self::startup_directory();
        profile.cwd = profile.cwd.or_else(|| startup.clone());
        let launch = shell_launch::profile_launch_session(profile);
        let cwd = startup.or_else(|| {
            self.tabs
                .get(self.active)
                .and_then(WorkspaceTab::focused_view)
                .and_then(|view| view.read(cx).local_cwd())
        });
        self.add_terminal_with(launch, cwd, None, window, cx);
    }

    pub(super) fn add_cold_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let launch = shell_launch::configured_local_launch(cx);
        self.add_terminal_with(launch, Self::cold_start_cwd(), None, window, cx);
    }
}
