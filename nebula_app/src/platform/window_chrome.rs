//! Native window decoration belongs to the platform boundary. Workspace code
//! supplies ordinary GPUI options and uses optional geometry without depending
//! on AppKit types or duplicating platform detection.

use gpui::{Window, WindowOptions};

#[cfg(target_os = "macos")]
mod macos;

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
