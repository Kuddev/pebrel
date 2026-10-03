//! 短期原生探测的输出与等待上限；仅在后台线程使用。
use std::io::{self, Read as _, Seek as _, Write as _};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(super) fn read(command: Command, timeout: Duration, limit: usize) -> io::Result<Vec<u8>> {
    read_cancellable(command, timeout, limit, &|| false)
}

pub(crate) fn read_cancellable(
    command: Command,
    timeout: Duration,
    limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<Vec<u8>> {
    read_with_input(command, &[], timeout, limit, cancelled)
}

/// [`read_cancellable`] with `input` on the child's stdin (none when empty).
pub(crate) fn read_with_input(
    mut command: Command,
    input: &[u8],
    timeout: Duration,
    limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<Vec<u8>> {
    if cancelled() {
        return Err(io::ErrorKind::Interrupted.into());
    }
    let stdin = if input.is_empty() {
        Stdio::null()
    } else {
        let mut stdin = tempfile::tempfile()?;
        stdin.write_all(input)?;
        stdin.rewind()?;
        stdin.into()
    };
    // 临时文件避免子进程继承 stdout 后让读管道线程永不退出。
    let mut output = tempfile::tempfile()?;
    command.stdin(stdin).stdout(output.try_clone()?).stderr(Stdio::null());
    super::process::configure_process_group(&mut command);
    let mut child = command.spawn()?;
    let group = match super::process::ProcessGroup::attach(&child) {
        Ok(group) => group,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        },
    };
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None)
                if !cancelled()
                    && Instant::now() < deadline
                    && output.metadata().is_ok_and(|meta| meta.len() <= limit as u64) =>
            {
                std::thread::sleep(Duration::from_millis(10));
            },
            result => {
                group.terminate(&mut child);
                let _ = child.wait();
                if cancelled() {
                    return Err(io::ErrorKind::Interrupted.into());
                }
                return Err(result.err().unwrap_or_else(|| {
                    io::Error::other("Process probe exceeded its time or output limit")
                }));
            },
        }
    };
    group.finish();
    if cancelled() {
        return Err(io::ErrorKind::Interrupted.into());
    }
    if !status.success() {
        return Err(io::Error::other(format!("Process probe exited with {status}")));
    }
    output.rewind()?;
    let mut bytes = Vec::new();
    output.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shell running a script: `cmd /c` on Windows, `sh -c` elsewhere.
    fn shell(windows: &str, unix: &str) -> Command {
        let mut command = Command::new(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
        if cfg!(windows) {
            command.args(["/D", "/C", windows]);
        } else {
            command.args(["-c", unix]);
        }
        command
    }

    #[test]
    fn probes_bound_output_and_reap_timed_out_children() {
        let ok = read(shell("<nul set /p =ok& exit /b 0", "printf ok"), Duration::from_secs(20), 2);
        assert_eq!(ok.unwrap(), b"ok");
        let long = read(shell("echo too-long", "printf too-long"), Duration::from_secs(20), 2);
        assert!(long.is_err());
        assert!(read(shell("exit /b 7", "exit 7"), Duration::from_secs(20), 2).is_err());
        let started = Instant::now();
        let slow = shell("ping -n 30 127.0.0.1 >nul", "exec sleep 20");
        assert!(read(slow, Duration::from_millis(300), 2).is_err());
        assert!(started.elapsed() < Duration::from_secs(15), "the child is killed, not awaited");
    }

    #[test]
    fn input_reaches_the_child_stdin() {
        let input = b"shell=/usr/bin/zsh\n";
        let echoed =
            read_with_input(shell("more", "cat"), input, Duration::from_secs(20), 1024, &|| false);
        let echoed = String::from_utf8(echoed.unwrap()).unwrap();
        assert!(echoed.contains("shell=/usr/bin/zsh"), "{echoed:?}");
    }

    #[test]
    fn cancelled_probe_reaps_its_child_on_each_desktop_platform() {
        #[cfg(windows)]
        let mut command = Command::new("powershell.exe");
        #[cfg(windows)]
        command.args(["-NoProfile", "-NonInteractive", "-Command", "Start-Sleep -Seconds 20"]);
        #[cfg(unix)]
        let mut command = Command::new("/bin/sh");
        #[cfg(unix)]
        command.args(["-c", "exec sleep 20"]);
        let checks = std::cell::Cell::new(0);
        let error = read_cancellable(command, Duration::from_secs(5), 1024, &|| {
            checks.set(checks.get() + 1);
            checks.get() > 1
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(checks.get() > 1, "cancellation occurs after process creation");
    }
}
