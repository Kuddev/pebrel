use super::*;
use nebula_terminal::render::{BgRun, BoxGlyph, CursorSnapshot, SnapCell, TextSegment};
use nebula_terminal::term::cell::Flags;
use nebula_terminal::vte::ansi::{Color, CursorShape, NamedColor};

fn snapshot(lines: &[String], rows: u16) -> RenderSnapshot {
    let mut snap = RenderSnapshot {
        cols: 64,
        rows,
        display_offset: 0,
        color_overrides: Default::default(),
        bg_runs: Vec::new(),
        selection_runs: Vec::new(),
        segments: Vec::new(),
        box_glyphs: Vec::new(),
        image_runs: Vec::new(),
        cursor: None,
    };
    for (row, line) in lines.iter().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            if ch == ' ' {
                continue;
            }
            if nebula_terminal::render::boxdraw::is_builtin(ch) {
                snap.box_glyphs.push(BoxGlyph {
                    row: row as u16,
                    col: col as u16,
                    wide: false,
                    ch,
                    fg: Color::Named(NamedColor::Foreground),
                    bold: false,
                    dim: false,
                });
            } else {
                snap.segments.push(TextSegment {
                    row: row as u16,
                    start_col: col as u16,
                    wide: false,
                    cells: vec![SnapCell {
                        col: col as u16,
                        text: ch.to_string(),
                        fg: Color::Named(NamedColor::Foreground),
                        bg: Color::Named(NamedColor::Background),
                        bold: false,
                        dim: false,
                        bg_dim: false,
                        italic: false,
                        underline: false,
                        strikethrough: false,
                    }],
                });
            }
        }
    }
    snap
}

fn logo(ch: char) -> Vec<String> {
    let mut lines = vec![String::new()];
    lines.extend((0..8).map(|_| format!("  {}", ch.to_string().repeat(12))));
    lines.push("following text".into());
    lines
}

fn update(layout: &mut Layout, snap: &RenderSnapshot, top: i64) {
    layout.update(snap, (1, 0, false, snap.cols), top, 8.0, 22.0, false, |row| row, |_, _| true);
}

#[test]
fn character_art_fits_a_region_without_moving_body_or_following_rows() {
    for ch in ['⣿', '█', '/'] {
        let snap = snapshot(&logo(ch), 16);
        let mut layout = Layout::default();
        update(&mut layout, &snap, 0);
        assert_eq!(layout.frame.regions.len(), 1);
        let (first, h) = layout.frame.cell(1, 2, 1.0);
        let (last, _) = layout.frame.cell(8, 13, 8.0);
        assert!((h * 22.0 - 16.0).abs() < 0.001);
        assert!((last + h - first - 8.0 * h).abs() < 0.001);
        assert_eq!(layout.frame.cell(9, 2, 9.0), (9.0, 1.0));
        assert_eq!(layout.frame.cell(4, 30, 4.0), (4.0, 1.0));
        for row in 1..9 {
            let (y, h) = layout.frame.cell(row, 2, row as f32);
            assert!(
                (layout.frame.mapped_source_row(2.5, y + h * 0.5).unwrap() - row as f32 - 0.5)
                    .abs()
                    < 0.001
            );
        }
    }
}

#[test]
fn character_art_selection_spans_match_the_same_inverse_geometry() {
    let mut layout = Layout::default();
    update(&mut layout, &snapshot(&logo('⣿'), 16), 0);
    let mut spans = Vec::new();
    layout.frame.spans(3, 0..20, 3.0, |cols, y, h| spans.push((cols, y, h)));
    assert_eq!(spans.len(), 3);
    assert_eq!(spans[0], (0..2, 3.0, 1.0));
    assert_eq!(spans[1].0, 2..14);
    assert_eq!(spans[2], (14..20, 3.0, 1.0));
    assert!((spans[1].2 * 22.0 - 16.0).abs() < 0.001);
}

#[test]
fn character_art_excludes_prose_tables_powerline_small_decoration_and_mixed_content() {
    for lines in [
        vec!["hello ⣿⣿⣿⣿ world".into()],
        logo('─'),
        logo(''),
        logo('⠁'),
        vec!["/path/to/file".into(); 8],
        {
            let mut rows = logo('⣿');
            rows[4] = "  ⣿⣿⣿word⣿⣿⣿⣿⣿".into();
            rows
        },
    ] {
        let mut layout = Layout::default();
        update(&mut layout, &snapshot(&lines, 16), 0);
        assert!(layout.frame.regions.is_empty());
    }
}

