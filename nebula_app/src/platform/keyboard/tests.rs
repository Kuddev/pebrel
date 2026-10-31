//! Exercise the shared encoder through ConPTY, including both reader contracts.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use nebula_terminal::event::WindowSize;
use nebula_terminal::term::TermMode;
use nebula_terminal::tty::{self, EventedReadWrite};
use windows_sys::Win32::Storage::FileSystem::ReadFile;
use windows_sys::Win32::System::Console::*;

use crate::input::terminal_input::{
    RuntimeKey, RuntimeKeyModifiers, build_runtime_sequence_for_program,
};

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
        pty.writer().write_all(&input).unwrap();
        let result = wait_file(&mut pty, &directory.path().join("result"));
        assert_eq!(result, expected, "{program}: {mode:?}");
    }
}
