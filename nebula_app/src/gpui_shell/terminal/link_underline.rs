//! Link dashes in physical grid coordinates, independent of glyph width/shaping.

use gpui::{Bounds, Hsla, Pixels, Window, fill, point, px, size};

pub(super) fn paint(window: &mut Window, cell: Bounds<Pixels>, grid_left: Pixels, color: Hsla) {
    let scale = window.scale_factor().max(0.5);
    let y = (cell.bottom().as_f32() * scale).round();
    let thickness = scale.round().max(1.0);
    for (left, right) in
        dash_segments(cell.left().as_f32(), cell.right().as_f32(), grid_left.as_f32(), scale)
    {
        window.paint_quad(fill(
            Bounds::new(
                point(px(left / scale), px((y - thickness) / scale)),
                size(px((right - left) / scale), px(thickness / scale)),
            ),
            color,
        ));
    }
}

fn dash_segments(
    left: f32,
    right: f32,
    anchor: f32,
    scale: f32,
) -> impl Iterator<Item = (f32, f32)> {
    let left = (left * scale).round();
    let right = (right * scale).round();
    let anchor = (anchor * scale).round();
    let dash = (3.0 * scale).round().max(1.0);
    let period = dash + (2.0 * scale).round().max(1.0);
    let mut x = anchor + ((left - anchor) / period).floor() * period;
    std::iter::from_fn(move || {
        while x < right {
            let start = x.max(left);
            let end = (x + dash).min(right);
            x += period;
            if start < end {
                return Some((start, end));
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::config::UiConfig;
    use crate::gpui_shell::terminal::osc_links::link_decorations;
    use nebula_terminal::event::VoidListener;
    use nebula_terminal::term::test::TermSize;
    use nebula_terminal::term::{Config, Term};
    use nebula_terminal::vte::ansi;

    #[test]
    #[ignore = "informational hot-path measurement; no machine-specific timing gate"]
    fn measure_visible_link_decoration_work() {
        for (name, linked, text) in
            [("plain", false, "x"), ("ascii", true, "x"), ("cjk", true, "中")]
        {
            let mut term = Term::new(Config::default(), &TermSize::new(120, 40), VoidListener);
            let mut parser: ansi::Processor = ansi::Processor::new();
            let text = text.repeat(if name == "cjk" { 50 } else { 100 });
            let row = if linked {
                format!("\x1b]8;;file:///fixture\x1b\\{text}\x1b]8;;\x1b\\\r\n")
            } else {
                format!("{text}\r\n")
            };
            parser.advance(&mut term, row.repeat(35).as_bytes());
            let config = UiConfig::default();
            let started = std::time::Instant::now();
            let mut last_cells = 0;
            for _ in 0..200 {
                let cells = link_decorations(&term, &config, 40, 120).dashed;
                last_cells = cells.len();
                let segments: usize = cells
                    .keys()
                    .map(|&(_, col)| {
                        dash_segments(
                            2.3 + f32::from(col) * 8.4,
                            2.3 + f32::from(col + 1) * 8.4,
                            2.3,
                            1.25,
                        )
                        .count()
                    })
                    .sum();
                std::hint::black_box((cells, segments));
            }
            crate::gpui_shell::try_write_stderr(format_args!(
                "{name}: 120x40 grid, {last_cells} linked columns, {:.1} us/pass (capture + dash geometry, no GPU)",
                started.elapsed().as_secs_f64() * 1_000_000.0 / 200.0
            ));
        }
    }

    #[test]
    fn real_osc8_grid_covers_cjk_spacers_and_spaces_with_one_dash_phase() {
        let mut term = Term::new(Config::default(), &TermSize::new(40, 2), VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(
            &mut term,
            "\x1b]8;;file:///tmp/example\x1b\\A开始 菜单Z\x1b]8;;\x1b\\".as_bytes(),
        );
        let cells = link_decorations(&term, &UiConfig::default(), 2, 40).dashed;
        assert_eq!(cells.len(), 11);
        assert!((0..11).all(|col| cells.contains_key(&(0, col))));
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for width in [7.0, 8.4, 9.5, 12.0] {
                let anchor = 2.3;
                let pixels = |left, right| {
                    dash_segments(left, right, anchor, scale)
                        .flat_map(|(left, right)| left as i32..right as i32)
                };
                let actual: BTreeSet<_> = cells
                    .keys()
                    .flat_map(|&(_, col)| {
                        pixels(anchor + f32::from(col) * width, anchor + f32::from(col + 1) * width)
                    })
                    .collect();
                let expected: BTreeSet<_> = pixels(anchor, anchor + 11.0 * width).collect();
                assert_eq!(actual, expected, "scale={scale}, width={width}");
            }
        }
    }

    /// 提示符里的 ssh 标签自带图标与文字，不该再叠"Ctrl+点击打开外部目标"
    /// 那条虚线下划线；同一行里的外部链接照旧有。
    #[test]
    fn prompt_ssh_chip_is_not_painted_as_an_external_link() {
        let mut term = Term::new(Config::default(), &TermSize::new(60, 2), VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(
            &mut term,
            "\x1b]8;;pebrel-ssh://RQ\x1b\\ ssh \x1b]8;;\x1b\\ \x1b]8;;https://example.com\x1b\\site\x1b]8;;\x1b\\"
                .as_bytes(),
        );
        let cells = link_decorations(&term, &UiConfig::default(), 2, 60).dashed;
        let keys: Vec<_> = cells.keys().copied().collect();
        // 只有 https 链接那 4 列有装饰；ssh 标签（列 0..5）没有。
        assert!((0..5).all(|col| !cells.contains_key(&(0, col))), "{keys:?}");
        assert!((6..10).all(|col| cells.contains_key(&(0, col))), "{keys:?}");
    }

    /// ssh 标签里那格图标是宿主与提示符之间的第二份契约：宿主按码位认出它，
    /// 画上 Claude 品牌图。同一行里孤立的同码位字形不属于任何标签，不能误画。
    #[test]
    fn prompt_ssh_chip_exposes_the_icon_cell_for_the_brand_mark() {
        use nebula_terminal::tty::REMOTE_CLAUDE_CHIP_GLYPH;

        let mut term = Term::new(Config::default(), &TermSize::new(60, 2), VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        parser.advance(
            &mut term,
            format!(
                "\x1b]8;;pebrel-ssh://RQ\x1b\\ {REMOTE_CLAUDE_CHIP_GLYPH} ssh \x1b]8;;\x1b\\\
                 {REMOTE_CLAUDE_CHIP_GLYPH}\r\n"
            )
            .as_bytes(),
        );
        let icons = link_decorations(&term, &UiConfig::default(), 2, 60).chip_icons;
        // " <icon> ssh" 从第 0 列起：图标在第 1 列，第 8 列那枚孤立的同码位字形不算。
        assert_eq!(icons, vec![(0, 1)]);
    }
}
