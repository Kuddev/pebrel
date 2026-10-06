//! 没有后处理后端时不创建 actor；保持与原平台缺省路径相同的绘制与可见性行为。
use super::{colors::Palette, cursor_painter::CursorPaint, view::TerminalView};
use gpui::{App, Bounds, Context, Entity, Pixels, Window};
use nebula_terminal::term::color::Colors;

// 此类型没有可构造值，确保不支持的平台始终没有伪造的效果 entity。
pub(super) type TerminalEffect = std::convert::Infallible;

pub(super) fn visibility_changed(_: &TerminalView, _: &mut Context<TerminalView>) {}

pub(super) fn paint(
    _: &Entity<TerminalView>,
    _: Bounds<Pixels>,
    _: &Palette,
    _: &Colors,
    _: Option<&CursorPaint>,
    _: bool,
    _: &mut Window,
    _: &mut App,
) {
}
