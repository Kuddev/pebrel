//! Read-only guest commands retain the existing process/channel cancellation owners.
use super::*;

/// One captured execution boundary used for discovery, system counters and optional tools.
pub(super) enum Transport {
    /// Frozen distribution, user and environment from the WSL pane.
    Wsl(PaneExecContext),
    /// Already authenticated, unambiguous SSH connection generation.
    Ssh(crate::ssh_session::completion::Connection),
}

impl Transport {
    /// Identify the guest from its own native tools; the desktop platform is irrelevant.
    /// Windows PowerShell discovery also covers Windows hosts whose PATH contains Unix tools.
    pub(super) fn discover(&self, cancelled: &dyn Fn() -> bool) -> Result<GuestOs, String> {
        if matches!(self, Self::Wsl(_)) {
            return Ok(GuestOs::Linux);
        }
        if let Ok(bytes) = self.read("uname -s", "", Duration::from_secs(2), 65536, cancelled) {
            match String::from_utf8_lossy(&bytes).trim() {
                "Linux" => return Ok(GuestOs::Linux),
                "Darwin" => return Ok(GuestOs::Macos),
                _ => {},
            }
        }
        if cancelled() {
            return Err("Resource discovery cancelled".into());
        }
        let bytes = self
            .script(
                GuestOs::Windows,
                "if ($env:OS -ne 'Windows_NT') { exit 1 }; [Console]::WriteLine('PebrelWindows')\n",
                Duration::from_secs(3),
                65536,
                cancelled,
            )
            .map_err(|error| format!("Unsupported guest or system shell unavailable: {error}"))?;
        if String::from_utf8_lossy(&bytes).trim() == "PebrelWindows" {
            Ok(GuestOs::Windows)
        } else {
            Err("Guest OS could not be identified".into())
        }
    }

    /// Execute embedded source through the identified system shell under caller-owned limits.
    /// POSIX probes enter sh explicitly, independent of the account's interactive shell.
    pub(super) fn script(
        &self,
        os: GuestOs,
        script: &str,
        budget: Duration,
        limit: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<u8>, String> {
        let command =
            if os == GuestOs::Windows { powershell_command() } else { "/bin/sh -s".into() };
        self.read(&command, script, budget, limit, cancelled)
    }

    /// Reuse native child ownership or a private SSH channel; never write terminal input.
    fn read(
        &self,
        command: &str,
        script: &str,
        budget: Duration,
        limit: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<u8>, String> {
        if budget.is_zero() || cancelled() {
            return Err("Resource query cancelled or timed out".into());
        }
        match self {
            Self::Wsl(context) => {
                let argv = ["/bin/sh".to_owned(), "-c".into(), script.into()];
                let (command, _) = crate::runtime_exec::build_command(context, "/", &argv)
                    .map_err(|error| format!("{error:?}"))?;
                crate::platform::process_output::read_cancellable(command, budget, limit, cancelled)
                    .map_err(|error| error.to_string())
            },
            Self::Ssh(connection) => {
                let runtime = crate::ssh_session::runtime().map_err(|error| error.to_string())?;
                runtime
                    .block_on(crate::ssh_session::completion::read_connection(
                        connection,
                        command,
                        script.as_bytes(),
                        budget,
                        limit,
                        cancelled,
                    ))
                    .map_err(|error| error.to_string())
            },
        }
    }
}
