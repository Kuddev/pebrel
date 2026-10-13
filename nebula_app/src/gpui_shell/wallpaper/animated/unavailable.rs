//! 没有原生播放后端时保留设置与明确错误，不创建解码器、计时器或 GPU 所有者。
use super::super::{VisualEffects, WallpaperLayout, show_media_error, show_shader_error};
use gpui::{App, Bounds, Pixels, Window};
#[derive(Default)]
pub(in crate::gpui_shell::wallpaper) struct Animated {
    effect_config: nebula_settings::BackgroundEffects,
}
pub(in crate::gpui_shell::wallpaper) fn configure(
    rt: &nebula_settings::RuntimeSettings,
    source_changed: bool,
    cx: &mut App,
) {
    let state = &mut cx.global_mut::<VisualEffects>().animated;
    let changed = state.effect_config != rt.background_effects;
    state.effect_config = rt.background_effects.clone();
    if changed && rt.background_effects.preset() != "off" {
        show_shader_error(cx);
    }
    if source_changed && rt.background_media_kind.is_animated() {
        show_media_error(rt.background_media_kind, cx);
    }
}
pub(in crate::gpui_shell::wallpaper) fn source_failed(_: &App) -> bool {
    false
}
pub(in crate::gpui_shell::wallpaper) fn shader_ready(_: &App) -> bool {
    false
}
pub(in crate::gpui_shell::wallpaper) fn media_ready(_: &App) -> bool {
    false
}
pub(in crate::gpui_shell::wallpaper) fn video_available() -> bool {
    false
}
pub(in crate::gpui_shell::wallpaper) fn shader_available() -> bool {
    false
}
pub(in crate::gpui_shell::wallpaper) fn media_available(
    kind: nebula_settings::BackgroundMediaKind,
) -> bool {
    kind == nebula_settings::BackgroundMediaKind::Image
}
pub(in crate::gpui_shell::wallpaper) fn paint(
    _: Bounds<Pixels>,
    _: bool,
    _: WallpaperLayout,
    _: &mut Window,
    _: &mut App,
) -> bool {
    false
}
pub(in crate::gpui_shell::wallpaper) fn reload_media(_: &mut App) {}
pub(in crate::gpui_shell::wallpaper) fn reload_shader(cx: &mut App) {
    show_shader_error(cx);
}
