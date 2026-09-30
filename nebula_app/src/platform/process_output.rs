//! 短期原生探测的输出与等待上限；仅在后台线程使用。
use std::io::{self, Read as _, Seek as _};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(super) fn read(mut command: Command, timeout: Duration, limit: usize) -> io::Result<Vec<u8>> {
    // 临时文件避免子进程继承 stdout 后让读管道线程永不退出。
    let mut output = tempfile::tempfile()?;
    command.stdin(Stdio::null()).stdout(output.try_clone()?).stderr(Stdio::null());
    let mut child = command.spawn()?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None)
                if Instant::now() < deadline
                    && output.metadata().is_ok_and(|meta| meta.len() <= limit as u64) =>
            {
                std::thread::sleep(Duration::from_millis(10));
            },
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(result.err().unwrap_or_else(|| {
                    io::Error::other("Process probe exceeded its time or output limit")
                }));
            },
        }
    };
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
