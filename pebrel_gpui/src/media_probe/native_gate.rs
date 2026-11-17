//! Native presentation gate, separate from GPUI's cached activation observation.
use gpui::Window;

#[cfg(windows)]
pub type NativeWindow = isize;
#[cfg(not(windows))]
pub type NativeWindow = ();

pub fn capture(window: &Window) -> Option<NativeWindow> {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        match HasWindowHandle::window_handle(window).ok()?.as_raw() {
            RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
            _ => None,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        Some(())
    }
}

pub fn allowed(handle: Option<NativeWindow>, observed_active: bool) -> bool {
    #[cfg(windows)]
    {
        let _ = observed_active;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, IsIconic, IsWindowVisible,
        };
        let Some(handle) = handle else {
            return false;
        };
        let hwnd = HWND(handle as *mut std::ffi::c_void);
        // Read-only native queries; no focus changes, polling timer, or messages.
        unsafe {
            IsWindowVisible(hwnd).as_bool()
                && !IsIconic(hwnd).as_bool()
                && GetForegroundWindow() == hwnd
        }
    }
    #[cfg(not(windows))]
    {
        handle.is_some() && observed_active
    }
}
