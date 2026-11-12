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
    use crate::gpui_shell::terminal::osc_links::{dashed_cells, dashed_cells_where};
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
                let cells = dashed_cells(&term, &config, 40, 120);
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
        let cells = dashed_cells(&term, &UiConfig::default(), 2, 40);
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

    #[test]
    fn hidden_source_rows_preserve_link_context_without_per_cell_filtering() {
        let mut term = Term::new(Config::default(), &TermSize::new(120, 40), VoidListener);
        let mut parser: ansi::Processor = ansi::Processor::new();
        let mut wire = String::new();
        for row in 0..35 {
            let suffix = if row == 9 { "x".repeat(300) } else { row.to_string() };
            wire.push_str(&format!(
                "\x1b[32m\x1b]8;;file:///fixture/{row}\x1b\\A开始 菜单Z\x1b]8;;\x1b\\\x1b[0m https://example.com/{suffix}\r\n"
            ));
        }
        parser.advance(&mut term, wire.as_bytes());
        let config = UiConfig::default();
        let all = dashed_cells(&term, &config, 40, 120);
        let checks = std::cell::Cell::new(0);
        let visible = dashed_cells_where(&term, &config, 40, 120, |row| {
            checks.set(checks.get() + 1);
            !(10..30).contains(&row)
        });
        assert_eq!(checks.get(), 40, "visibility is a row operation, not a cell operation");
        let expected: BTreeSet<_> =
            all.keys().filter(|(row, _)| !(10..30).contains(row)).copied().collect();
        assert_eq!(visible.keys().copied().collect::<BTreeSet<_>>(), expected);
        assert!(visible.contains_key(&(0, 0)) && visible.contains_key(&(34, 0)));
        assert!(
            visible.contains_key(&(9, 119)),
            "a visible URL fragment keeps its hidden continuation context"
        );
        for (point, value) in &visible {
            let original = &all[point];
            assert_eq!(value.fg, original.fg);
            assert_eq!(value.bg, original.bg);
            assert_eq!(
                (value.bold, value.dim, value.bg_dim),
                (original.bold, original.dim, original.bg_dim)
            );
        }
    }
}
