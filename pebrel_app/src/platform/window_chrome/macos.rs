//! Keep the standard window controls in AppKit's native toolbar hierarchy.

use super::TitleBarLayout;
use gpui::Window;
use objc2::{MainThreadOnly, define_class, msg_send, rc::Retained, sel};
use objc2_app_kit::{
    NSToolbar, NSView, NSWindow, NSWindowButton, NSWindowDidEnterFullScreenNotification,
    NSWindowDidExitFullScreenNotification, NSWindowToolbarStyle,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObject, NSRect, ns_string};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

fn native_window(window: &Window) -> Option<Retained<NSWindow>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };
    // SAFETY: GPUI owns this live NSView and calls the adapter on the UI thread.
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    view.window()
}

define_class!(
    #[unsafe(super(NSToolbar, NSObject))]
    #[thread_kind = MainThreadOnly]
    struct PebrelWindowControlsToolbar;

    impl PebrelWindowControlsToolbar {
        #[unsafe(method(pebrelWindowDidEnterFullScreen:))]
        fn did_enter_full_screen(&self, notification: &NSNotification) {
            // AppKit draws the empty Unified toolbar in a separate full-screen
            // overlay. Transparency on the main window alone cannot remove it.
            self.setVisible(false);
            if let Some(window) = notification.object().and_then(|object| object.downcast::<NSWindow>().ok()) {
                // GPUI 4f2c269f turns transparency off before the transition.
                window.setTitlebarAppearsTransparent(true);
            }
        }

        #[unsafe(method(pebrelWindowDidExitFullScreen:))]
        fn did_exit_full_screen(&self, _notification: &NSNotification) {
            self.setVisible(true);
        }
    }
);

pub(super) fn configure(window: &Window) {
    let Some(native) = native_window(window) else { return };
    // SAFETY: initWithIdentifier: is NSToolbar's designated initializer and
    // PebrelWindowControlsToolbar adds no instance state or initializer.
    let toolbar: Retained<PebrelWindowControlsToolbar> = unsafe {
        msg_send![PebrelWindowControlsToolbar::alloc(native.mtm()), initWithIdentifier: ns_string!("pebrel-window-controls")]
    };
    native.setToolbarStyle(NSWindowToolbarStyle::Unified);
    native.setToolbar(Some(&toolbar));

    // AppKit retains the toolbar for this window. Selector observers are weak
    // (macOS 10.11+); the observer is not kept alive after the window closes.
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: the selectors above accept one NSNotification. The object filter
    // limits delivery to AppKit's main-thread transitions of this exact window.
    unsafe {
        center.addObserver_selector_name_object(
            &toolbar,
            sel!(pebrelWindowDidEnterFullScreen:),
            Some(NSWindowDidEnterFullScreenNotification),
            Some(&native),
        );
        center.addObserver_selector_name_object(
            &toolbar,
            sel!(pebrelWindowDidExitFullScreen:),
            Some(NSWindowDidExitFullScreenNotification),
            Some(&native),
        );
    }
}

pub(super) fn layout(window: &Window) -> TitleBarLayout {
    const WINDOWED_FALLBACK: TitleBarLayout =
        TitleBarLayout { height: 52.0, left_inset: 96.0, native_controls_visible: true };
    if window.is_fullscreen() {
        return TitleBarLayout { height: 52.0, left_inset: 8.0, native_controls_visible: false };
    }
    let Some(native) = native_window(window) else { return WINDOWED_FALLBACK };
    let Some(close) = native.standardWindowButton(NSWindowButton::CloseButton) else {
        return WINDOWED_FALLBACK;
    };
    let Some(zoom) = native.standardWindowButton(NSWindowButton::ZoomButton) else {
        return WINDOWED_FALLBACK;
    };
    let Some(content) = native.contentView() else { return WINDOWED_FALLBACK };
    let close = close.convertRect_toView(close.bounds(), Some(&content));
    let zoom = zoom.convertRect_toView(zoom.bounds(), Some(&content));
    control_layout(close, zoom, content.bounds(), content.isFlipped())
}

fn control_layout(close: NSRect, zoom: NSRect, content: NSRect, flipped: bool) -> TitleBarLayout {
    let center = close.origin.y + close.size.height / 2.0;
    let center_from_top = if flipped {
        center - content.origin.y
    } else {
        content.origin.y + content.size.height - center
    };
    TitleBarLayout {
        height: 2.0 * center_from_top as f32,
        left_inset: (zoom.origin.x + zoom.size.width - content.origin.x + 16.0) as f32,
        native_controls_visible: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::{NSPoint, NSSize};

    #[test]
    fn app_controls_follow_native_centers_and_leave_room_for_the_group() {
        for height in [600.0, 900.0] {
            for (inset, center) in [(19.0, 26.0), (12.0, 20.0)] {
                for flipped in [false, true] {
                    let content = NSRect::new(NSPoint::new(5.0, 3.0), NSSize::new(900.0, height));
                    let y = 3.0 + if flipped { center - 7.0 } else { height - center - 7.0 };
                    let close = NSRect::new(NSPoint::new(5.0 + inset, y), NSSize::new(14.0, 14.0));
                    let zoom = NSRect::new(NSPoint::new(5.0 + inset + 46.0, y), close.size);
                    let layout = control_layout(close, zoom, content, flipped);
                    assert_eq!(layout.height / 2.0, center as f32);
                    assert_eq!(layout.left_inset, (inset + 46.0 + 14.0 + 16.0) as f32);
                    assert!(layout.native_controls_visible);
                }
            }
        }
    }
}
