use crate::display::color::Rgb;
use crate::display::ui::theme::Skin;
use crate::display::ui::widgets;
use crate::display::{truncate_tab_label, SizeInfo};
use crate::renderer::{GlyphCache, Renderer};
use unicode_width::UnicodeWidthChar;

pub(super) fn draw_big_text(
    r: &mut Renderer,
    gc: &mut GlyphCache,
    size: &SizeInfo,
    _scale: f32,
    x: f32,
    y: f32,
    mult: f32,
    ink: Rgb,
    text: &str,
) {
    r.draw_ui_text(size, x, y, mult, ink, nebula_terminal::term::cell::Flags::empty(), text, gc);
}

/// A group heading inside the content pane: clearly larger than row labels
/// (strict size hierarchy: page title 1.6× > group 1.2× > rows 1.0×) and in
/// the strong ink. One helper so every group shares one size/rhythm.
pub(super) fn section_title(
    r: &mut Renderer,
    gc: &mut GlyphCache,
    size: &SizeInfo,
    scale: f32,
    sk: &Skin,
    x: f32,
    y: f32,
    text: &str,
) {
    draw_big_text(r, gc, size, scale, x, y, 1.2, sk.ink_strong, text);
}

/// 网络页的分组标题属于测试横幅这一组；标题必须挂在横幅上方，不能再
/// 以代理方式行作为锚点，否则横幅提前后标题会被绘制到横幅内部。
pub(super) fn proxy_section_title_y(test_row_y: f32, scale: f32) -> f32 {
    test_row_y - 42.0 * scale
}

/// Keymap groups follow the prototype's quiet hierarchy: small, tracked-ish
/// captions and generous whitespace do the grouping work; no frame or filled
/// block is needed around a category.
pub(super) fn keymap_group_title(
    r: &mut Renderer,
    gc: &mut GlyphCache,
    size: &SizeInfo,
    x: f32,
    y: f32,
    text: &str,
    ink: Rgb,
) {
    r.draw_ui_text(size, x, y, 0.86, ink, nebula_terminal::term::cell::Flags::empty(), text, gc);
}

pub(super) fn warning_lines(note: &str, max_cols: usize) -> [String; 2] {
    let mut lines = [String::new(), String::new()];
    let mut line = 0usize;
    let mut used = 0usize;
    let mut remaining = false;
    for ch in note.chars() {
        let width = ch.width().unwrap_or(1).max(1);
        if used + width > max_cols {
            if line == 0 {
                line = 1;
                used = 0;
            } else {
                remaining = true;
                break;
            }
        }
        lines[line].push(ch);
        used += width;
    }
    if remaining && !lines[1].is_empty() {
        let _ = lines[1].pop();
        lines[1].push('…');
    }
    lines
}

/// Draw a settings row: a left-aligned label and a right-aligned, truncated
/// value, both vertically centered. Labels are single-line by design — any
/// explanation must fit the label itself (rows with obvious semantics carry
/// no description at all). Inks come from the active theme's [`Skin`].
#[allow(clippy::too_many_arguments)]
pub(super) fn row_label(
    r: &mut Renderer,
    gc: &mut GlyphCache,
    size: &SizeInfo,
    scale: f32,
    sk: &Skin,
    (rx, ry, rw, rh): (f32, f32, f32, f32),
    k: &str,
    v: &str,
    value_ink: Rgb,
) {
    row_label_with_right_inset(r, gc, size, scale, sk, (rx, ry, rw, rh), k, v, value_ink, 0.0);
}

pub(super) fn draw_button_label(
    r: &mut Renderer,
    gc: &mut GlyphCache,
    size: &SizeInfo,
    rect: (f32, f32, f32, f32),
    label: &str,
    ink: Rgb,
) {
    let cell_w = size.cell_width();
    let cell_h = size.cell_height();
    let cols = label.chars().map(|ch| ch.width().unwrap_or(1)).sum::<usize>();
    r.draw_chrome_text(
        size,
        rect.0 + (rect.2 - cols as f32 * cell_w) * 0.5,
        widgets::centered_y(rect.1, rect.3, cell_h),
        ink,
        label,
        gc,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn row_label_with_right_inset(
    r: &mut Renderer,
    gc: &mut GlyphCache,
    size: &SizeInfo,
    scale: f32,
    sk: &Skin,
    (rx, ry, rw, rh): (f32, f32, f32, f32),
    k: &str,
    v: &str,
    value_ink: Rgb,
    right_inset: f32,
) {
    let s = |val: f32| val * scale;
    let cell_w = size.cell_width();
    let cell_h = size.cell_height();
    if rh >= s(56.0) {
        // 窄屏行明确分为两层：标签占稳定的上基线，值或控件独占下一层。
        let label_y = ry + s(9.0);
        r.draw_chrome_text(size, rx + s(16.0), label_y, sk.ink, k, gc);
        let value_left = rx + s(16.0);
        let value_right = rx + rw - s(16.0) - right_inset;
        let max_chars = ((value_right - value_left).max(cell_w) / cell_w).floor().max(1.0) as usize;
        let value = truncate_tab_label(v, max_chars);
        if !value.is_empty() {
            r.draw_chrome_text(size, value_left, ry + s(9.0) + cell_h, value_ink, &value, gc);
        }
        return;
    }
    let ty = ry + (rh - cell_h) / 2.0;
    r.draw_chrome_text(size, rx + s(16.0), ty, sk.ink, k, gc);
    let value_left = rx + rw * 0.42;
    let value_right = rx + rw - s(16.0) - right_inset;
    let max_chars = ((value_right - value_left).max(cell_w) / cell_w).floor().max(1.0) as usize;
    let value = truncate_tab_label(v, max_chars);
    let value_cols: usize = value.chars().map(|c| c.width().unwrap_or(0)).sum();
    let vx = value_right - value_cols as f32 * cell_w;
    r.draw_chrome_text(size, vx.max(value_left), ty, value_ink, &value, gc);
}
