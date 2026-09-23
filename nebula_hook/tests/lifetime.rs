//! Real-process lifetime regressions: a provider may retain its stdin handle,
//! and a pipe receiver may stop draining. Neither can pin the installed helper.

use std::io::Write as _;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

struct Invocation(Option<Child>);

impl Invocation {
    fn spawn(args: &[&str], pipe: Option<&str>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pebrel-hook"));
        command.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        for suffix in ["NOTIFY_PIPE", "REMOTE_HOOK_TOKEN", "HOOK_LOG"] {
            command.env_remove(format!("PEBREL_{suffix}"));
            command.env_remove(format!("NEBULA_{suffix}"));
        }
        if let Some(pipe) = pipe {
            command.env("PEBREL_NOTIFY_PIPE", pipe);
        }
        Self(Some(command.spawn().unwrap()))
    }

    fn finish(mut self) -> Output {
        let deadline = Instant::now() + Duration::from_secs(6);
        let child = self.0.as_mut().unwrap();
        loop {
            if child.try_wait().unwrap().is_some() {
                let output = self.0.take().unwrap().wait_with_output().unwrap();
                assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
                return output;
            }
            assert!(Instant::now() < deadline, "helper retained a blocked invocation");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Invocation {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn stdin_kept_open_cannot_pin_a_native_hook_executable() {
    for args in [&["codex", "--hooks=full"][..], &["claude"][..]] {
        let mut invocation = Invocation::spawn(args, None);
        let mut input = invocation.0.as_mut().unwrap().stdin.take().unwrap();
        input.write_all(b"{\"hook_event_name\":\"Stop\"}").unwrap();
        // Hold stdin open across finish: EOF must not be necessary for exit.
        let output = invocation.finish();
        assert!(output.stdout.is_empty());
        drop(input);
    }
}

#[test]
fn cursor_still_allows_submission_when_stdin_never_closes() {
    let mut invocation = Invocation::spawn(&["cursor", "--event", "prompt"], None);
    let input = invocation.0.as_mut().unwrap().stdin.take().unwrap();
    let output = invocation.finish();
    assert_eq!(output.stdout, b"{\"continue\":true}\n");
    drop(input);
}

#[test]
fn ordinary_completed_payload_exits_silently() {
    let mut invocation = Invocation::spawn(&["codex", "--hooks=full"], None);
    invocation.0.as_mut().unwrap().stdin.take().unwrap().write_all(b"{}").unwrap();
    let output = invocation.finish();
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
}

#[test]
fn legacy_notify_still_invokes_the_users_chained_program() {
    let executable = std::env::current_exe().unwrap();
    let invocation = Invocation::spawn(
        &["codex", "--chain", executable.to_str().unwrap(), "--list", "--format=terse"],
        None,
    );
    let output = invocation.finish();
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("ordinary_completed_payload_exits_silently")
    );
}

#[cfg(windows)]
#[test]
fn a_receiver_that_never_reads_cannot_pin_the_helper() {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateNamedPipeW(
            name: *const u16,
            mode: u32,
            pipe_mode: u32,
            instances: u32,
            output: u32,
            input: u32,
            timeout: u32,
            security: *const c_void,
        ) -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn GetNamedPipeClientProcessId(handle: *mut c_void, pid: *mut u32) -> i32;
    }
    struct Pipe(*mut c_void);
    impl Drop for Pipe {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let name = format!(r"\\.\pipe\pebrel-hook-lifetime-test-{}", std::process::id());
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // The server owns an inbound byte pipe and deliberately never drains it.
    let handle =
        unsafe { CreateNamedPipeW(wide.as_ptr(), 1, 0, 1, 4096, 4096, 0, std::ptr::null()) };
    assert_ne!(handle as isize, -1, "{}", std::io::Error::last_os_error());
    let _pipe = Pipe(handle);
    let payload = "x".repeat(16 * 1024);
    let invocation = Invocation::spawn(&["codex", &payload], Some(&name));
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        let mut pid = 0;
        if unsafe { GetNamedPipeClientProcessId(handle, &mut pid) } != 0 {
            assert_eq!(pid, invocation.0.as_ref().unwrap().id());
            break;
        }
        assert!(Instant::now() < deadline, "helper never connected to the blocked receiver");
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = invocation.finish();
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
}
