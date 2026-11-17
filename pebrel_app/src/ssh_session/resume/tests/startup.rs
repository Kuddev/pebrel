//! Run the startup script in a real shell.

use super::*;

#[test]
fn vanished_target_or_attach_failure_runs_default_shell_and_cwd_is_never_input() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let probe = Command::new("sh").args(["-c", &probe_command()]).output().unwrap();
    assert!(probe.status.success());
    assert_eq!(probe.stdout, PROBE_REPLY.as_bytes());

    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("client's executable");
    std::fs::write(&binary, "#!/bin/sh\ncase \"$2\" in\ndisplay-message) printf '100:200\\n';;\nselect-window|select-pane) exit ${REJECT_SELECTION:-0};;\nattach-session) printf 'ATTACH_FAILED\\n'; exit 1;;\n*) exit 1;;\nesac\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let shell = directory.path().join("default shell");
    std::fs::write(&shell, "#!/bin/sh\nprintf 'DEFAULT:%s:%s\\n' \"$1\" \"$PWD\"\n").unwrap();
    std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o700)).unwrap();
    let endpoint = Endpoint { executable: binary.to_str().unwrap().into(), socket: String::new() };
    let target =
        ResumeTarget::parse("/dev/pts/3\t$1\t100:200\t@3\t%4", "/dev/pts/3", &endpoint).unwrap();
    for (identity, reject) in [("0:0", "0"), ("100:200", "1"), ("100:200", "0")] {
        let mut target = target.clone();
        target.identity = identity.into();
        let output = Command::new("sh")
            .args(["-c", &startup(TOKEN, None, Some(&target), directory.path().to_str())])
            .env("SHELL", &shell)
            .env("REJECT_SELECTION", reject)
            .output()
            .unwrap();
        assert!(output.status.success());
        let output = String::from_utf8(output.stdout).unwrap();
        assert!(
            output.ends_with(&format!("DEFAULT:-l:{}\n", directory.path().display())),
            "{output}"
        );
        assert_eq!(output.contains("ATTACH_FAILED"), identity == "100:200" && reject == "0");
    }
}

#[test]
fn startup_reports_a_named_terminal_and_sends_no_marker_without_one() {
    use std::fs::File;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};

    let directory = tempfile::tempdir().unwrap();
    let shell = directory.path().join("default shell");
    std::fs::write(&shell, "#!/bin/sh\nprintf 'DEFAULT\\n'\n").unwrap();
    std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (mut controller, mut terminal) = (0, 0);
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut controller,
                &mut terminal,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0
    );
    let _controller = unsafe { File::from_raw_fd(controller) };
    let terminal = unsafe { File::from_raw_fd(terminal) };
    let mut name = [0; 1024];
    assert_eq!(unsafe { libc::ttyname_r(terminal.as_raw_fd(), name.as_mut_ptr(), name.len()) }, 0);
    let name = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }.to_str().unwrap().to_owned();
    // The JumpServer report shows tty failing during startup; null stdin reproduces that.
    for (stdin, tty) in [(Stdio::from(terminal), Some(name)), (Stdio::null(), None)] {
        let output = Command::new("sh")
            .args(["-c", &startup(TOKEN, None, None, None)])
            .env("SHELL", &shell)
            .stdin(stdin)
            .output()
            .unwrap();
        assert!(output.status.success());
        let identity = Arc::new(Mutex::new(None));
        let mut filter = IdentityInput::new(TOKEN, identity.clone());
        let mut visible = filter.feed(&output.stdout).into_owned();
        visible.extend(filter.finish());
        assert_eq!(String::from_utf8(visible).unwrap(), "DEFAULT\n");
        assert_eq!(identity.lock().unwrap().as_ref().map(|value| value.tty.clone()), tty);
    }
}
