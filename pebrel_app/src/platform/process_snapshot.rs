//! One native process snapshot for close checks and process identity routing.
//! OS reads live here; tree traversal and PID-reuse rules live in process_tree.

#[cfg(windows)]
#[path = "process_snapshot/windows.rs"]
mod windows;

#[cfg(all(test, target_os = "linux"))]
pub(crate) mod test_pty;

/// Native process identity at one snapshot instant. A zero creation time means
/// that the platform could not provide it; consumers must preserve that edge.
pub(crate) struct ProcessRow {
    pub(crate) pid: u32,
    pub(crate) parent: u32,
    pub(crate) executable: String,
    pub(crate) created: u64,
}

pub(crate) fn snapshot() -> Result<Vec<ProcessRow>, String> {
    #[cfg(windows)]
    {
        windows::snapshot()
    }
    #[cfg(not(windows))]
    {
        unix_snapshot()
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LinuxTerminal {
    session: u32,
    device: i32,
}

#[cfg(target_os = "linux")]
impl LinuxTerminal {
    pub(crate) fn is_attached(self) -> bool {
        self.device != 0
    }
}

#[cfg(target_os = "linux")]
pub(crate) struct LinuxProcess {
    pub(crate) parent: u32,
    pub(crate) name: Option<String>,
    pub(crate) terminal: LinuxTerminal,
}

/// Read one process's parent and terminal from the same kernel record.
#[cfg(target_os = "linux")]
pub(crate) fn linux_process(pid: u32) -> Option<LinuxProcess> {
    parse_linux_process(&std::fs::read(format!("/proc/{pid}/stat")).ok()?)
}

#[cfg(target_os = "linux")]
fn parse_linux_process(stat: &[u8]) -> Option<LinuxProcess> {
    // The comm field can contain spaces, closing parentheses or non-UTF-8 bytes.
    let start_of_comm = stat.iter().position(|byte| *byte == b'(')?;
    let end_of_comm = stat.iter().rposition(|byte| *byte == b')')?;
    let name =
        std::str::from_utf8(stat.get(start_of_comm + 1..end_of_comm)?).ok().map(str::to_owned);
    let mut fields =
        std::str::from_utf8(stat.get(end_of_comm + 1..)?).ok()?.split_ascii_whitespace();
    // Fields after comm: state, parent PID, group, session, tty_nr.
    let parent = fields.nth(1)?.parse().ok()?;
    let session = fields.nth(1)?.parse().ok()?;
    let device = fields.next()?.parse().ok()?;
    Some(LinuxProcess { parent, name, terminal: LinuxTerminal { session, device } })
}

#[cfg(unix)]
pub(crate) fn is_codex_process_name(name: &str) -> bool {
    matches!(name, "codex" | "codex.exe") || name.starts_with("codex-")
}

/// Parse the stable three-column `ps` output used by Unix snapshots. The
/// command column is the remainder so executable paths containing spaces are
/// not split into a fake fourth field.
#[allow(dead_code)]
fn parse_ps_line(line: &str) -> Option<(u32, u32, String)> {
    fn field(input: &str) -> Option<(&str, &str)> {
        let input = input.trim_start();
        let end = input.find(char::is_whitespace).unwrap_or(input.len());
        let value = &input[..end];
        (!value.is_empty()).then(|| (value, &input[end..]))
    }

    let (pid, rest) = field(line)?;
    let (parent_pid, executable) = field(rest)?;
    let executable = executable.trim();
    if executable.is_empty() {
        return None;
    }
    Some((pid.parse().ok()?, parent_pid.parse().ok()?, executable.to_owned()))
}

/// Unix has no Toolhelp equivalent shared by Linux and macOS. `ps` is part of
/// both base systems and this path only runs for explicit process inspection
/// or a throttled identity check, never on the render loop.
#[cfg(not(windows))]
fn unix_snapshot() -> Result<Vec<ProcessRow>, String> {
    let mut command = std::process::Command::new("/bin/ps");
    command.args(["-axo", "pid=,ppid=,comm="]);
    let output =
        super::process_output::read(command, std::time::Duration::from_secs(1), 8 * 1024 * 1024)
            .map_err(|error| format!("could not launch ps: {error}"))?;

    let mut processes = Vec::new();
    for line in String::from_utf8_lossy(&output).lines() {
        if let Some((pid, parent_pid, executable)) = parse_ps_line(line) {
            processes.push(ProcessRow { pid, parent: parent_pid, executable, created: 0 });
        }
    }
    if processes.is_empty() {
        return Err("ps returned no process rows".to_owned());
    }
    Ok(processes)
}

#[cfg(test)]
mod tests {
    use super::parse_ps_line;

    #[cfg(target_os = "linux")]
    #[test]
    fn controlling_terminal_distinguishes_kernel_session_and_device() {
        use super::{LinuxTerminal, parse_linux_process};

        let attached = parse_linux_process(b"41 (my ) process \xff) S 9 41 9 34818 41").unwrap();
        assert_eq!(attached.parent, 9);
        assert!(attached.name.is_none());
        assert_eq!(attached.terminal, LinuxTerminal { session: 9, device: 34818 });
        assert!(attached.terminal.is_attached());
        let detached = parse_linux_process(b"42 (codex) S 41 42 42 0 -1").unwrap();
        assert_eq!(detached.name.as_deref(), Some("codex"));
        assert!(!detached.terminal.is_attached());
        assert!(parse_linux_process(b"42 (codex) S 41 42").is_none());
    }

    #[test]
    fn unix_process_rows_keep_the_complete_command_column() {
        assert_eq!(
            parse_ps_line("  42     7 /Applications/Nebula Preview/bin/zsh"),
            Some((42, 7, "/Applications/Nebula Preview/bin/zsh".to_owned()))
        );
        assert_eq!(parse_ps_line("header"), None);
    }
}
