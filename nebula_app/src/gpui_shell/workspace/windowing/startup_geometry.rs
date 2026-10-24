//! One sizing rule for the initial native window and its first terminal grid.
//!
//! Measure the configured base font, not persisted terminal zoom. Where the
//! platform exposes display DPI, supply the final size before the window is
//! shown; otherwise keep the existing post-creation sizing fallback.

use gpui::{App, Bounds, Pixels, Size, Window, point, px, size};

use crate::gpui_shell::terminal::view::TerminalView;

fn chrome_size(sidebar_width: f32) -> Size<Pixels> {
    // Sidebar, gutters, terminal padding and the existing 2px rounding allowance.
    size(px(sidebar_width + 16.0 + 24.0 + 2.0), px(34.0 + 16.0 + 16.0 + 2.0))
}

fn default_size(
    metrics: (Pixels, Pixels),
    grid: (f32, f32),
    sidebar_width: f32,
    display_limit: Option<Size<Pixels>>,
) -> Size<Pixels> {
    let chrome = chrome_size(sidebar_width);
    let preferred = size(metrics.0 * grid.0 + chrome.width, metrics.1 * grid.1 + chrome.height);
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
            crate::gpui_shell::config::startup_grid(cx),
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

/// 把持久化的窗口几何恢复成平台 bounds：几何原点所在的显示器还在就
/// 原位恢复；不在（拔掉/换分辨率）就把窗口完整拉回第一块（主屏）可
/// 见区。尺寸不超可见区；最大化时返回的 bounds 是还原框，交给平台的
/// 最大化语义。无显示器时返回 None，调用方走既有居中默认。
pub(super) fn restored_window_bounds(
    geometry: &nebula_settings::WindowGeometry,
    visible_list: &[Bounds<Pixels>],
) -> Option<(Bounds<Pixels>, bool)> {
    let host = visible_list
        .iter()
        .find(|visible| {
            let (x, y) = (geometry.x, geometry.y);
            let (left, top) = (f32::from(visible.origin.x), f32::from(visible.origin.y));
            let (right, bottom) =
                (left + f32::from(visible.size.width), top + f32::from(visible.size.height));
            x >= left && x < right && y >= top && y < bottom
        })
        .or_else(|| visible_list.first())?;
    let host_w = f32::from(host.size.width);
    let host_h = f32::from(host.size.height);
    let width = geometry.width.min(host_w);
    let height = geometry.height.min(host_h);
    let host_left = f32::from(host.origin.x);
    let host_top = f32::from(host.origin.y);
    let x = geometry.x.clamp(host_left, host_left + host_w - width);
    let y = geometry.y.clamp(host_top, host_top + host_h - height);
    Some((
        Bounds { origin: point(px(x), px(y)), size: size(px(width), px(height)) },
        geometry.maximized,
    ))
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
            crate::gpui_shell::config::startup_grid(cx),
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
        let grid = (
            f32::from(TerminalView::DEFAULT_GRID_COLUMNS),
            f32::from(TerminalView::DEFAULT_GRID_LINES),
        );
        assert_eq!(default_size(metrics, grid, 230.0, None), size(px(1200.0), px(668.0)));
        assert_eq!(
            default_size(metrics, grid, 230.0, Some(size(px(1000.0), px(600.0)))),
            size(px(1000.0), px(600.0)),
        );
        assert_eq!(default_size(metrics, grid, 300.0, None), size(px(1270.0), px(668.0)),);
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

    #[test]
    fn restored_bounds_keep_the_saved_position_on_the_occupied_display() {
        let geometry =
            nebula_settings::WindowGeometry::new(100.0, 50.0, 1200.0, 800.0, false).unwrap();
        let visible = [visible(0.0, 0.0, 1920.0, 1080.0)];
        let (bounds, maximized) = restored_window_bounds(&geometry, &visible).unwrap();
        assert_eq!(
            bounds,
            Bounds { origin: point(px(100.0), px(50.0)), size: size(px(1200.0), px(800.0)) }
        );
        assert!(!maximized);
    }

    #[test]
    fn restored_bounds_return_to_the_first_display_when_the_saved_one_is_gone() {
        let geometry =
            nebula_settings::WindowGeometry::new(-1920.0, 100.0, 1200.0, 800.0, true).unwrap();
        let visible = [visible(0.0, 0.0, 1920.0, 1080.0)];
        let (bounds, maximized) = restored_window_bounds(&geometry, &visible).unwrap();
        assert_eq!(bounds.origin, point(px(0.0), px(100.0)));
        assert_eq!(bounds.size, size(px(1200.0), px(800.0)));
        assert!(maximized);
    }

    #[test]
    fn restored_bounds_shrink_oversized_windows_into_the_visible_area() {
        let geometry =
            nebula_settings::WindowGeometry::new(0.0, 0.0, 4096.0, 2048.0, false).unwrap();
        let visible = [visible(0.0, 0.0, 1920.0, 1080.0)];
        let (bounds, _) = restored_window_bounds(&geometry, &visible).unwrap();
        assert_eq!(
            bounds,
            Bounds { origin: point(px(0.0), px(0.0)), size: size(px(1920.0), px(1080.0)) }
        );

        assert_eq!(restored_window_bounds(&geometry, &[]), None, "无显示器时交回默认居中路径");
    }

    fn visible(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds { origin: point(px(x), px(y)), size: size(px(width), px(height)) }
    }
}
