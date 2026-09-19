use std::ffi::c_void;
use std::io::Error;
use std::mem::ManuallyDrop;
use std::num::NonZeroU32;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::os::windows::process::ExitStatusExt;
use std::process::ExitStatus;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use polling::os::iocp::{CompletionPacket, PollerIocpExt};
use polling::{Event, Poller};

use windows_sys::Win32::Foundation::{BOOLEAN, FALSE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessId, INFINITE, RegisterWaitForSingleObject, UnregisterWaitEx,
    WT_EXECUTEINWAITTHREAD, WT_EXECUTEONLYONCE,
};

use crate::tty::ChildEvent;

struct Interest {
    poller: Arc<Poller>,
    event: Event,
}

struct ChildExitSender {
    sender: mpsc::Sender<ChildEvent>,
    interest: Mutex<Option<Interest>>,
    child_handle: OwnedHandle,
}

/// WinAPI callback to run when child process exits.
extern "system" fn child_exit_callback(ctx: *mut c_void, timed_out: BOOLEAN) {
    if timed_out != 0 {
        return;
    }

    // The watcher retains this allocation and process handle until unregister
    // has waited for every callback. The callback only borrows the context.
    let event_tx = unsafe { &*ctx.cast::<ChildExitSender>() };

    let mut exit_code = 0_u32;
    let child_handle = event_tx.child_handle.as_raw_handle() as HANDLE;
    let status = unsafe { GetExitCodeProcess(child_handle, &mut exit_code) };
    let exit_status = if status == FALSE { None } else { Some(ExitStatus::from_raw(exit_code)) };
    event_tx.sender.send(ChildEvent::Exited(exit_status)).ok();

    let interest = event_tx.interest.lock().unwrap();
    if let Some(interest) = interest.as_ref() {
        interest.poller.post(CompletionPacket::new(interest.event)).ok();
    }
}

pub struct ChildExitWatcher {
    wait_handle: AtomicPtr<c_void>,
    event_rx: mpsc::Receiver<ChildEvent>,
    context: ManuallyDrop<Arc<ChildExitSender>>,
    pid: Option<NonZeroU32>,
}

impl ChildExitWatcher {
    pub fn new(child_handle: OwnedHandle) -> Result<ChildExitWatcher, Error> {
        let (event_tx, event_rx) = mpsc::channel();

        let mut wait_handle: HANDLE = ptr::null_mut();
        let raw_handle = child_handle.as_raw_handle() as HANDLE;
        let pid = unsafe { NonZeroU32::new(GetProcessId(raw_handle)) };
        let context = Arc::new(ChildExitSender {
            sender: event_tx,
            interest: Mutex::new(None),
            child_handle,
        });

        let success = unsafe {
            RegisterWaitForSingleObject(
                &mut wait_handle,
                raw_handle,
                Some(child_exit_callback),
                Arc::as_ptr(&context).cast_mut().cast(),
                INFINITE,
                WT_EXECUTEINWAITTHREAD | WT_EXECUTEONLYONCE,
            )
        };

        if success == 0 {
            Err(Error::last_os_error())
        } else {
            Ok(ChildExitWatcher {
                event_rx,
                pid,
                context: ManuallyDrop::new(context),
                wait_handle: AtomicPtr::from(wait_handle),
            })
        }
    }

    pub fn event_rx(&self) -> &mpsc::Receiver<ChildEvent> {
        &self.event_rx
    }

    pub fn register(&self, poller: &Arc<Poller>, event: Event) {
        *self.context.interest.lock().unwrap() = Some(Interest { poller: poller.clone(), event });
    }

    pub fn deregister(&self) {
        *self.context.interest.lock().unwrap() = None;
    }

    /// Retrieve the process handle of the underlying child process.
    ///
    /// This function does **not** pass ownership of the raw handle to you,
    /// and the handle is only guaranteed to be valid while the hosted application
    /// has not yet been destroyed.
    ///
    /// If you terminate the process using this handle, the terminal will get a
    /// timeout error, and the child watcher will emit an `Exited` event.
    pub fn raw_handle(&self) -> HANDLE {
        self.context.child_handle.as_raw_handle() as HANDLE
    }

