//! Exercise the same Windows PTY and shell bootstrap used by the product.
#![cfg(windows)]

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use nebula_terminal::event::WindowSize;
use nebula_terminal::osc_cwd::{CwdSniffer, OscEvent};
use nebula_terminal::tty::{self, EventedReadWrite, Options, Shell};

#[test]
fn connection_reports_survive_native_conpty_in_execution_order() {
    let shell = tty::powershell_with_nebula_integration(
        "powershell.exe".into(),
        vec!["-NoLogo".into(), "-NoProfile".into()],
    );
    let mut args = shell.args().to_vec();
    // No network or stored credentials: this checks the real transport boundary.
    args.last_mut().unwrap().push_str(
        "; Invoke-PebrelConnection 'cmd.exe' @('/d', '/c', 'exit 7'); \
         [Console]::Write('PEBREL_TRANSPORT_DONE'); exit",
    );
    let options =
        Options { shell: Some(Shell::new(shell.program().into(), args)), ..Options::default() };
    let size = WindowSize { num_lines: 24, num_cols: 100, cell_width: 8, cell_height: 16 };
    let mut pty = tty::new(&options, size, 0).expect("create actual Windows PTY");
    let mut bytes = Vec::new();
    let mut sniffer = CwdSniffer::default();
    let mut reports = Vec::new();
    let mut cursor_replied = false;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        let mut chunk = [0; 8192];
        match pty.reader().read(&mut chunk) {
            Ok(count) if count > 0 => {
                bytes.extend_from_slice(&chunk[..count]);
                reports.extend(sniffer.feed(&chunk[..count]).into_iter().filter_map(
                    |(_, event)| match event {
                        OscEvent::UserVar { name, value } => Some((name, value)),
                        _ => None,
                    },
                ));
                if !cursor_replied && bytes.windows(4).any(|s| s == b"\x1b[6n") {
                    cursor_replied = true;
                    let _ = pty.writer().write_all(b"\x1b[1;1R");
                }
                if bytes.windows(21).any(|s| s == b"PEBREL_TRANSPORT_DONE") {
                    break;
                }
            },
            Ok(_) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            },
            Err(error) => panic!("PTY read failed: {error}"),
        }
    }
    drop(pty);
    assert!(
        bytes.windows(21).any(|s| s == b"PEBREL_TRANSPORT_DONE"),
        "shell did not finish: {}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(reports.len(), 3, "unexpected native PTY reports: {reports:?}");
    assert_eq!(reports[0].0, "pebrel_shell");
    assert_eq!(reports[1].0, "pebrel_connection");
    assert_eq!(reports[2], reports[0], "return must restore the same owner");
    let (owner, argv) = reports[1].1.split_once('\n').unwrap();
    assert_eq!(owner, reports[0].1);
    assert_eq!(argv, "cmd.exe\0/d\0/c\0exit 7");
}
