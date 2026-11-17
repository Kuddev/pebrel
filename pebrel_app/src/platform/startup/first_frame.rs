//! Present a regular Windows startup window before making it visible.
//!
//! GPUI builds the scene in App::open_window, but its Windows backend normally
//! shows the HWND before that scene exists. Its next-frame callbacks run BEFORE
//! presentation, so neither those callbacks nor App::defer are a paint barrier.

use gpui::{AnyWindowHandle, App, WindowOptions};

pub(crate) fn defer_show(options: &mut WindowOptions) -> bool {
    if !cfg!(windows) || !options.show || !options.focus {
        return false;
    }
    // Preserve the requested activation; GPUI retains the original placement
    // until activate_window. Silent/background/Quick Terminal paths never enter.
    options.show = false;
    true
}

pub(crate) fn present_then_show(handle: AnyWindowHandle, cx: &mut App) {
    #[cfg(windows)]
    cx.spawn(async move |cx| {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_PAINT};
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

        let hwnd = handle.update(cx, |_, window, _| {
            let native = HasWindowHandle::window_handle(window).ok()?;
            let RawWindowHandle::Win32(native) = native.as_raw() else { return None };
            Some(native.hwnd.get())
        });
        let Ok(Some(hwnd)) = hwnd else { return };

        // SAFETY: this one-shot task runs on GPUI's foreground thread. The handle
        // was resolved from a live GPUI window above, and no App/Window borrow is
        // held across SendMessage: WM_PAINT re-enters GPUI's frame callback.
        // Pinned GPUI Windows handles WM_PAINT by synchronously drawing/presenting
        // even when hidden. Explicit delivery avoids relying on hidden-window
        // WM_PAINT generation by Windows. There are no timers or polling loops.
        unsafe { SendMessageW(hwnd as _, WM_PAINT, 0, 0) };

        // Only after the paint handler returns, use GPUI activation to consume
        // its pending placement. Raw ShowWindow would leave that placement stale.
        let _ = handle.update(cx, |_, window, _| window.activate_window());
    })
    .detach();

    #[cfg(not(windows))]
    let _ = (handle, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_visible_focused_windows_defer_their_initial_show() {
        for (show, focus) in [(true, true), (true, false), (false, true), (false, false)] {
            let mut options = WindowOptions { show, focus, ..Default::default() };
            let deferred = defer_show(&mut options);
            assert_eq!(deferred, cfg!(windows) && show && focus);
            assert_eq!(options.show, show && !deferred);
            assert_eq!(options.focus, focus);
        }
    }
}