    /// Retrieve the Process ID associated to the underlying child process.
    pub fn pid(&self) -> Option<NonZeroU32> {
        self.pid
    }
}

impl Drop for ChildExitWatcher {
    fn drop(&mut self) {
        // This runs on the PTY owner, never inside child_exit_callback. Waiting
        // here makes both callback completion and cancellation safe to reclaim.
        let success = unsafe {
            UnregisterWaitEx(self.wait_handle.load(Ordering::Relaxed), INVALID_HANDLE_VALUE)
        };
        if success != 0 {
            unsafe { ManuallyDrop::drop(&mut self.context) };
        } else {
            // An OS failure leaves callback liveness unknown. Retain its context
            // rather than freeing memory that native code might still access.
            log::error!(
                "cannot unregister child wait; retaining callback context: {}",
                Error::last_os_error()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::windows::io::AsHandle;
    use std::process::{Child, Command, Stdio};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use super::super::PTY_CHILD_EVENT_TOKEN;
    use super::*;

    fn waiting_child() -> Child {
        Command::new("cmd.exe")
            .args(["/d", "/q", "/k"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    fn watch(child: &Child) -> ChildExitWatcher {
        ChildExitWatcher::new(child.as_handle().try_clone_to_owned().unwrap()).unwrap()
    }

    #[test]
    pub fn event_is_emitted_when_child_exits() {
        const WAIT_TIMEOUT: Duration = Duration::from_secs(5);

        let poller = Arc::new(Poller::new().unwrap());

        let mut child = waiting_child();
        let child_exit_watcher = watch(&child);
        child_exit_watcher.register(&poller, Event::readable(PTY_CHILD_EVENT_TOKEN));

        child.kill().unwrap();

        // Poll for the event or fail with timeout if nothing has been sent.
        let mut events = polling::Events::new();
        poller.wait(&mut events, Some(WAIT_TIMEOUT)).unwrap();
        assert_eq!(events.iter().next().unwrap().key, PTY_CHILD_EVENT_TOKEN);
        // Verify that at least one `ChildEvent::Exited` was received.
        let expected_status = ExitStatus::from_raw(1);
        assert_eq!(
            child_exit_watcher.event_rx().try_recv(),
            Ok(ChildEvent::Exited(Some(expected_status)))
        );
        child.wait().unwrap();
    }

    #[test]
    fn cancellation_releases_context_and_poller_before_child_exit() {
        let mut child = waiting_child();
        let watcher = watch(&child);
        let context = Arc::downgrade(&watcher.context);
        let poller = Arc::new(Poller::new().unwrap());
        let poller_weak = Arc::downgrade(&poller);
        watcher.register(&poller, Event::readable(PTY_CHILD_EVENT_TOKEN));
        drop(poller);
        drop(watcher);

        let context_released = context.upgrade().is_none();
        let poller_released = poller_weak.upgrade().is_none();
        let still_running = child.try_wait().unwrap().is_none();
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(context_released && poller_released);
        assert!(still_running, "dropping a watcher must not terminate its borrowed child");
    }

    #[test]
    fn drop_waits_for_an_in_flight_callback() {
        let mut child = waiting_child();
        let watcher = watch(&child);
        let context = Arc::clone(&watcher.context);
        // The callback publishes the exit event before acquiring this lock.
        let interest_guard = context.interest.lock().unwrap();
        child.kill().unwrap();
        watcher.event_rx().recv_timeout(Duration::from_secs(5)).unwrap();
        let (finished, result) = mpsc::channel();
        let dropper = thread::spawn(move || {
            drop(watcher);
            finished.send(()).unwrap();
        });
        let returned_early = result.recv_timeout(Duration::from_millis(100)).is_ok();
        drop(interest_guard);
        if !returned_early {
            result.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        dropper.join().unwrap();
        child.wait().unwrap();
        assert!(!returned_early, "callback context cannot be freed while the callback runs");
    }
}
