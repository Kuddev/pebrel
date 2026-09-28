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

/// A server-side named pipe in the exact state the live transport can leave
/// behind during shutdown: created, never accepted and never drained. The
/// default harness runs tests in one process, so every name must be unique.
#[cfg(windows)]
mod stalled_receiver {
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

    pub struct Receiver(*mut c_void);

    impl Receiver {
        /// mode 1 = PIPE_ACCESS_INBOUND, one instance: mirrors the live
        /// transport's one-client-at-a-time shape.
        pub fn create(discriminator: &str) -> (Self, String) {
            let name = format!(r"\\.\pipe\pebrel-hook-lifetime-test-{discriminator}");
            let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            let handle = unsafe {
                CreateNamedPipeW(wide.as_ptr(), 1, 0, 1, 4096, 4096, 0, std::ptr::null())
            };
            assert_ne!(handle as isize, -1, "{}", std::io::Error::last_os_error());
            (Self(handle), name)
        }

        /// Kernel answer to "did this process connect?" — the same identity
        /// probe the live transport issues for every accepted client.
        pub fn connected_client(&self) -> Option<u32> {
            let mut pid = 0;
            let ok = unsafe { GetNamedPipeClientProcessId(self.0, &mut pid) };
            (ok != 0 && pid != 0).then_some(pid)
        }
    }

    impl Drop for Receiver {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

#[cfg(windows)]
#[test]
fn a_receiver_that_never_reads_cannot_pin_the_helper() {
    let (receiver, name) = stalled_receiver::Receiver::create("codex-argv");
    let payload = "x".repeat(16 * 1024);
    let invocation = Invocation::spawn(&["codex", &payload], Some(&name));
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if receiver.connected_client().is_some_and(|pid| pid == invocation.0.as_ref().unwrap().id())
        {
            break;
        }
        assert!(Instant::now() < deadline, "helper never connected to the blocked receiver");
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = invocation.finish();
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
}

/// Issue #259: Pi's extension launches `pebrel-hook pi <prompt-json>` as a
/// detached Windows process and never reaps it, so a prompt burst can
/// outlive its host. The padded context field exceeds the receiver's buffer,
/// pinning the first connect mid-write — the stalled-write condition the
/// forwarding deadline exists for.
#[cfg(windows)]
#[test]
fn concurrent_pi_prompt_notifications_exit_against_a_wedged_receiver() {
    let (receiver, name) = stalled_receiver::Receiver::create("pi-burst");
    let prompt = |session: u32| {
        format!(
            r#"{{"kind":"prompt","session_id":"s-{session}","bridge_instance":"5c1e","bridge_sequence":"{session}","event_id":"5c1e:{session}","cwd":"C:\\Users\\dev\\project","context":"{}"}}"#,
            "x".repeat(16 * 1024),
        )
    };
    let pending: Vec<_> =
        (1..=5).map(|session| Invocation::spawn(&["pi", &prompt(session)], Some(&name))).collect();
    // The wedged receiver pins whichever helper connects first mid-write, so
    // that connection stays observable until the deadline retires the helper.
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if receiver.connected_client().is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "no pi prompt helper connected to the wedged receiver");
        std::thread::sleep(Duration::from_millis(10));
    }
    for invocation in pending {
        let output = invocation.finish();
        assert!(output.stdout.is_empty() && output.stderr.is_empty());
    }
}

#[test]
fn pi_prompt_notification_exits_cleanly_without_a_host() {
    let invocation =
        Invocation::spawn(&["pi", r#"{"kind":"prompt","session_id":"s1","cwd":"/tmp"}"#], None);
    let output = invocation.finish();
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
}
