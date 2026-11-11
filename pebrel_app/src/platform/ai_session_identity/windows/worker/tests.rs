use super::super::{PROCESS_QUERY_INFORMATION, creation_time, open_process};
use super::*;
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::System::Threading::{GetProcessId, GetStartupInfoW, STARTUPINFOW};

pub(super) const HELPER: &str =
    "platform::ai_session_identity::windows::worker::tests::worker_helper";
pub(super) const CHILD: &str = "pebrel-session-worker-child";

#[test]
fn worker_helper() {
    let args: Vec<_> = std::env::args().collect();
    if !args.iter().any(|arg| arg == CHILD) {
        return;
    }
    let mut startup: STARTUPINFOW = unsafe { mem::zeroed() };
    unsafe { GetStartupInfoW(&mut startup) };
    assert_ne!(
        startup.dwFlags & STARTF_FORCEOFFFEEDBACK,
        0,
        "native startup must suppress feedback"
    );
    assert!(unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() }.is_null());
    let mode = args.last().unwrap().as_str();
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let result = if matches!(mode, "oversized" | "partial" | "exit") {
        output.write_all(READY).unwrap();
        output.flush().unwrap();
        input.read_exact(&mut [0; 12]).unwrap();
        match mode {
            "oversized" => output.write_all(&(MAX_PROBE_OUTPUT as u32 + 1).to_le_bytes()),
            "partial" => {
                output.write_all(&[10, 0]).unwrap();
                output.flush().unwrap();
                std::thread::sleep(Duration::from_secs(30));
                Ok(())
            },
            _ => std::process::exit(7),
        }
    } else {
        serve(&mut input, &mut output, |pid, created| {
            if mode == "stall" {
                std::thread::sleep(Duration::from_secs(30));
            }
            collect_records(pid, created)
        })
    };
    output.flush().unwrap();
    std::process::exit(if result.is_ok() { 0 } else { 1 });
}

fn root_identity() -> (u32, u64) {
    let pid = std::process::id();
    let process = open_process(pid, PROCESS_QUERY_INFORMATION).unwrap();
    (pid, creation_time(process.0).unwrap())
}

fn process_id(worker: &Worker) -> u32 {
    unsafe { GetProcessId(worker.process.as_raw_handle()) }
}

#[test]
fn repeated_requests_reuse_the_native_worker_and_drop_reaps_it() {
    let (pid, created) = root_identity();
    let mut slot = None;
    query_slot(&mut slot, pid, created, Instant::now() + PROBE_TIMEOUT).unwrap();
    let worker_id = process_id(slot.as_ref().unwrap());
    assert_ne!(worker_id, 0);
    let owned = slot.as_ref().unwrap().process.try_clone().unwrap();
    for _ in 0..12 {
        query_slot(&mut slot, pid, created, Instant::now() + PROBE_TIMEOUT).unwrap();
        assert_eq!(process_id(slot.as_ref().unwrap()), worker_id);
    }
    drop(slot);
    assert_eq!(unsafe { WaitForSingleObject(owned.as_raw_handle(), 1000) }, WAIT_OBJECT_0);
}

#[test]
fn failed_or_incomplete_replies_discard_the_worker_before_the_next_request() {
    let (pid, created) = root_identity();
    for mode in ["stall", "partial", "oversized", "exit"] {
        let mut args = arguments();
        args.extend(["--skip".into(), mode.into()]);
        let worker = Worker::start(&args, Instant::now() + Duration::from_secs(10)).unwrap();
        let owned = worker.process.try_clone().unwrap();
        let mut slot = Some(worker);
        let started = Instant::now();
        let error = query_slot(&mut slot, pid, created, started + Duration::from_millis(150));
        assert!(error.is_err(), "{mode}");
        assert!(started.elapsed() < Duration::from_secs(3), "{mode}");
        assert!(slot.is_none(), "failed channels cannot be reused or automatically replayed");
        assert_eq!(unsafe { WaitForSingleObject(owned.as_raw_handle(), 1000) }, WAIT_OBJECT_0);
        query_slot(&mut slot, pid, created, Instant::now() + PROBE_TIMEOUT).unwrap();
    }
}

#[test]
fn worker_owner_helper() {
    let Some(ready) = std::env::var_os("PEBREL_TEST_WORKER_OWNER_READY") else { return };
    let (pid, created) = root_identity();
    let mut slot = None;
    query_slot(&mut slot, pid, created, Instant::now() + PROBE_TIMEOUT).unwrap();
    std::fs::write(ready, process_id(slot.as_ref().unwrap()).to_string()).unwrap();
    io::stdin().read_exact(&mut [0]).unwrap();
    // Do not run Rust destructors: process termination itself must close the job.
    std::process::exit(0);
}

