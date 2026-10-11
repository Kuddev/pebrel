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
            LaunchSession::Shell { name, program, args } => TerminalLaunch::Local {
                cwd,
                shell: Some(nebula_terminal::tty::Shell::new(program.clone(), args.clone())),
                shell_name: Some(name.clone()),
            },
            LaunchSession::Profile { .. } => {
                let profile = crate::profile_launch::resolve(launch).expect("profile variant");
                TerminalLaunch::Profile { cwd: profile.cwd.clone().or(cwd), profile }
            },
            LaunchSession::Ssh { host } => {
                TerminalLaunch::Ssh { destination: host.clone(), cwd: None }
            },
        }
    }
}
