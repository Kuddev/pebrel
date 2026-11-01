use super::*;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

#[test]
fn marker_survives_every_packet_boundary_without_changing_banners_or_terminal_bytes() {
    let marker = format!("\x1ePEBREL_PTY_{TOKEN}=/dev/pts/3:42\x1f");
    let bytes = format!("banner\r\n{marker}\x1b[?1049h正文\r\n").into_bytes();
    for chunk_size in 1..=bytes.len() {
        let identity = Arc::new(Mutex::new(None));
        let mut filter = IdentityInput::new(TOKEN, identity.clone());
        let mut output = Vec::new();
        for chunk in bytes.chunks(chunk_size) {
            output.extend_from_slice(&filter.feed(chunk));
        }
        output.extend(filter.finish());
        assert_eq!(output, "banner\r\n\x1b[?1049h正文\r\n".as_bytes());
        assert_eq!(identity.lock().unwrap().as_ref().unwrap().tty, "/dev/pts/3");
        assert!(matches!(filter.feed(b"later"), std::borrow::Cow::Borrowed(_)));
    }
}

#[test]
fn foreign_invalid_partial_and_oversized_markers_remain_byte_exact() {
    for bytes in [
        b"\x1ePEBREL_PTY_other=/dev/pts/3:42\x1f".to_vec(),
        format!("\x1ePEBREL_PTY_{TOKEN}=/dev/pts/3:0\x1f").into_bytes(),
        format!("\x1ePEBREL_PTY_{TOKEN}=/dev/pts/3;evil:42\x1f").into_bytes(),
        format!("\x1ePEBREL_PTY_{TOKEN}=/dev/").into_bytes(),
        format!("\x1ePEBREL_PTY_{TOKEN}={}", "a".repeat(9000)).into_bytes(),
    ] {
        let identity = Arc::new(Mutex::new(None));
        let mut filter = IdentityInput::new(TOKEN, identity.clone());
        let mut output = Vec::new();
        for chunk in bytes.chunks(3) {
            output.extend_from_slice(&filter.feed(chunk));
        }
        output.extend(filter.finish());
        assert_eq!(output, bytes);
        assert!(identity.lock().unwrap().is_none());
    }
}

#[test]
fn only_a_still_owned_pty_can_confirm_intentional_detach() {
    let identity = Arc::new(Mutex::new(None));
    let mut filter = IdentityInput::new(TOKEN, identity.clone());
    let _ = filter.feed(format!("\x1ePEBREL_PTY_{TOKEN}=/dev/pts/3:42\x1f").as_bytes());
    let identity = identity.lock().unwrap().clone().unwrap();
    assert_eq!(identity.has_client("42 pts/3 bash\n43 pts/3 tmux: client\n"), Some(true));
    assert_eq!(identity.has_client("42 pts/3 bash\n"), Some(false));
    assert_eq!(identity.has_client("99 pts/3 bash\n"), None);
    assert_eq!(identity.has_client("42 pts/4 bash\n"), None);
    assert_eq!(identity.has_client(""), None);
}

#[test]
fn pooled_ptys_and_reused_names_cannot_select_another_target() {
    let endpoint = Endpoint::default();
    let rows = "/dev/pts/3\t$1\t100:200\t@3\t%4\n/dev/pts/4\t$2\t101:200\t@5\t%6\n";
    let a = ResumeTarget::parse(rows, "/dev/pts/3", &endpoint).unwrap();
    let b = ResumeTarget::parse(rows, "/dev/pts/4", &endpoint).unwrap();
    assert_eq!(a.session, "$1");
    assert_eq!(b.session, "$2");
    assert!(ResumeTarget::parse(rows, "/dev/pts/5", &endpoint).is_none());
    assert!(ResumeTarget::parse(&format!("{rows}{rows}"), "/dev/pts/3", &endpoint).is_none());
    for fields in ["$1;evil\t100:200\t@3\t%4", "$1\t100:x\t@3\t%4", "$1\t100:200\t@3\t%4;evil"] {
        assert!(
            ResumeTarget::parse(&format!("/dev/pts/3\t{fields}"), "/dev/pts/3", &endpoint)
                .is_none()
        );
    }
}

#[test]
fn endpoint_metadata_is_bounded_validated_and_literal() {
    let endpoints = Endpoint::parse(
        "/opt/it's binary\t/tmp/it's socket\nrelative\t/tmp/s\n/bin/p\trelative\n/bin/p\t/tmp/a\textra\n",
    );
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].command(), "'/opt/it'\"'\"'s binary' -u -S '/tmp/it'\"'\"'s socket'");
    assert_eq!(
        Endpoint::parse(&(0..100).map(|n| format!("/bin/p\t/tmp/{n}\n")).collect::<String>()).len(),
        32
    );
}

#[cfg(unix)]
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

#[cfg(unix)]
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
