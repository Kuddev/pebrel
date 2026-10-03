//! One sizing rule for the initial native window and its first terminal grid.
//!
//! Measure the configured base font, not persisted terminal zoom. Where the
//! platform exposes display DPI, supply the final size before the window is
//! shown; otherwise keep the existing post-creation sizing fallback.

use gpui::{App, Bounds, Pixels, Point, Size, Window, WindowBounds, WindowOptions, point, px, size};
use super::{WindowRole, WorkspaceStartup};
use crate::gpui_shell::config::StartupWindow;

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

fn display_limit(window: &Window, cx: &App) -> Option<Size<Pixels>> {
    window.display(cx).map(|display| display.visible_bounds().size * 0.95)
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
            cx.primary_display().map(|display| display.visible_bounds().size * 0.95),
        ),
        cx,
    ))
}

fn fit_bounds(preferred: Size<Pixels>, origin: Option<Point<Pixels>>, visible: Bounds<Pixels>) -> Bounds<Pixels> {
    let fitted = fit_native_size(preferred, Some(visible.size));
    let origin = origin.unwrap_or_else(|| point(
        visible.origin.x + (visible.size.width - fitted.width) / 2.0,
        visible.origin.y + (visible.size.height - fitted.height) / 2.0,
    ));
    Bounds::new(point(
        px(origin.x.as_f32().clamp(visible.left().as_f32(), (visible.right() - fitted.width).as_f32().max(visible.left().as_f32()))),
        px(origin.y.as_f32().clamp(visible.top().as_f32(), (visible.bottom() - fitted.height).as_f32().max(visible.top().as_f32()))),
    ), fitted)
}

pub(super) fn load_restore(id: u64, startup: &WorkspaceStartup, role: WindowRole, cx: &mut App) {
    if id != 1 || role != WindowRole::Regular || !matches!(startup, WorkspaceStartup::RestoreOrDefault)
        || crate::platform::elevation::requires_isolation() { return; }
    let restored = crate::session::load().and_then(|session| session.startup_window_state());
    if let Some(config) = cx.try_global_mut::<StartupWindow>() { config.restored = restored; }
}

pub(in crate::gpui_shell::workspace) fn restored_size(id: u64, startup: &WorkspaceStartup, cx: &App) -> bool {
    id == 1 && matches!(startup, WorkspaceStartup::RestoreOrDefault)
        && cx.try_global::<StartupWindow>().is_some_and(|config| config.dimensions.is_none() && config.restored.is_some())
}

pub(super) fn regular_options(id: u64, focus: bool, sidebar_width: f32, cx: &mut App) -> WindowOptions {
    let config = cx.try_global::<StartupWindow>();
    let restored = (id == 1).then(|| config.and_then(|config| config.restored)).flatten();
    let configured = config.and_then(|config| config.position).map(|position| (position.x, position.y));
    let physical = configured.or_else(|| restored.and_then(|window| window.position));
    let physical_display = configured.and_then(crate::platform::startup::physical_display);
    let displays = cx.displays();
    let selected = physical_display.and_then(|(id, _)| displays.iter().find(|display| u64::from(display.id()) == id).cloned())
        .or_else(|| restored.and_then(|window| window.display).and_then(|uuid| displays.iter().find(|display| display.uuid().is_ok_and(|id| id.as_bytes() == &uuid)).cloned()))
        .or_else(|| physical.and_then(crate::platform::startup::physical_display).and_then(|(id, _)| displays.iter().find(|display| u64::from(display.id()) == id).cloned()))
        .or_else(|| cx.primary_display());
    let display_id = selected.as_ref().map(|display| display.id());
    let scale = display_id.and_then(|id| crate::platform::startup::display_scale(u64::from(id)));
    let visible = selected.map(|display| display.visible_bounds()).unwrap_or_else(|| Bounds::centered(None, size(px(1080.0), px(720.0)), cx));
    let configured_dimensions = config.is_some_and(|config| config.dimensions.is_some());
    let preferred = if !configured_dimensions && let Some(restored) = restored {
        size(px(restored.width as f32), px(restored.height as f32))
    } else if let Some(scale) = scale {
        default_size(TerminalView::startup_cell_metrics_at_scale(scale, cx), crate::gpui_shell::config::startup_grid(cx), sidebar_width, Some(visible.size * 0.95))
    } else { preferred_size(cx, sidebar_width).unwrap_or_else(|| size(px(1080.0), px(720.0))) };
    let origin = physical.zip(scale).map(|((x, y), scale)| point(px(x as f32 / scale), px(y as f32 / scale)));
    let mut bounds = fit_bounds(preferred, origin, visible);
    if let Some(scale) = scale {
        let offset = crate::platform::startup::placement_offset(u64::from(display_id.unwrap()));
        bounds.origin -= point(px(offset.0 as f32 / scale), px(offset.1 as f32 / scale));
    }
    let maximized = !configured_dimensions && configured.is_none() && restored.is_some_and(|window| window.maximized);
    crate::platform::window_chrome::configure_options(WindowOptions {
        window_bounds: Some(if maximized { WindowBounds::Maximized(bounds) } else { WindowBounds::Windowed(bounds) }),
        window_min_size: Some(size(px(760.0), px(540.0)).min(&bounds.size)),
        titlebar: Some(gpui_component::TitleBar::title_bar_options()),
        app_id: Some("pebrel".to_owned()),
        window_background: crate::gpui_shell::wallpaper::initial_background_appearance(),
        focus,
        display_id,
        ..Default::default()
    })
}

