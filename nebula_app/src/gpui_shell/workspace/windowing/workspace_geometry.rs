//! 普通工作区窗口的几何持久化：观测只标脏，quit/autosave/关窗统一
//! 落盘（与 quick 终端尺寸同一模式）；恢复交给
//! [`super::startup_geometry::restored_window_bounds`] 的 clamp 规则。

use gpui::{App, Window, WindowBounds};

use super::WindowRegistry;

/// 恢复普通窗口的上次几何；无存档或无显示器时返回 None，调用方走
/// 既有居中默认。
pub(in crate::gpui_shell::workspace) fn restored_regular_bounds(
    cx: &App,
) -> Option<(gpui::Bounds<gpui::Pixels>, bool)> {
    let visible_list: Vec<gpui::Bounds<gpui::Pixels>> =
        cx.displays().iter().map(|display| display.visible_bounds()).collect();
    nebula_settings::RuntimeSettings::load().window_geometry.as_ref().and_then(|geometry| {
        super::startup_geometry::restored_window_bounds(geometry, &visible_list)
    })
}

/// 几何与 quick 尺寸同构：观测只标脏，落盘集中在统一钩子里。
pub(in crate::gpui_shell::workspace) fn persist_window_geometry(cx: &mut App) {
    let Some(geometry) = cx.global::<WindowRegistry>().window_geometry_dirty else { return };
    match nebula_settings::persist_keys(&geometry.persisted_values()) {
        Ok(()) => cx.global_mut::<WindowRegistry>().window_geometry_dirty = None,
        Err(error) => log::warn!("could not save window geometry: {error}"),
    }
}

/// 普通工作区窗口的几何观测；quick 窗口由 quick 侧自己处理，这里跳过。
/// 全屏（F11）不记：它是独立的呈现状态，退出时不该被还原成全屏。
pub(in crate::gpui_shell::workspace) fn workspace_window_geometry_changed(
    id: u64,
    window: &mut Window,
    cx: &mut App,
) {
    let registry = cx.global::<WindowRegistry>();
    if registry.quick_terminal.as_ref().is_some_and(|quick| quick.runtime_window_id == id) {
        return;
    }
    let (bounds, maximized) = match window.window_bounds() {
        WindowBounds::Windowed(bounds) => (bounds, false),
        WindowBounds::Maximized(bounds) => (bounds, true),
        WindowBounds::Fullscreen(_) => return,
    };
    let Some(geometry) = nebula_settings::WindowGeometry::new(
        f32::from(bounds.origin.x),
        f32::from(bounds.origin.y),
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
        maximized,
    ) else {
        return;
    };
    cx.global_mut::<WindowRegistry>().window_geometry_dirty = Some(geometry);
}