#[test]
fn parent_exit_reaps_the_worker_without_running_rust_destructors() {
    use windows_sys::Win32::System::Threading::{PROCESS_SYNCHRONIZE, PROCESS_TERMINATE};
    struct Owner(std::process::Child);
    impl Drop for Owner {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let ready = directory.path().join("worker-pid");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "platform::ai_session_identity::windows::worker::tests::worker_owner_helper",
            "--nocapture",
        ])
        .env("PEBREL_TEST_WORKER_OWNER_READY", &ready)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    crate::platform::process::hidden_command(&mut command);
    let mut owner = Owner(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    let worker = loop {
        if let Ok(text) = std::fs::read_to_string(&ready) {
            if let Ok(pid) = text.parse() {
                break open_process(pid, PROCESS_SYNCHRONIZE | PROCESS_TERMINATE).unwrap();
            }
        }
        assert!(owner.0.try_wait().unwrap().is_none(), "owner exited before readiness");
        assert!(Instant::now() < deadline, "owner did not create its worker");
        std::thread::sleep(POLL);
    };
    owner.0.stdin.take().unwrap().write_all(b"x").unwrap();
    loop {
        if let Some(status) = owner.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "owner did not exit");
        std::thread::sleep(POLL);
    }
    let exited = unsafe { WaitForSingleObject(worker.0, 1000) } == WAIT_OBJECT_0;
    if !exited {
        unsafe {
            TerminateProcess(worker.0, 1);
            WaitForSingleObject(worker.0, 1000);
        }
    }
    assert!(exited, "the inherited handle list must not leak the owning job into its worker");
}

#[test]
fn server_frames_requests_independently_and_bounds_each_response() {
    let mut input = Vec::new();
    for (pid, created) in [(7_u32, 8_u64), (9, 10), (11, 12)] {
        input.extend_from_slice(&pid.to_le_bytes());
        input.extend_from_slice(&created.to_le_bytes());
    }
    let mut output = Vec::new();
    serve(&mut input.as_slice(), &mut output, |pid, created| {
        (pid != 9).then(|| format!("{pid}:{created}"))
    })
    .unwrap();
    let mut frames = &output[READY.len()..];
    for expected in ["7:8", "", "11:12"] {
        let length = u32::from_le_bytes(frames[..4].try_into().unwrap()) as usize;
        frames = &frames[4..];
        assert_eq!(&frames[..length], expected.as_bytes());
        frames = &frames[length..];
    }
    assert!(frames.is_empty());
    assert!(
        serve(&mut input.as_slice(), &mut Vec::new(), |_, _| {
            Some("x".repeat(MAX_PROBE_OUTPUT + 1))
        })
        .is_err()
    );
}

#[test]
#[ignore = "requires an idle Windows desktop and PEBREL_CURSOR_PROBE_OUTPUT"]
fn native_cursor_feedback_comparison() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CURSORINFO, GetCursorInfo, IDC_APPSTARTING, LoadCursorW,
    };

    let output_path = std::env::var_os("PEBREL_CURSOR_PROBE_OUTPUT").unwrap();
    let (pid, created) = root_identity();
    let mut results = Vec::new();
    for mode in ["legacy-before", "reused-worker", "legacy-after"] {
        std::thread::sleep(Duration::from_millis(500));
        let (stop, finished) = std::sync::mpsc::channel::<()>();
        let sampler = std::thread::spawn(move || {
            let starting = unsafe { LoadCursorW(ptr::null_mut(), IDC_APPSTARTING) };
            assert!(!starting.is_null());
            let mut cursor: CURSORINFO = unsafe { mem::zeroed() };
            let (mut samples, mut busy, mut transitions, mut previous) = (0, 0, 0, false);
            loop {
                // cbSize is an input on every call; the native output need not preserve it.
                cursor.cbSize = mem::size_of::<CURSORINFO>() as u32;
                if unsafe { GetCursorInfo(&mut cursor) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                let active = cursor.hCursor == starting;
                samples += 1;
                busy += u32::from(active);
                transitions += u32::from(active && !previous);
                previous = active;
                if !matches!(
                    finished.recv_timeout(Duration::from_millis(2)),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                ) {
                    break;
                }
            }
            Ok((samples, busy, transitions))
        });
        let mut slot = None;
        let mut workers = std::collections::BTreeSet::new();
        for _ in 0..12 {
            if mode == "reused-worker" {
                query_slot(&mut slot, pid, created, Instant::now() + PROBE_TIMEOUT).unwrap();
                workers.insert(process_id(slot.as_ref().unwrap()));
            } else {
                let command = super::super::helper_command(pid, created).unwrap();
                assert!(super::super::run_probe_output(command).is_some());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        drop(stop);
        let (samples, busy, transitions) = sampler.join().unwrap().unwrap();
        results.push(serde_json::json!({
            "mode": mode, "requests": 12, "cursor_samples": samples,
            "appstarting_samples": busy, "appstarting_transitions": transitions,
            "reused_worker_count": workers.len(),
        }));
    }
    std::fs::write(output_path, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
}
