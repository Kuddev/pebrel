use super::Monitor;
use crate::platform::session_end::is_pending;
use std::{cell::Cell, ptr::null_mut};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::WindowsAndMessaging::*,
};

thread_local! {
    static VETO: Cell<bool> = const { Cell::new(false) };
    static PROTECTED_ON_ENTRY: Cell<bool> = const { Cell::new(false) };
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match message {
        WM_QUERYENDSESSION => {
            PROTECTED_ON_ENTRY.set(is_pending());
            if VETO.get() { 0 } else { 1 }
        },
        WM_ENDSESSION => 0,
        _ => unsafe { DefWindowProcW(hwnd, message, wp, lp) },
    }
}

struct Window {
    hwnd: HWND,
    name: Vec<u16>,
}
impl Window {
    fn new() -> Self {
        let name: Vec<u16> = format!("PebrelSessionEndTest-{}\0", unsafe { GetCurrentThreadId() })
            .encode_utf16()
            .collect();
        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        let class = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            lpszClassName: name.as_ptr(),
            ..unsafe { std::mem::zeroed() }
        };
        assert_ne!(unsafe { RegisterClassW(&class) }, 0);
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                name.as_ptr(),
                name.as_ptr(),
                0,
                0,
                0,
                1,
                1,
                null_mut(),
                null_mut(),
                instance,
                null_mut(),
            )
        };
        assert!(!hwnd.is_null());
        Self { hwnd, name }
    }
    fn send(&self, message: u32, wp: WPARAM) -> LRESULT {
        unsafe { SendMessageW(self.hwnd, message, wp, 0) }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(self.hwnd) != 0 {
                DestroyWindow(self.hwnd);
            }
            UnregisterClassW(self.name.as_ptr(), GetModuleHandleW(std::ptr::null()));
        }
        VETO.set(false);
    }
}

#[test]
fn query_and_confirm_protect_before_the_receiver_without_changing_returns() {
    let window = Window::new();
    let _monitor = Monitor::install().unwrap();
    assert!(!is_pending());
    assert_eq!(window.send(WM_QUERYENDSESSION, 0), 1);
    assert!(is_pending(), "a real sent query must arm the persistence guard");
    assert!(PROTECTED_ON_ENTRY.get(), "protection must precede the receiving window procedure");
    assert_eq!(window.send(WM_QUERYENDSESSION, 0), 1);
    assert_eq!(window.send(WM_ENDSESSION, 1), 0);
    assert!(is_pending());
    window.send(WM_ENDSESSION, 0);
    assert!(!is_pending());
}

#[test]
fn a_veto_or_cancel_releases_protection_and_ordinary_close_never_arms_it() {
    let window = Window::new();
    let _monitor = Monitor::install().unwrap();
    VETO.set(true);
    assert_eq!(window.send(WM_QUERYENDSESSION, 0), 0);
    assert!(!is_pending());
    VETO.set(false);
    window.send(WM_QUERYENDSESSION, 0);
    assert!(is_pending());
    window.send(WM_ENDSESSION, 0);
    assert!(!is_pending());
    window.send(WM_APP, 0);
    window.send(WM_CLOSE, 0);
    assert!(!is_pending());
}

#[test]
fn observation_is_limited_to_the_installing_ui_thread() {
    let _monitor = Monitor::install().unwrap();
    std::thread::spawn(|| {
        let window = Window::new();
        window.send(WM_QUERYENDSESSION, 0);
        assert!(!is_pending(), "a different thread must not be hooked");
    })
    .join()
    .unwrap();
    assert!(!is_pending());
}

#[test]
fn dropping_the_monitor_unhooks_and_allows_reinstallation() {
    let window = Window::new();
    let monitor = Monitor::install().unwrap();
    assert!(Monitor::install().is_err(), "two monitors must not compete on one UI thread");
    window.send(WM_QUERYENDSESSION, 0);
    assert!(is_pending());
    drop(monitor);
    assert!(!is_pending());
    window.send(WM_QUERYENDSESSION, 0);
    assert!(!is_pending(), "dropped monitor must no longer observe messages");
    let _next = Monitor::install().unwrap();
    window.send(WM_ENDSESSION, 1);
    assert!(is_pending(), "confirmed end without a prior query is still protected");
}
