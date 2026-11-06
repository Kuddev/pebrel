//! 逐格反色选区：把渲染快照里的选区 run 解析成「块填前景、字用背景」的配色。
//!
//! 契约见 `.docs/adr/0002-selection-inverse-rendering.md`：开关打开时主题与用户
//! 的选区配色整体让位，选区里每一格用**自己**的前景铺不透明块、用自己的背景画
//! 字——彩色文字因此保留色相，深浅主题也自动成立。

use gpui::Rgba;
use nebula_terminal::render::CellRun;
use nebula_terminal::term::color::Colors;

use super::colors::Palette;

/// 一条已解析的反色 run：`fill` 铺块，`text` 画块内的字。
#[derive(Clone, Copy, Debug)]
pub(super) struct InverseRun {
    pub(super) row: u16,
    pub(super) start: u16,
    pub(super) end: u16,
    pub(super) fill: Rgba,
    pub(super) text: Rgba,
}

pub(super) fn resolve(runs: &[CellRun], palette: &Palette, overrides: &Colors) -> Vec<InverseRun> {
    runs.iter()
        .map(|run| InverseRun {
            row: run.row,
            start: run.start,
            end: run.end,
            fill: palette.resolve_styled(run.fg, overrides, run.bold, run.dim),
            // SGR 2 的底色走 bg_dim：块内的字是"交换后的底色"，暗色背景反成暗字。
            text: palette.resolve_styled(run.bg, overrides, false, run.bg_dim),
        })
        .collect()
}

/// 选区里某一格的反色文字色；不在选区里返回 `None`，调用方回落到正常解析。
pub(super) fn text_color(runs: &[InverseRun], row: u16, col: u16) -> Option<Rgba> {
    runs.iter().find(|run| run.row == row && run.start <= col && col < run.end).map(|run| run.text)
}

/// 块要铺的列区间：整段铺满（空格与行尾空白同样着块），只跳过光标接管的那几格。
pub(super) fn block_columns(run: &InverseRun, cursor: Option<(u16, u16, bool)>) -> Vec<(u16, u16)> {
    let Some((row, col, wide)) = cursor.filter(|(row, col, _)| *row == run.row && *col < run.end)
    else {
        return vec![(run.start, run.end)];
    };
    let skip_end = col.saturating_add(if wide { 2 } else { 1 });
    if skip_end <= run.start || col >= run.end {
        return vec![(run.start, run.end)];
    }
    let mut parts = Vec::new();
    if run.start < col {
        parts.push((run.start, col));
    }
    if skip_end < run.end {
        parts.push((skip_end, run.end));
    }
    parts
}

/// 逐条下发要铺的块：`inverse` 为 `None` 时整片共用主题选区色，否则逐格取自己的配色。
/// 前端只需把 `(行, 起列, 止列, 颜色)` 换成矩形。
pub(super) fn paint_blocks(
    runs: &[CellRun],
    inverse: Option<&[InverseRun]>,
    theme_fill: Rgba,
    cursor: Option<(u16, u16, bool)>,
    mut paint: impl FnMut(u16, u16, u16, Rgba),
) {
    for (index, run) in runs.iter().enumerate() {
        let (spans, color) = match inverse {
            Some(resolved) => (block_columns(&resolved[index], cursor), resolved[index].fill),
            None => (vec![(run.start, run.end)], theme_fill),
        };
        for (start, end) in spans {
            paint(run.row, start, end, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nebula_terminal::term::color::Colors;
    use nebula_terminal::vte::ansi::{Color, NamedColor};

    fn run(fg: Color, bg: Color) -> CellRun {
        CellRun { row: 0, start: 0, end: 1, fg, bg, bold: false, dim: false, bg_dim: false }
    }

    #[test]
    fn block_takes_the_cell_foreground_and_text_the_cell_background() {
        let palette = Palette::default();
        let overrides = Colors::default();
        let runs = resolve(
            &[run(Color::Named(NamedColor::Red), Color::Named(NamedColor::Background))],
            &palette,
            &overrides,
        );
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].fill, palette.ansi[1]);
        assert_eq!(runs[0].text, palette.background);
    }

    #[test]
    fn inverted_cells_reverse_back_to_their_original_pair() {
        let palette = Palette::default();
        let overrides = Colors::default();
        // SGR 7 已在快照里交换过：块用背景色，字用前景色，视觉上回到原样。
        let runs = resolve(
            &[run(Color::Named(NamedColor::Background), Color::Named(NamedColor::Foreground))],
            &palette,
            &overrides,
        );
        assert_eq!(runs[0].fill, palette.background);
        assert_eq!(runs[0].text, palette.foreground);
    }

    #[test]
    fn dim_background_dims_the_text_inside_the_block() {
        let palette = Palette::default();
        let overrides = Colors::default();
        let mut cell =
            run(Color::Named(NamedColor::Foreground), Color::Named(NamedColor::Background));
        cell.bg = Color::Named(NamedColor::Foreground);
        cell.bg_dim = true;
        cell.fg = Color::Named(NamedColor::Background);
        let runs = resolve(&[cell], &palette, &overrides);
        assert_eq!(runs[0].text, palette.dim_foreground);
    }

    #[test]
    fn text_color_is_scoped_to_the_selected_columns() {
        let palette = Palette::default();
        let overrides = Colors::default();
        let runs = resolve(
            &[
                CellRun {
                    row: 2,
                    start: 0,
                    end: 3,
                    fg: Color::Named(NamedColor::Green),
                    bg: Color::Named(NamedColor::Background),
                    bold: false,
                    dim: false,
                    bg_dim: false,
                },
                CellRun {
                    row: 2,
                    start: 5,
                    end: 6,
                    fg: Color::Named(NamedColor::Background),
                    bg: Color::Named(NamedColor::Foreground),
                    bold: false,
                    dim: false,
                    bg_dim: false,
                },
            ],
            &palette,
            &overrides,
        );
        assert_eq!(text_color(&runs, 2, 0), Some(palette.background));
        assert_eq!(text_color(&runs, 2, 2), Some(palette.background));
        assert_eq!(text_color(&runs, 2, 5), Some(palette.foreground));
        assert_eq!(text_color(&runs, 2, 3), None, "列间隙不属于任何 run");
        assert_eq!(text_color(&runs, 1, 0), None, "非选中行不参与反色");
    }

    fn one_run(start: u16, end: u16) -> InverseRun {
        InverseRun { row: 0, start, end, fill: Rgba::default(), text: Rgba::default() }
    }

    #[test]
    fn blocks_fill_the_whole_run_and_leave_the_cursor_cell_alone() {
        let run = one_run(0, 4);
        assert_eq!(block_columns(&run, None), vec![(0, 4)]);
        assert_eq!(block_columns(&run, Some((0, 1, false))), vec![(0, 1), (2, 4)]);
        assert_eq!(block_columns(&run, Some((1, 1, false))), vec![(0, 4)], "别的行不影响");
        assert_eq!(block_columns(&run, Some((0, 9, false))), vec![(0, 4)], "run 之外的列不影响");
        assert_eq!(block_columns(&run, Some((0, 0, true))), vec![(2, 4)], "宽光标占两格");
        assert!(block_columns(&one_run(3, 4), Some((0, 3, true))).is_empty(), "整条被光标接管");
    }
}
