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

// These run the generated startup and probe scripts through a real POSIX sh.
#[cfg(unix)]
mod startup;
