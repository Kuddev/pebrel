//! One sizing rule for the initial native window and its first terminal grid.
//!
//! Measure the configured base font, not persisted terminal zoom. Where the
//! platform exposes display DPI, supply the final size before the window is
//! shown; otherwise keep the existing post-creation sizing fallback.

use gpui::{App, Pixels, Size, Window, px, size};

use crate::gpui_shell::terminal::view::TerminalView;

fn chrome_size(sidebar_width: f32) -> Size<Pixels> {
    // Sidebar, gutters, terminal padding and the existing 2px rounding allowance.
    size(px(sidebar_width + 16.0 + 24.0 + 2.0), px(34.0 + 16.0 + 16.0 + 2.0))
}

fn default_size(
    metrics: (Pixels, Pixels),
    sidebar_width: f32,
    display_limit: Option<Size<Pixels>>,
) -> Size<Pixels> {
    let chrome = chrome_size(sidebar_width);
    let preferred = size(
        metrics.0 * f32::from(TerminalView::DEFAULT_GRID_COLUMNS) + chrome.width,
        metrics.1 * f32::from(TerminalView::DEFAULT_GRID_LINES) + chrome.height,
    );
    display_limit.map_or(preferred, |limit| preferred.min(&limit))
}

fn display_limit(cx: &App) -> Option<Size<Pixels>> {
    cx.primary_display().map(|display| {
        let bounds = display.bounds().size;
        size(bounds.width * 0.95, bounds.height * 0.95)
    })
}

fn fit_native_size(preferred: Size<Pixels>, visible: Option<Size<Pixels>>) -> Size<Pixels> {
    // Keep the existing resize floor even for a very small configured font.
    // A display smaller than that floor still owns the upper bound.
    let preferred = preferred.max(&size(px(760.0), px(540.0)));
    visible.map_or(preferred, |visible| preferred.min(&visible))
}

fn fit_preflight_size(preferred: Size<Pixels>, cx: &App) -> Size<Pixels> {
    fit_native_size(preferred, cx.primary_display().map(|display| display.visible_bounds().size))
}

pub(super) fn preferred_size(cx: &App, sidebar_width: f32) -> Option<Size<Pixels>> {
    let scale = crate::platform::startup::primary_display_scale()?;
    Some(fit_preflight_size(
        default_size(
            TerminalView::startup_cell_metrics_at_scale(scale, cx),
            sidebar_width,
            display_limit(cx),
        ),
        cx,
    ))
}

fn same_device_size(actual: Size<Pixels>, requested: Size<Pixels>, scale: f32) -> bool {
    // Window placement quantizes logical sizes to device pixels. A subpixel
    // round-trip difference must not schedule a second asynchronous SetWindowPos.
    (f32::from(actual.width - requested.width) * scale).abs() < 1.0
        && (f32::from(actual.height - requested.height) * scale).abs() < 1.0
}

pub(in crate::gpui_shell::workspace) fn prepare_initial_grid(
    window: &mut Window,
    cx: &mut App,
    sidebar_width: f32,
    fit_window_to_default_grid: bool,
) -> (u16, u16) {
    let (cell_w, line_h) = TerminalView::cell_metrics(window, cx);
    let actual = window.bounds().size;
    let target = if fit_window_to_default_grid {
        let requested = default_size(
            TerminalView::startup_cell_metrics(window, cx),
            sidebar_width,
            display_limit(cx),
        );
        let requested = if crate::platform::startup::primary_display_scale().is_some() {
            fit_preflight_size(requested, cx)
        } else {
            // Keep the original sizing policy on platforms without preflight DPI.
            requested
        };
        if same_device_size(actual, requested, window.scale_factor()) {
            actual
        } else {
            // DPI changes during creation, failed preflight and unsupported
            // platforms retain the measured-window fallback instead of guessing.
            window.resize(requested);
            requested
        }
    } else {
        // Quick Terminal owns its monitor geometry and slide-in animation.
        actual
    };
    let chrome = chrome_size(sidebar_width);
    let cols = ((f32::from(target.width - chrome.width) / f32::from(cell_w)) + 0.001)
        .floor()
        .max(2.0) as u16;
    let rows = ((f32::from(target.height - chrome.height) / f32::from(line_h)) + 0.001)
        .floor()
        .max(2.0) as u16;
    (cols, rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_window_keeps_the_base_font_grid_and_fits_the_display() {
        let metrics = (px(8.0), px(20.0));
        assert_eq!(default_size(metrics, 230.0, None), size(px(1200.0), px(668.0)));
        assert_eq!(
            default_size(metrics, 230.0, Some(size(px(1000.0), px(600.0)))),
            size(px(1000.0), px(600.0)),
        );
        assert_eq!(default_size(metrics, 300.0, None), size(px(1270.0), px(668.0)),);
    }

    #[test]
    fn startup_size_skips_only_device_pixel_rounding_differences() {
        let requested = size(px(1200.0), px(668.0));
        for scale in [1.0, 1.25, 1.5, 2.0] {
            assert!(same_device_size(requested, requested, scale));
            assert!(same_device_size(
                size(requested.width + px(0.5 / scale), requested.height),
                requested,
                scale,
            ));
            assert!(!same_device_size(size(px(1080.0), px(720.0)), requested, scale));
            assert!(!same_device_size(
                size(requested.width + px(2.0 / scale), requested.height),
                requested,
                scale,
            ));
        }
    }

    #[test]
    fn small_fonts_keep_the_resize_floor_unless_the_display_is_smaller() {
        let small_font = size(px(504.0), px(248.0));
        assert_eq!(fit_native_size(small_font, None), size(px(760.0), px(540.0)));
        assert_eq!(
            fit_native_size(small_font, Some(size(px(1920.0), px(1040.0)))),
            size(px(760.0), px(540.0)),
        );
        assert_eq!(
            fit_native_size(small_font, Some(size(px(640.0), px(480.0)))),
            size(px(640.0), px(480.0)),
        );
        assert_eq!(
            fit_native_size(size(px(1200.0), px(668.0)), Some(size(px(1000.0), px(600.0)))),
            size(px(1000.0), px(600.0)),
        );
    }
}
