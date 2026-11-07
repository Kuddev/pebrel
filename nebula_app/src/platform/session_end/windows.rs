use std::{cell::Cell, marker::PhantomData, ptr::null_mut, rc::Rc};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::{
        CWPRETSTRUCT, CWPSTRUCT, CallNextHookEx, HHOOK, SetWindowsHookExW, UnhookWindowsHookEx,
        WH_CALLWNDPROC, WH_CALLWNDPROCRET, WM_ENDSESSION, WM_QUERYENDSESSION,
    },
};

thread_local! { static INSTALLED: Cell<bool> = const { Cell::new(false) }; }

// Hooks observe only this process's UI thread and never call application code or I/O.
unsafe extern "system" fn before(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && lp != 0 {
        // The OS owns this payload for the duration of the callback.
        let message = unsafe { &*(lp as *const CWPSTRUCT) };
        match message.message {
            WM_QUERYENDSESSION => super::set_pending(true),
            WM_ENDSESSION => super::set_pending(message.wParam != 0),
            _ => {},
        }
    }
    unsafe { CallNextHookEx(null_mut(), code, wp, lp) }
}

unsafe extern "system" fn after(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && lp != 0 {
        let message = unsafe { &*(lp as *const CWPRETSTRUCT) };
        if message.message == WM_QUERYENDSESSION && message.lResult == 0 {
            super::set_pending(false);
        }
    }
    unsafe { CallNextHookEx(null_mut(), code, wp, lp) }
}

pub(crate) struct Monitor {
    before: HHOOK,
    after: HHOOK,
    // Installation, callbacks, and unhooking must stay on the same UI thread.
    _thread: PhantomData<Rc<()>>,
}

impl Monitor {
    pub(crate) fn install() -> std::io::Result<Self> {
        if INSTALLED.get() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "Session-end monitor already installed",
            ));
        }
        let thread = unsafe { GetCurrentThreadId() };
        if thread == 0 {
            return Err(std::io::Error::other("Cannot observe a global window thread"));
        }
        let before = unsafe { SetWindowsHookExW(WH_CALLWNDPROC, Some(before), null_mut(), thread) };
        if before.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let after =
            unsafe { SetWindowsHookExW(WH_CALLWNDPROCRET, Some(after), null_mut(), thread) };
        if after.is_null() {
            let error = std::io::Error::last_os_error();
            unsafe {
                UnhookWindowsHookEx(before);
            }
            return Err(error);
        }
        INSTALLED.set(true);
        Ok(Self { before, after, _thread: PhantomData })
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        let after = unsafe { UnhookWindowsHookEx(self.after) };
        let before = unsafe { UnhookWindowsHookEx(self.before) };
        if after != 0 && before != 0 {
            INSTALLED.set(false);
        } else {
            log::warn!("Could not remove all session-end observation hooks");
        }
        super::set_pending(false);
    }
}

#[cfg(test)]
#[path = "windows/tests.rs"]
mod tests;
