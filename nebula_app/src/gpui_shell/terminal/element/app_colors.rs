//! Theme correction for colors written by applications: contrast against the
//! actual cell background and remapping of old theme surfaces. Pure data in,
//! pure data out; [`super::TerminalElement`] only supplies the resolver owned
//! by the view.

use nebula_terminal::render::RenderSnapshot;
use nebula_terminal::term::color::Colors;
use nebula_terminal::vte::ansi::Color;

use super::{Palette, rgb_from_rgba};
use crate::display::terminal_color::TerminalColorResolver;

/// [`super::TerminalElement::resolve_app_colors`] 的实际逻辑（脱开 GPUI 实体，可测）。
pub(super) fn resolve_app_colors_into(
    snap: &mut RenderSnapshot,
    theme: &Palette,
    overrides: &Colors,
    resolver: &mut TerminalColorResolver,
) {
    use crate::display::content::is_terminal_graphic;
    use crate::display::terminal_color::is_fixed_color;

    for run in &mut snap.bg_runs {
        let base = rgb_from_rgba(theme.resolve(run.color, overrides, false));
        let resolved = resolver.resolve_background(base, is_fixed_color(run.color, overrides));
        if resolved != base {
            run.color = Color::Spec(resolved.0);
        }
    }
    for cell in snap.segments.iter_mut().flat_map(|segment| segment.cells.iter_mut()) {
        // 图形字符的颜色表达图形本身，不是正文对比度——图标被「矫正」成另一个
        // 颜色就是另一张图了。
        let graphic = cell.text.chars().next().is_some_and(is_terminal_graphic);
        if graphic {
            continue;
        }
        // 对比度是一对颜色的属性：这个前景可不可读，取决于它**这一格**底下是
        // 什么，而不是主题底色。默认底色的格子没有 bg run，所以 `SnapCell::bg`
        // 单独带着这个值。
        if let Some(fg) = resolve_text_foreground(
            cell.fg, cell.bg, cell.bold, cell.dim, theme, overrides, resolver,
        ) {
            cell.fg = fg;
            cell.dim = false;
        }
    }
}

/// `Some` when contrast adjustment replaced the color. The returned `Spec`
/// already carries bold and dim, so the caller must clear `dim`: painting
/// dims a `Spec` again, while bold leaves it alone.
pub(super) fn resolve_text_foreground(
    fg: Color,
    bg: Color,
    bold: bool,
    dim: bool,
    theme: &Palette,
    overrides: &Colors,
    resolver: &mut TerminalColorResolver,
) -> Option<Color> {
    use crate::display::terminal_color::is_fixed_color;
    let bg_base = rgb_from_rgba(theme.resolve(bg, overrides, false));
    let bg = resolver.resolve_background(bg_base, is_fixed_color(bg, overrides));
    // Resolve bold and dim before contrast adjustment; Spec must retain both.
    let base = rgb_from_rgba(theme.resolve_styled(fg, overrides, bold, dim));
    let resolved = resolver.resolve_foreground(
        base,
        bg,
        true,
        rgb_from_rgba(theme.foreground),
        rgb_from_rgba(theme.background),
    );
    (resolved != base).then_some(Color::Spec(resolved.0))
}
