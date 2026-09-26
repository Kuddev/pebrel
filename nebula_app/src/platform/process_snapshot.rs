//! One native process snapshot for close checks and process identity routing.
//! OS reads live here; tree traversal and PID-reuse rules live in process_tree.

#[cfg(windows)]
#[path = "process_snapshot/windows.rs"]
mod windows;

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
    let output = std::process::Command::new("ps")
        .args(["-axo", "pid=,ppid=,comm="])
        .output()
        .map_err(|error| format!("could not launch ps: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ps exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let mut processes = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
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

    #[test]
    fn unix_process_rows_keep_the_complete_command_column() {
        assert_eq!(
            parse_ps_line("  42     7 /Applications/Nebula Preview/bin/zsh"),
            Some((42, 7, "/Applications/Nebula Preview/bin/zsh".to_owned()))
        );
        assert_eq!(parse_ps_line("header"), None);
    }
}