pub(super) fn capture_window(window: &Window, cx: &App) -> crate::session::WindowState {
    let normal = window.window_bounds();
    let bounds = normal.get_bounds();
    let display = window.display(cx);
    #[cfg(windows)]
    let position = display.as_ref().map(|display| {
        let offset = crate::platform::startup::placement_offset(u64::from(display.id()));
        ((bounds.origin.x.as_f32() * window.scale_factor()).round() as i32 + offset.0,
         (bounds.origin.y.as_f32() * window.scale_factor()).round() as i32 + offset.1)
    });
    #[cfg(not(windows))]
    let position = None;
    crate::session::WindowState {
        width: bounds.size.width.as_f32().round().max(1.0) as u32,
        height: bounds.size.height.as_f32().round().max(1.0) as u32,
        maximized: matches!(normal, WindowBounds::Maximized(_)),
        position,
        display: display.and_then(|display| display.uuid().ok()).map(|id| *id.as_bytes()),
    }
}

pub(super) fn apply_configured_position(window: &Window, cx: &App) {
    #[cfg(windows)]
    if let Some(position) = cx.try_global::<StartupWindow>().and_then(|config| config.position)
        && let Some(hwnd) = super::native_hwnd(window)
        && !crate::platform::startup::place_normal_window(hwnd, (position.x, position.y)) {
        log::warn!("Could not apply configured window position");
    }
    #[cfg(not(windows))]
    let _ = (window, cx);
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
            crate::gpui_shell::config::startup_grid(cx),
            sidebar_width,
            display_limit(window, cx),
        );
        let requested = if crate::platform::startup::primary_display_scale().is_some() {
            fit_native_size(requested, window.display(cx).map(|display| display.visible_bounds().size))
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
    fn restoring_a_large_legal_window_does_not_shrink_it() {
        let visible = Bounds::new(point(px(0.0), px(0.0)), size(px(1920.0), px(1040.0)));
        let bounds = fit_bounds(size(px(1900.0), px(1030.0)), Some(point(px(10.0), px(5.0))), visible);
        assert_eq!(bounds.size, size(px(1900.0), px(1030.0)));
        assert_eq!(bounds.origin, point(px(10.0), px(5.0)));
    }

    #[test]
    fn restored_bounds_stay_visible_on_the_selected_negative_origin_display() {
        let visible = Bounds::new(point(px(-1920.0), px(40.0)), size(px(1920.0), px(1040.0)));
        let bounds = fit_bounds(size(px(1300.0), px(800.0)), Some(point(px(-1700.0), px(120.0))), visible);
        assert_eq!(bounds.origin, point(px(-1700.0), px(120.0)));
        assert_eq!(bounds.size, size(px(1300.0), px(800.0)));
        let offscreen = fit_bounds(size(px(3000.0), px(2000.0)), Some(point(px(9000.0), px(-9000.0))), visible);
        assert!(visible.contains(&offscreen.origin));
        assert!(offscreen.right() <= visible.right());
        assert!(offscreen.bottom() <= visible.bottom());
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn restored_dimensions_apply_only_to_the_first_window_and_explicit_grid_wins(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            cx.set_global(StartupWindow { restored: Some(crate::session::WindowState { width: 900, height: 650, ..Default::default() }), ..Default::default() });
            assert!(restored_size(1, &WorkspaceStartup::RestoreOrDefault, cx));
            assert!(!restored_size(2, &WorkspaceStartup::RestoreOrDefault, cx));
            assert!(!restored_size(1, &WorkspaceStartup::Empty, cx));
            let options = regular_options(1, false, 230.0, cx);
            assert_eq!(options.window_bounds.unwrap().get_bounds().size, size(px(900.0), px(650.0)));
            cx.global_mut::<StartupWindow>().dimensions = Some(crate::config::window::Dimensions { columns: 155, lines: 43 });
            assert!(!restored_size(1, &WorkspaceStartup::RestoreOrDefault, cx));
        });
    }

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
}
