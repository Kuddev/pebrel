//! Native agent executable resolution and terminal-preserving process handoff.
use std::io;
use std::process::Command;

use crate::ai_agents::AgentKind;

pub(crate) fn command(agent: AgentKind, fallback: &str) -> Command {
    #[cfg(windows)]
    {
        if let Some(executable) = crate::ai_hook::integrations::executable(agent) {
            if executable.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("ps1"))
            {
                // -File preserves script argument boundaries; never construct
                // a PowerShell expression from native CLI arguments.
                let powershell = std::env::var_os("SystemRoot")
                    .map(std::path::PathBuf::from)
                    .map(|root| root.join("System32/WindowsPowerShell/v1.0/powershell.exe"))
                    .unwrap_or_else(|| "powershell.exe".into());
                let mut command = Command::new(powershell);
                command.args(["-NoLogo", "-NoProfile", "-File"]).arg(executable);
                return command;
            }
            // Explicit .cmd/.bat paths use std's batch argument escaping.
            // Bare program names only receive an implicit .exe search.
            return Command::new(executable);
        }
    }
    #[cfg(not(windows))]
    let _ = agent;
    Command::new(fallback)
}

/// Unix replaces the launcher; other hosts wait while inheriting its terminal.
/// The caller owns localized error reporting and the fallback exit code.
pub(crate) fn handoff(mut command: Command) -> io::Result<i32> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        Err(command.exec())
    }
    #[cfg(not(unix))]
    {
        command.status().map(|status| status.code().unwrap_or(1))
    }
}
