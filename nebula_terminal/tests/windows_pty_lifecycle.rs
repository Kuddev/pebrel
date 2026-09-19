//! Run in its own test process: handle counts must not include other tests.
#![cfg(windows)]

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};
use std::{env, fs, process::Command};

use nebula_terminal::event::WindowSize;
use nebula_terminal::tty::{self, Options, Shell};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};

const SIZE: WindowSize = WindowSize { num_lines: 24, num_cols: 80, cell_width: 8, cell_height: 16 };

fn handle_count() -> u32 {
    let mut count = 0;
    assert_ne!(unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) }, 0);
    count
}

fn options(program: &str, args: &[&str]) -> Options {
    Options {
        shell: Some(Shell::new(program.into(), args.iter().map(|arg| (*arg).into()).collect())),
        ..Options::default()
    }
}

fn close_pty(options: &Options) {
    let pty = tty::new(options, SIZE, 0).expect("create native ConPTY");
    // Let the host and shell start, including the busy shell used below. Do not
    // consume its output: closing must drain a backed-up pipe by itself.
    thread::sleep(Duration::from_millis(300));
    drop(pty);
}

fn assert_handles_return(baseline: u32, phase: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let current = handle_count();
        // Allow two process-wide lazy wait-pool handles, not a per-PTY allowance.
        if current <= baseline + 2 {
            eprintln!("{phase}: {current} handles (warm baseline {baseline})");
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{phase}: native handles leaked: {baseline} -> {current}"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn bundled_conpty_resources_return_after_close_and_failed_spawn() {
    if env::var_os("PEBREL_PTY_LIFECYCLE_CHILD").is_none() {
        run_with_bundled_host();
        return;
    }
    // A drop deadlock must fail the test instead of hanging cargo indefinitely.
    let (finished, result) = mpsc::channel();
    thread::spawn(move || {
        let outcome = std::panic::catch_unwind(|| {
            let idle = options("cmd.exe", &["/d", "/q", "/k"]);
            for _ in 0..3 {
                close_pty(&idle);
            }
            thread::sleep(Duration::from_millis(500));
            let baseline = handle_count();

            for _ in 0..8 {
                close_pty(&idle);
            }
            assert_handles_return(baseline, "idle tab close");

            let busy = options(
                "cmd.exe",
                &[
                    "/d",
                    "/q",
                    "/c",
                    "for /L %i in (1,1,2147483647) do @echo 0123456789012345678901234567890123456789",
                ],
            );
            for _ in 0..8 {
                close_pty(&busy);
            }
            assert_handles_return(baseline, "busy tab close");

            let missing = options("__pebrel_missing_shell_memory_regression__.exe", &[]);
            for _ in 0..8 {
                assert!(tty::new(&missing, SIZE, 0).is_err());
            }
            assert_handles_return(baseline, "failed shell spawn");
        });
        finished.send(outcome.is_ok()).ok();
    });
    assert_eq!(
        result.recv_timeout(Duration::from_secs(45)),
        Ok(true),
        "ConPTY resource regression failed or teardown did not complete within 45 seconds"
    );
}

fn run_with_bundled_host() {
    // Match the product's shipped host. A native-only probe on Windows 11
    // build 22631 retained a conhost process handle per create/close without
    // a shell or Nebula code. Do not relax the budget to absorb that OS defect.
    let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos();
    let directory =
        env::temp_dir().join(format!("pebrel-pty-lifecycle-{}-{stamp}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let executable = directory.join("pty-lifecycle.exe");
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/windows/conhost");
    fs::copy(env::current_exe().unwrap(), &executable).unwrap();
    for name in ["conpty.dll", "OpenConsole.exe"] {
        fs::copy(fixtures.join(name), directory.join(name)).unwrap();
    }
    fs::write(directory.join("pebrel_settings.txt"), "openconsole=on\n").unwrap();
    let status = Command::new(executable)
        .args([
            "--exact",
            "bundled_conpty_resources_return_after_close_and_failed_spawn",
            "--nocapture",
        ])
        .env("PEBREL_PTY_LIFECYCLE_CHILD", "1")
        .env("PEBREL_CONFIG_DIR", &directory)
        .env("NEBULA_CONFIG_DIR", &directory)
        .status();
    // The host can finish asynchronously after ClosePseudoConsole. Windows
    // keeps its image mapped briefly after the test process has exited.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match fs::remove_dir_all(&directory) {
            Ok(()) => break,
            Err(error)
                if Instant::now() < deadline
                    && error.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                thread::sleep(Duration::from_millis(50));
            },
            Err(error) => {
                panic!("cannot release the test host files at {}: {error}", directory.display())
            },
        }
    }
    assert!(status.unwrap().success(), "the bundled-host lifecycle regression failed");
}
