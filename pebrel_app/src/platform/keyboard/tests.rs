//! Exercise the shared encoder through ConPTY, including both reader contracts.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use pebrel_terminal::event::WindowSize;
use pebrel_terminal::term::TermMode;
use pebrel_terminal::tty::{self, EventedReadWrite};
use windows_sys::Win32::Storage::FileSystem::ReadFile;
use windows_sys::Win32::System::Console::*;

use crate::input::terminal_input::build_runtime_sequence_for_program;
use crate::runtime_api::{RuntimeKey, RuntimeKeyModifiers};

const FIXTURE: &str = "platform::keyboard::tests::console_reader_fixture";

#[test]
fn console_reader_fixture() {
    let Some(directory) = std::env::var_os("PEBREL_TEST_KEYBOARD_READER") else { return };
    let directory = Path::new(&directory);
    if let Err(error) = read_console(directory) {
        std::fs::write(directory.join("error"), error.to_string()).unwrap();
        panic!("console reader failed: {error}");
    }
}

fn read_console(directory: &Path) -> io::Result<()> {
    // The product test executable is GUI-subsystem. Its parent CMD owns the
    // pseudoconsole; attaching here affects this isolated fixture process only.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    let native = std::env::var("PEBREL_TEST_KEYBOARD_MODE").unwrap() == "native";
    let mode = if native { 0 } else { ENABLE_VIRTUAL_TERMINAL_INPUT };
    if unsafe { SetConsoleMode(input, mode) } == 0 {
        return Err(io::Error::last_os_error());
    }
    std::fs::write(directory.join("ready"), b"ready")?;
    let mut result = Vec::new();
    loop {
        let mut count = 0;
        if native {
            let mut record: INPUT_RECORD = unsafe { std::mem::zeroed() };
            if unsafe { ReadConsoleInputW(input, &mut record, 1, &mut count) } == 0 {
                return Err(io::Error::last_os_error());
            }
            if count == 0 || record.EventType != KEY_EVENT as u16 {
                continue;
            }
            let key = unsafe { record.Event.KeyEvent };
            if key.bKeyDown != 0 && key.wVirtualKeyCode == 13 {
                result = format!(
                    "{}:{}:{}",
                    key.wVirtualKeyCode,
                    unsafe { key.uChar.UnicodeChar },
                    key.dwControlKeyState,
                )
                .into_bytes();
                break;
            }
        } else {
            let mut byte = 0_u8;
            if unsafe { ReadFile(input, &mut byte, 1, &mut count, std::ptr::null_mut()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            if count == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "console input ended"));
            }
            if byte == b'~' {
                break;
            }
            result.push(byte);
        }
    }
    let pending = directory.join("result.tmp");
    std::fs::write(&pending, result)?;
    std::fs::rename(pending, directory.join("result"))
}

fn wait_file(pty: &mut tty::Pty, path: &Path) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut output = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        if let Ok(bytes) = std::fs::read(path) {
            return bytes;
        }
        if let Ok(error) = std::fs::read_to_string(path.with_file_name("error")) {
            panic!("fixture failed: {error}");
        }
        match pty.reader().read(&mut buffer) {
            Ok(count) => output.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
            Err(error) => panic!("ConPTY read failed: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "fixture did not write {path:?}; output: {:?}",
            String::from_utf8_lossy(&output),
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn write_input(writer: &mut impl Write, mut input: &[u8], timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    while !input.is_empty() {
        match writer.write(input) {
            Ok(count) => input = &input[count..],
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {},
            Err(error) => return Err(error),
        }
        if !input.is_empty() {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "ConPTY input remained blocked",
                ));
            }
            // The evented writer reports zero while its asynchronous pipe is busy.
            // Retain the unaccepted suffix; no input or reader assertion is retried.
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    Ok(())
}

#[test]
fn conpty_fixture_write_preserves_partial_input_and_bounds_backpressure() {
    #[derive(Default)]
    struct Backpressure {
        step: usize,
        accepted: Vec<u8>,
    }
    impl Write for Backpressure {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.step += 1;
            match self.step {
                1 => return Ok(0),
                2 => return Err(io::ErrorKind::WouldBlock.into()),
                4 => return Err(io::ErrorKind::Interrupted.into()),
                _ => {},
            }
            let count = bytes.len().min(2);
            self.accepted.extend_from_slice(&bytes[..count]);
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let input = b"\x1b[13;2u~";
    let mut previous = Backpressure::default();
    assert_eq!(previous.write_all(input).unwrap_err().kind(), io::ErrorKind::WriteZero);
    let mut writer = Backpressure::default();
    write_input(&mut writer, input, Duration::from_secs(1)).unwrap();
    assert_eq!(writer.accepted, input);
    let mut stalled = Backpressure::default();
    assert_eq!(
        write_input(&mut stalled, input, Duration::ZERO).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    assert!(stalled.accepted.is_empty());
}

#[test]
fn modified_enter_reaches_native_and_byte_readers_through_conpty() {
    let native = TermMode::WIN32_INPUT_MODE;
    for (program, mode, reader, expected) in [
        ("codex", native, "native", b"13:13:16".as_slice()),
        ("codebuddy", native, "bytes", b"\x1b\r"),
        ("claude", native, "bytes", b"\n"),
        ("codebuddy", native | TermMode::DISAMBIGUATE_ESC_CODES, "bytes", b"\x1b[13;2u"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let executable = std::env::current_exe().unwrap();
        let options = tty::Options {
            shell: Some(tty::Shell::new(
                "cmd.exe".into(),
                vec![format!(
                    "/d /s /c start \"\" /b /wait \"{}\" --exact {FIXTURE} --nocapture",
                    executable.display(),
                )],
            )),
            env: HashMap::from([
                ("PEBREL_TEST_KEYBOARD_READER".into(), directory.path().to_string_lossy().into()),
                ("PEBREL_TEST_KEYBOARD_MODE".into(), reader.into()),
            ]),
            ..Default::default()
        };
        let mut pty = tty::new(
            &options,
            WindowSize { num_lines: 24, num_cols: 80, cell_width: 8, cell_height: 16 },
            0,
        )
        .unwrap();
        wait_file(&mut pty, &directory.path().join("ready"));
        let mut input = build_runtime_sequence_for_program(
            RuntimeKey::Enter,
            RuntimeKeyModifiers { shift: true, ..Default::default() },
            1,
            mode,
            Some(program),
        );
        if reader == "bytes" {
            input.push(b'~');
        }
        write_input(pty.writer(), &input, Duration::from_secs(10)).unwrap();
        let result = wait_file(&mut pty, &directory.path().join("result"));
        assert_eq!(result, expected, "{program}: {mode:?}");
    }
}