#[test]
fn character_art_respects_cursor_background_and_other_projection_ownership() {
    let mut snap = snapshot(&logo('⣿'), 16);
    snap.cursor = Some(CursorSnapshot {
        row: 4,
        col: 5,
        shape: CursorShape::Hidden,
        wide: false,
        cell_ch: ' ',
        cell_flags: Flags::empty(),
        cell_bg: Color::Named(NamedColor::Background),
    });
    let mut layout = Layout::default();
    update(&mut layout, &snap, 0);
    assert!(layout.frame.regions.is_empty());
    snap.cursor = None;
    snap.bg_runs.push(BgRun {
        row: 3,
        start: 5,
        end: 6,
        color: Color::Named(NamedColor::Red),
        dim: false,
    });
    update(&mut layout, &snap, 0);
    assert!(layout.frame.regions.is_empty());
    snap.bg_runs.clear();
    layout.update(&snap, (1, 0, false, 64), 0, 8.0, 22.0, false, |r| r, |r, _| r != 3);
    assert!(layout.frame.regions.is_empty());
    layout.update(&snap, (1, 0, false, 64), 0, 8.0, 22.0, true, |r| r, |_, _| true);
    assert!(layout.frame.regions.is_empty());
}

#[test]
fn character_art_preserves_absolute_bounds_during_partial_scroll_and_clears_on_reset() {
    let rows = logo('⣿');
    let mut layout = Layout::default();
    update(&mut layout, &snapshot(&rows, 16), 0);
    let (before, h) = layout.frame.cell(4, 2, 4.0);
    assert_eq!(
        layout.capture_rows((1, 0, false, 64), 3, 16, 0..32, &snapshot(&rows[3..], 16)),
        Some(0..19)
    );
    layout.update(
        &snapshot(&rows, 19),
        (1, 0, false, 64),
        0,
        8.0,
        22.0,
        false,
        |r| r - 3.0,
        |_, _| true,
    );
    layout.frame.shift_source(-3);
    let (after, new_h) = layout.frame.cell(1, 2, 1.0);
    assert_eq!(h, new_h);
    assert!((before - 3.0 - after).abs() < 0.001);
    let partial = snapshot(&rows[3..], 16);
    layout.update(&partial, (1, 1, false, 64), 3, 8.0, 22.0, false, |r| r, |_, _| true);
    assert!(layout.frame.regions.is_empty(), "a newly seen partial region has no complete bounds");
}

#[test]
fn character_art_preserves_visible_pixels_from_rows_above_the_source_viewport() {
    let rows = logo('⣿');
    let mut layout = Layout::default();
    update(&mut layout, &snapshot(&rows, 16), 0);
    assert_eq!(
        layout.capture_rows((1, 0, false, 64), 2, 16, 0..32, &snapshot(&rows[2..], 16)),
        Some(0..18)
    );
    let source = snapshot(&rows, 18);
    layout.update(&source, (1, 0, false, 64), 0, 8.0, 22.0, false, |r| r - 2.0, |_, _| true);
    layout.frame.shift_source(-2);
    let (y, h) = layout.frame.cell_at(-1, 2, -1.0);
    assert!(y > 0.0 && y + h < 1.0);
    assert!(source.box_glyphs.iter().any(|g| g.row == 1 && g.col == 2));
    assert!((layout.frame.mapped_source_row(2.5, y + h * 0.5).unwrap() + 0.5).abs() < 0.001);
}

#[test]
fn character_art_erased_edges_do_not_depend_on_a_previous_frame() {
    let mut rows = logo('⣿');
    let mut reused = Layout::default();
    update(&mut reused, &snapshot(&rows, 16), 0);
    for row in &mut rows[5..9] {
        *row = String::new();
    }
    update(&mut reused, &snapshot(&rows, 16), 0);
    let mut fresh = Layout::default();
    update(&mut fresh, &snapshot(&rows, 16), 0);
    assert_eq!(reused.frame.cell(1, 2, 1.0), fresh.frame.cell(1, 2, 1.0));
    assert_eq!(reused.frame.source_bounds(), 1..5);
}

#[test]
fn character_art_probes_unseen_boundaries_instead_of_rejecting_a_top_edge_logo() {
    let rows = logo('⣿');
    let layout = Layout::default();
    let visible = snapshot(&rows[1..], 16);
    assert_eq!(layout.capture_rows((1, 0, false, 64), 12, 16, 0..64, &visible), Some(0..28));
    let text = snapshot(&["ordinary /path/to/file".into()], 16);
    assert_eq!(layout.capture_rows((1, 0, false, 64), 12, 16, 0..64, &text), None);
}

#[test]
fn character_art_slash_panels_keep_their_blank_cross_and_neighboring_text() {
    let mut lines = vec![String::new()];
    for row in 0..9 {
        lines.push(if row == 4 {
            "                              CPU: test".into()
        } else {
            "  //////////  //////////      OS: example".into()
        });
    }
    lines.push("following / text".into());
    let mut layout = Layout::default();
    update(&mut layout, &snapshot(&lines, 16), 0);
    assert_eq!(layout.frame.regions.len(), 1);
    assert!(layout.frame.cell(2, 2, 2.0).1 < 1.0);
    assert_eq!(layout.frame.cell(2, 30, 2.0), (2.0, 1.0));
}
