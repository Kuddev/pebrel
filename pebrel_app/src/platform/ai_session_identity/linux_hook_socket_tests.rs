//! Linux hook ownership requires real process sessions and a controlling PTY.

#![cfg(target_os = "linux")]

use super::*;

#[test]
fn codex_hook_requires_the_launchers_controlling_terminal() {
    use std::os::unix::fs::symlink;
    use std::os::unix::process::CommandExt as _;
    use std::process::{Command, Stdio};

    const ENDPOINT: &str = "PEBREL_TEST_CODEX_SOCKET";
    const HELPER: &str = "PEBREL_TEST_CODEX_HELPER";
    const TEST: &[&str] = &[
        "--exact",
        "ai_hook::unix::linux_hook_socket_tests::codex_hook_requires_the_launchers_controlling_terminal",
        "--nocapture",
    ];
    fn detach(command: &mut Command) {
        // SAFETY: setsid is async-signal-safe and only changes the fixture child.
        unsafe {
            command.pre_exec(|| {
                (libc::setsid() >= 0).then_some(()).ok_or_else(io::Error::last_os_error)
            });
        }
    }

    if let Some(endpoint) = std::env::var_os(ENDPOINT) {
        if let Some(helper) = std::env::var_os(HELPER) {
            // Real native hooks open their own process session in either mode.
            let mut child = Command::new(helper);
            child.args(TEST).env_remove(HELPER);
            detach(&mut child);
            assert!(child.status().unwrap().success());
            return;
        }
        let mut stream = UnixStream::connect(endpoint).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        stream.write_all(
            b"nebula-hook/1 source=codex pane=42 codex_hooks=full\n{\"hook_event_name\":\"SessionStart\",\"session_id\":\"other-pane\"}",
        ).unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        let mut ack = [0];
        stream.read_exact(&mut ack).unwrap();
        assert_eq!(ack, [b'\n']);
        eprintln!("read detached hook acknowledgement");
        return;
    }

    let (tx, rx) = mpsc::sync_channel(1);
    let server = start(tx, None).unwrap();
    let fixture = tempfile::tempdir().unwrap();
    let program = fixture.path().join("codex");
    let helper = std::env::current_exe().unwrap();
    symlink(&helper, &program).unwrap();
    for attached in [false, true] {
        let mut command = Command::new(&program);
        command
            .args(TEST)
            .env(ENDPOINT, &server.endpoint)
            .env(HELPER, &helper)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let _master = if attached {
            Some(crate::platform::process_snapshot::test_pty::attach(&mut command).unwrap())
        } else {
            detach(&mut command);
            None
        };
        let child = command.spawn().unwrap();
        let owner = child.id();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("read detached hook acknowledgement")
        );
        if attached {
            let event = rx.try_recv().unwrap();
            assert_eq!(event.agent_pid, Some(owner));
            assert_eq!(event.pane, Some(42));
        } else {
            assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        }
    }
}
