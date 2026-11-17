use super::*;

#[test]
fn bare_port_defaults_to_115200_8n1() {
    let config: SerialConfig = "serial:COM3".parse().unwrap();
    assert_eq!(
        config,
        SerialConfig {
            port: "COM3".into(),
            baud: 115_200,
            data_bits: 8,
            parity: Parity::None,
            stop_bits: 1,
        }
    );
}

#[test]
fn baud_and_frame_override_defaults() {
    let config: SerialConfig = "serial:/dev/ttyUSB0,9600,7e2".parse().unwrap();
    assert_eq!(
        config,
        SerialConfig {
            port: "/dev/ttyUSB0".into(),
            baud: 9600,
            data_bits: 7,
            parity: Parity::Even,
            stop_bits: 2,
        }
    );
}

#[test]
fn invalid_destinations_are_rejected() {
    for destination in [
        "COM3",
        "serial:",
        "serial:COM3,fast",
        "serial:COM3,9600,9N1",
        "serial:COM3,9600,8M1",
        "serial:COM3,9600,8N1,x",
    ] {
        assert!(destination.parse::<SerialConfig>().is_err(), "{destination}");
    }
}

#[test]
fn display_name_drops_options() {
    assert_eq!(display_name("serial:COM3,9600"), "COM3");
}

#[test]
fn serial_validation_keeps_ssh_routes_separate() {
    use crate::ssh_profiles::{SshConnectionOptions, SshHostJumpMode};
    let options = SshConnectionOptions::default();
    assert!(options.validate("serial:COM3,115200,8N1").is_ok());
    assert!(options.validate("dev@example.com:2222").is_ok());
    assert!(options.validate("serial:COM3,0,8N1").is_err());
    assert!(options.validate("serial:/dev/tty\nUSB0").is_err());
    let jump = SshConnectionOptions {
        jump_mode: SshHostJumpMode::Host,
        jump_host: "serial:COM3".into(),
        ..options.clone()
    };
    assert!(jump.validate("example.com").is_err());
    assert!(jump.validate("serial:COM3").is_err());
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::fs::File;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::time::Instant;

    #[derive(Clone)]
    struct Events;
    impl EventListener for Events {
        fn send_event(&self, _: TerminalEvent) {}
    }

    fn open() -> (File, EventLoopSender, Arc<FairMutex<Term<Events>>>) {
        let (mut master, mut slave) = (0, 0);
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0
        );
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        let mut name = [0; 1024];
        assert_eq!(unsafe { libc::ttyname_r(slave.as_raw_fd(), name.as_mut_ptr(), name.len()) }, 0);
        let path = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) }.to_str().unwrap();
        let size = WindowSize { num_cols: 80, num_lines: 24, cell_width: 8, cell_height: 16 };
        let terminal = Arc::new(FairMutex::new(Term::new(Default::default(), &size, Events)));
        let sender =
            spawn_session(&format!("serial:{path}"), Arc::clone(&terminal), Events).unwrap();
        (master, sender, terminal)
    }

    fn wait_closed(terminal: &Arc<FairMutex<Term<Events>>>) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Arc::strong_count(terminal) != 1 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(Arc::strong_count(terminal), 1, "both serial workers must release the session");
    }

    #[test]
    fn shutdown_and_sender_drop_cancel_serial_backpressure() {
        for explicit_shutdown in [true, false] {
            let (master, sender, terminal) = open();
            sender.send(Msg::Input(vec![b'x'; 1024 * 1024].into())).unwrap();
            // Wait for real progress, then leave the peer undrained. A blocking
            // write_all cannot finish the payload; shutdown must cancel it.
            let mut fd = libc::pollfd { fd: master.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            assert_eq!(unsafe { libc::poll(&mut fd, 1, 2000) }, 1);
            if explicit_shutdown {
                sender.send(Msg::Shutdown).unwrap();
            }
            drop(sender);
            wait_closed(&terminal);
            drop(master);
        }
    }

    #[test]
    fn serial_partial_writes_preserve_all_bytes_in_order() {
        use std::io::Read;
        let (mut master, sender, terminal) = open();
        let expected: Vec<u8> = (0..96 * 1024).map(|n| (n % 251) as u8).collect();
        for part in expected.chunks(17000) {
            sender.send(Msg::Input(part.to_vec().into())).unwrap();
        }
        let mut received = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(4);
        while received.len() < expected.len() && Instant::now() < deadline {
            let mut fd = libc::pollfd { fd: master.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            if unsafe { libc::poll(&mut fd, 1, 100) } > 0 {
                let mut bytes = [0; 997];
                let n = master.read(&mut bytes).unwrap();
                received.extend_from_slice(&bytes[..n]);
            }
        }
        sender.send(Msg::Shutdown).unwrap();
        wait_closed(&terminal);
        assert_eq!(received, expected);
    }

    #[test]
    fn serial_peer_disconnect_releases_workers_without_shutdown() {
        let (master, _sender, terminal) = open();
        drop(master);
        wait_closed(&terminal);
    }
}
