//! Frozen local profile identity and launch request.

pub enum TerminalLaunch {
    Local {
        cwd: Option<std::path::PathBuf>,
        shell: Option<nebula_terminal::tty::Shell>,
        /// 只用于欢迎屏选择 PowerShell/Bash 形态，不参与 PTY 启动。
        shell_name: Option<String>,
    },
    Profile {
        profile: crate::config::ui_config::Profile,
        cwd: Option<std::path::PathBuf>,
    },
    Ssh {
        destination: String,
        /// Remote cwd inherited by duplicate-tab. `None` starts in the
        /// account's ordinary login directory.
        cwd: Option<String>,
    },
}
