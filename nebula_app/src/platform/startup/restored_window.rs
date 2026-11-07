//! Re-anchor a restored window after GPUI consumes its own native placement.
//!
//! The pinned GPUI Windows backend converts `WindowOptions` bounds with the DPI
//! of the monitor that created the HWND (`GetDpiForWindow` immediately after
//! `CreateWindowExW`) and then lets Windows rescale the moved window through
//! `WM_DPICHANGED`. On a mixed-DPI desktop the saved logical origin is therefore
//! interpreted in creation-scale pixels and the system keeps that top-left while
//! only scaling the size, so a window restored onto a display with another
//! scale can end up half on the neighbouring display.
//!
//! Activation applies GPUI's pending placement; this pass then derives the
//! outer rectangle from the saved logical client size in the target display's
//! own physical pixels and clamps it into that display's work area.

use gpui::Window;

/// `(left, top, width, height)` differences between the native outer and client
/// rectangles, measured on the live window so border insets stay platform-owned.
#[cfg(any(windows, test))]
type Frame = (i32, i32, i32, i32);

/// `(left, top, right, bottom)` in physical desktop pixels.
#[cfg(any(windows, test))]
type WorkArea = (i32, i32, i32, i32);

/// Position a restored window. Returns `true` when the native rectangle either
/// already matches or was updated; non-Windows platforms keep GPUI placement.
pub(crate) fn settle_restored_window(
    window: &Window,
    position: (i32, i32),
    logical_size: (u32, u32),
    display: u64,
) -> bool {
    #[cfg(windows)]
    {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

        let Some(scale) = super::display_scale(display) else { return false };
        let Some(work) = super::display_work_area(display) else { return false };
        let Ok(handle) = HasWindowHandle::window_handle(window) else { return false };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else { return false };
        settle_native(
            handle.hwnd.get(),
            position,
            logical_size,
            scale,
            super::placement_offset(display),
            work,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (window, position, logical_size, display);
        true
    }
}

#[cfg(windows)]
fn settle_native(
    hwnd: isize,
    position: (i32, i32),
    logical_size: (u32, u32),
    scale: f32,
    offset: (i32, i32),
    work: WorkArea,
) -> bool {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, IsZoomed, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
    };

    let hwnd = hwnd as *mut core::ffi::c_void;
    // A maximized window owns the work area; never resize it behind zoom state.
    // SAFETY: the caller owns a live HWND.
    if unsafe { IsZoomed(hwnd) } != 0 {
        return true;
    }
    // SAFETY: plain native output structures admit zero initialization.
    let (mut outer, mut client): (RECT, RECT) = unsafe { std::mem::zeroed() };
    let mut client_origin = POINT { x: 0, y: 0 };
    // SAFETY: the live HWND and initialized local outputs remain valid.
    if unsafe { GetWindowRect(hwnd, &mut outer) } == 0
        || unsafe { GetClientRect(hwnd, &mut client) } == 0
        || unsafe { ClientToScreen(hwnd, &mut client_origin) } == 0
    {
        return false;
    }
    let frame = (
        client_origin.x - outer.left,
        client_origin.y - outer.top,
        (outer.right - outer.left) - client.right,
        (outer.bottom - outer.top) - client.bottom,
    );
    let (x, y, width, height) = outer_rect(position, logical_size, scale, offset, frame, work);
    if (outer.left, outer.top, outer.right - outer.left, outer.bottom - outer.top)
        == (x, y, width, height)
    {
        return true;
    }
    // SAFETY: only the live window's position and size change; ownership,
    // activation and z-order are preserved.
    unsafe {
        SetWindowPos(hwnd, std::ptr::null_mut(), x, y, width, height, SWP_NOACTIVATE | SWP_NOZORDER)
            != 0
    }
}

/// Saved geometry to the clamped physical outer rectangle.
///
/// `position` follows the session convention: the client origin captured on the
/// saved display includes that display's work-area offset, so restore removes
/// it again (mirroring `workspace_position` in the startup geometry).
#[cfg(any(windows, test))]
fn outer_rect(
    position: (i32, i32),
    logical_size: (u32, u32),
    scale: f32,
    offset: (i32, i32),
    frame: Frame,
    work: WorkArea,
) -> (i32, i32, i32, i32) {
    let client_origin = (position.0 - offset.0, position.1 - offset.1);
    let client_size = (
        (logical_size.0 as f32 * scale).round().max(1.0) as i32,
        (logical_size.1 as f32 * scale).round().max(1.0) as i32,
    );
    let outer_size = (client_size.0 + frame.2, client_size.1 + frame.3);
    let desired = (client_origin.0 - frame.0, client_origin.1 - frame.1);
    let (left, top, right, bottom) = work;
    (
        desired.0.clamp(left, (right - outer_size.0).max(left)),
        desired.1.clamp(top, (bottom - outer_size.1).max(top)),
        outer_size.0,
        outer_size.1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_saved_on_the_high_dpi_display_stays_inside_its_work_area() {
        // Regression: 1920x1200 primary at 125% plus a 3840x2160 display at 200%
        // whose physical origin is (-1033, -2160). The saved client origin and
        // logical size overflow the bottom of the 4K display, which the old
        // creation-DPI conversion anchored half on the primary display.
        let work = (-1033, -2160, 2807, 0);
        assert_eq!(
            outer_rect((-274, -1050), (1332, 765), 2.0, (0, 0), (16, 16, 16, 16), work),
            (-290, -1546, 2680, 1546),
        );
    }

    #[test]
    fn the_saved_work_area_offset_is_removed_from_the_client_origin() {
        // A top taskbar makes rcWork start below rcMonitor; capture adds that
        // offset and restore must remove it before converting to the client.
        let work = (0, 60, 1920, 1200);
        assert_eq!(
            outer_rect((100, 160), (800, 600), 1.0, (0, 60), (8, 8, 16, 16), work),
            (92, 92, 816, 616),
        );
    }

    #[test]
    fn a_rect_larger_than_the_work_area_starts_at_its_origin() {
        let work = (-1033, -2160, 2807, 0);
        assert_eq!(
            outer_rect((-2000, -2400), (3000, 1400), 2.0, (0, 0), (16, 16, 16, 16), work),
            (-1033, -2160, 6016, 2816),
        );
    }

    #[test]
    fn a_fully_visible_restored_rect_is_not_moved() {
        let work = (0, 0, 1920, 1040);
        assert_eq!(
            outer_rect((120, 80), (900, 700), 1.0, (0, 0), (8, 8, 16, 16), work),
            (112, 72, 916, 716),
        );
    }
}
