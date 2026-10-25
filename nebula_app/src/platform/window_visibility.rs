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
        RawWindowHandle::Xcb(_) | RawWindowHandle::Wayland(_) => {
            // 框架拥有首帧与事件队列；绕过它直接 map 会漏掉隐藏窗口的恢复绘制。
            window.set_window_visible(visible)
        },
        _ => false,
    }
}
