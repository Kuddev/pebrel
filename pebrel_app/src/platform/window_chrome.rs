//! Native window decoration belongs to the platform boundary. Workspace code
//! supplies ordinary GPUI options and uses optional geometry without depending
//! on AppKit types or duplicating platform detection.

use gpui::{Window, WindowOptions};

#[cfg(target_os = "macos")]
mod macos;

pub(crate) fn application_id(_fallback: &str) -> String {
    #[cfg(target_os = "linux")]
    {
        use std::sync::OnceLock;
        static ID: OnceLock<&'static str> = OnceLock::new();
        ID.get_or_init(|| {
            // 从同一安装前缀的桌面文件判定渠道；AppImage、解包版和 deb 共用此布局。
            let applications = std::env::current_exe().ok().and_then(|executable| {
                Some(executable.parent()?.parent()?.join("share/applications"))
            });
            if applications.is_some_and(|directory| {
                directory.join("io.github.kuddev.pebrel.preview.desktop").is_file()
                    && !directory.join("io.github.kuddev.pebrel.desktop").is_file()
            }) {
                crate::brand::LINUX_PREVIEW_APP_ID
            } else {
                crate::brand::LINUX_APP_ID
            }
        })
        .to_string()
    }
    #[cfg(not(target_os = "linux"))]
    _fallback.to_owned()
}

#[cfg(test)]
pub(crate) fn native_test_window_id(_window: &Window) -> Option<(&'static str, u32)> {
    #[cfg(target_os = "linux")]
    {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        match HasWindowHandle::window_handle(_window).ok()?.as_raw() {
            RawWindowHandle::Xcb(handle) => return Some(("x11", handle.window.get())),
            RawWindowHandle::Wayland(handle) => {
                #[link(name = "wayland-client")]
                unsafe extern "C" {
                    fn wl_proxy_get_id(proxy: *mut std::ffi::c_void) -> u32;
                }
                // 借用仍存活的 GPUI surface，只读协议 ID 以核对真实 compositor trace。
                return Some(("wayland", unsafe { wl_proxy_get_id(handle.surface.as_ptr()) }));
            },
            _ => {},
        }
    }
    None
}

pub(crate) fn configure_options(options: WindowOptions) -> WindowOptions {
    #[cfg(target_os = "macos")]
    {
        let mut options = options;
        if let Some(titlebar) = options.titlebar.as_mut() {
            // A manual position makes GPUI rewrite AppKit's titlebar container,
            // breaking the native toolbar layout.
            titlebar.traffic_light_position = None;
        }
        options.app_owns_titlebar_drag = true;
        options
    }
    #[cfg(not(target_os = "macos"))]
    {
        options
    }
}

pub(crate) fn configure(_window: &Window) {
    #[cfg(target_os = "macos")]
    macos::configure(_window);
}

/// macOS leaves no system control reserve on the right edge, so toolbar buttons need an inset.
pub(crate) const RIGHT_CONTROLS_INSET: f32 = if cfg!(target_os = "macos") { 12.0 } else { 0.0 };

/// Height and left inset when native controls determine the titlebar geometry.
pub(crate) fn layout(_window: &Window) -> Option<(f32, f32)> {
    #[cfg(target_os = "macos")]
    {
        Some(macos::layout(_window))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
