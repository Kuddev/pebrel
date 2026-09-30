//! 主线程窗口显隐适配；窗口实体和 PTY 始终由现有工作区持有。
use gpui::Window;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[cfg(target_os = "linux")]
pub(crate) fn is_wayland(window: &Window) -> bool {
    HasWindowHandle::window_handle(window)
        .is_ok_and(|handle| matches!(handle.as_raw(), RawWindowHandle::Wayland(_)))
}

pub(crate) fn set_visible(window: &Window, visible: bool) -> bool {
    let Ok(handle) = HasWindowHandle::window_handle(window) else { return false };
    match handle.as_raw() {
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(handle) => {
            // GPUI 的借用保证 NSView 在当前主线程调用期间存活。
            let view = unsafe { &*handle.ns_view.as_ptr().cast::<objc2_app_kit::NSView>() };
            let Some(native) = view.window() else { return false };
            if visible {
                native.orderFront(None);
            } else {
                native.orderOut(None);
            }
            true
        },
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xcb(handle) => xcb_visible(window, handle.window.get(), visible),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Wayland(_) => {
            // xdg-shell 不提供任意隐藏或定位；让合成器管理最小化，避免破坏 surface 生命周期。
            if visible {
                window.activate_window();
            } else {
                window.minimize_window();
            }
            true
        },
        _ => false,
    }
}

#[cfg(target_os = "linux")]
fn xcb_visible(window: &Window, xid: u32, visible: bool) -> bool {
    use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
    #[repr(C)]
    struct Cookie {
        sequence: u32,
    }
    #[link(name = "xcb")]
    unsafe extern "C" {
        fn xcb_map_window(connection: *mut std::ffi::c_void, window: u32) -> Cookie;
        fn xcb_unmap_window(connection: *mut std::ffi::c_void, window: u32) -> Cookie;
        fn xcb_flush(connection: *mut std::ffi::c_void) -> i32;
    }
    let Ok(display) = window.display_handle() else { return false };
    let RawDisplayHandle::Xcb(display) = display.as_raw() else { return false };
    let Some(connection) = display.connection else { return false };
    // 借用 GPUI 的同一连接，仅排队请求，不读取或窃取事件，也不关闭连接。
    unsafe {
        if visible {
            xcb_map_window(connection.as_ptr(), xid);
        } else {
            xcb_unmap_window(connection.as_ptr(), xid);
        }
        xcb_flush(connection.as_ptr()) > 0
    }
}
