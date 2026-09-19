//! Console attributes are process-global: each scenario runs in a disposable process.
use std::env;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use windows_sys::Win32::System::Console::{
    AllocConsole, CTRL_C_EVENT, FreeConsole, GenerateConsoleCtrlEvent, SetConsoleCtrlHandler,
};

const TEST: &str = "platform::startup::console_tests::gui_startup_restores_child_ctrl_c";

struct TestChild(Child);

impl Drop for TestChild {
    fn drop(&mut self) {
        // Failure cleanup only: a killed child never counts as an interruption.
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

struct ReadyFile(std::path::PathBuf);

impl Drop for ReadyFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn wait(child: &mut TestChild) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < deadline, "console test child exceeded its deadline");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn gui_startup_restores_child_ctrl_c() {
    let Ok(role) = env::var("PEBREL_CONSOLE_TEST_ROLE") else {
        for detached in [false, true] {
            let mut child = TestChild(
                Command::new(env::current_exe().unwrap())
                    .args(["--exact", TEST, "--nocapture"])
                    .env("PEBREL_CONSOLE_TEST_ROLE", "host")
                    .env("PEBREL_CONSOLE_TEST_DETACHED", detached.to_string())
                    .spawn()
                    .unwrap(),
            );
            assert!(wait(&mut child).success(), "GUI console startup failed (detached={detached})");
        }
        return;
    };
    if role == "target" {
        // The product uses the GUI subsystem, so its test executable must
        // attach explicitly just as main does before it can receive events.
        unsafe {
            windows_sys::Win32::System::Console::AttachConsole(
                windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
        let ready = env::var_os("PEBREL_CONSOLE_TEST_READY").unwrap();
        std::fs::write(ready, b"ready").unwrap();
        thread::sleep(Duration::from_secs(5));
        return;
    }

    // Never broadcast to cargo's or the developer's console.
    unsafe { FreeConsole() };
    assert_ne!(unsafe { AllocConsole() }, 0);
    assert_ne!(unsafe { SetConsoleCtrlHandler(None, 1) }, 0);
    let detached = env::var("PEBREL_CONSOLE_TEST_DETACHED").unwrap() == "true";
    if detached {
        assert_ne!(unsafe { FreeConsole() }, 0);
    }
    super::prepare_console_for_gui().unwrap();
    if detached {
        // The API must also succeed for Explorer/detached launches.
        return;
    }
    let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos();
    let ready_file = ReadyFile(
        env::temp_dir().join(format!("pebrel-ctrl-ready-{}-{stamp}", std::process::id())),
    );
    let ready = &ready_file.0;
    let mut target = TestChild(
        Command::new(env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env("PEBREL_CONSOLE_TEST_ROLE", "target")
            .env("PEBREL_CONSOLE_TEST_READY", &ready)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while !ready.exists() {
        assert!(Instant::now() < deadline, "target did not become ready");
        thread::sleep(Duration::from_millis(20));
    }
    // Shield only the disposable test driver AFTER the child inherited the
    // production startup state. This cannot change the existing child's state.
    assert_ne!(unsafe { SetConsoleCtrlHandler(None, 1) }, 0);
    let started = Instant::now();
    assert_ne!(unsafe { GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0) }, 0);
    let status = wait(&mut target);
    assert_eq!(status.code().map(|code| code as u32), Some(0xc000013a));
    assert!(started.elapsed() < Duration::from_secs(3), "child did not interrupt promptly");
}
