//! Bounded local service-manager calls. Output is either discarded or consumed
//! only to detect the systemd version; it never enters progress/error messages.

use std::{
    io::{self, Read},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const MAX_OUTPUT: u64 = 8 * 1024;

pub(super) fn run(program: &str, args: &[&str], capture: bool) -> io::Result<Vec<u8>> {
    run_with_deadline(program, args, capture, Duration::from_secs(12))
}

fn run_with_deadline(
    program: &str,
    args: &[&str],
    capture: bool,
    timeout: Duration,
) -> io::Result<Vec<u8>> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(if capture { Stdio::piped() } else { Stdio::null() })
        .stderr(Stdio::null())
        .spawn()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Err(io::Error::other("service_command_failed"));
                }
                let mut output = Vec::new();
                if let Some(stdout) = child.stdout.take() {
                    // Only /usr/bin/systemctl --version is captured. A command
                    // exceeding pipe capacity cannot block the deadline loop.
                    stdout.take(MAX_OUTPUT + 1).read_to_end(&mut output)?;
                }
                return if output.len() as u64 <= MAX_OUTPUT {
                    Ok(output)
                } else {
                    Err(io::Error::other("service_command_output_limit"))
                };
            },
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(20));
            },
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match result {
                    Err(error) => error,
                    _ => io::Error::new(io::ErrorKind::TimedOut, "service_command_timeout"),
                });
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_command_does_not_include_its_output() {
        #[cfg(windows)]
        let result = run_with_deadline(
            "C:/Windows/System32/cmd.exe",
            &["/D", "/C", "echo sensitive-output & exit /b 7"],
            true,
            Duration::from_secs(3),
        );
        #[cfg(not(windows))]
        let result = run_with_deadline(
            "/bin/sh",
            &["-c", "echo sensitive-output; exit 7"],
            true,
            Duration::from_secs(3),
        );
        assert_eq!(result.unwrap_err().to_string(), "service_command_failed");
    }

    #[test]
    fn hung_command_is_terminated_with_a_bounded_error() {
        #[cfg(windows)]
        let (program, args) = ("C:/Windows/System32/ping.exe", vec!["-n", "30", "127.0.0.1"]);
        #[cfg(not(windows))]
        let (program, args) = ("/bin/sleep", vec!["30"]);
        let started = Instant::now();
        let result = run_with_deadline(program, &args, false, Duration::from_millis(60));
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
