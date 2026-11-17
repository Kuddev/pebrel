//! 网格单格字形的列宽适配。

use gpui::{Pixels, SharedString, TextRun};

/// 单列的 East Asian Ambiguous 字符（①②★…）在主字体缺字时会落到 CJK 字体的
/// 全宽字形，右半截压进邻格。终端不能把格子改成 2 列（应用按 1 列排版），所以
/// 按字形自然宽度重新塑形成一列宽，与旧壳 `load_glyph` 的 1.4× 判据一致；
/// Private Use Area（Nerd Font 图标）刻意越格拼接，不缩。
pub(super) fn shape_cell(
    text_system: &gpui::WindowTextSystem,
    text: &str,
    font_size: Pixels,
    run: &TextRun,
    cell_width: Pixels,
) -> gpui::ShapedLine {
    use unicode_width::UnicodeWidthChar as _;

    // 两次塑形只读取相同样式，借用即可，避免逐格绘制时重复克隆 TextRun。
    let shaped = text_system.shape_line(
        SharedString::from(text.to_owned()),
        font_size,
        std::slice::from_ref(run),
        None,
    );
    let mut chars = text.chars();
    let single = chars.next().filter(|_| chars.next().is_none());
    let overflows = single.is_some_and(|ch| {
        ch.width() == Some(1)
            && !matches!(u32::from(ch), 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
    }) && shaped.width > cell_width * 1.4;
    if !overflows {
        return shaped;
    }
    let fitted = font_size * (cell_width / shaped.width);
    text_system.shape_line(
        SharedString::from(text.to_owned()),
        fitted,
        std::slice::from_ref(run),
        None,
    )
}
