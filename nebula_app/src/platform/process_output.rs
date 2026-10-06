//! 短期原生探测的输出与等待上限；仅在后台线程使用。
use std::io::{self, Read as _, Seek as _};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(super) fn read(command: Command, timeout: Duration, limit: usize) -> io::Result<Vec<u8>> {
    read_cancellable(command, timeout, limit, &|| false)
}

pub(crate) fn read_cancellable(
    mut command: Command,
    timeout: Duration,
    limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<Vec<u8>> {
    command.stdin(Stdio::null());
    read_cancellable_with_stdin(command, timeout, limit, cancelled)
}

pub(crate) fn read_cancellable_with_stdin(
    mut command: Command,
    timeout: Duration,
    limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<Vec<u8>> {
    if cancelled() {
        return Err(io::ErrorKind::Interrupted.into());
    }
    // 临时文件避免子进程继承 stdout 后让读管道线程永不退出。
    let mut output = tempfile::tempfile()?;
    command.stdout(output.try_clone()?).stderr(Stdio::null());
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn probes_bound_output_and_reap_timed_out_children() {
        let command = |script: &str| {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", script]);
            command
        };
        assert_eq!(read(command("printf ok"), Duration::from_secs(2), 2).unwrap(), b"ok");
        assert!(read(command("printf too-long"), Duration::from_secs(2), 2).is_err());
        assert!(read(command("exit 7"), Duration::from_secs(2), 2).is_err());
        assert!(read(command("exec sleep 20"), Duration::from_millis(50), 2).is_err());
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;

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
