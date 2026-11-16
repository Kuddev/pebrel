//! Frozen local profile identity and launch request.

pub enum TerminalLaunch {
    Local {
        cwd: Option<std::path::PathBuf>,
        shell: Option<pebrel_terminal::tty::Shell>,
        /// 只用于欢迎屏选择 PowerShell/Bash 形态，不参与 PTY 启动。
        shell_name: Option<String>,
    },
    Profile {
        profile: crate::config::ui_config::Profile,
        cwd: Option<std::path::PathBuf>,
    },
    Command {
        name: String,
        shell: pebrel_terminal::tty::Shell,
        cwd: Option<std::path::PathBuf>,
    },
    Ssh {
        destination: String,
        /// Remote cwd inherited by duplicate-tab. `None` starts in the
        /// account's ordinary login directory.
        cwd: Option<String>,
        resume_target: Option<crate::ssh_session::ResumeTarget>,
    },
    Serial {
        destination: String,
    },
}
