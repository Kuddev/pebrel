use super::*;
use crate::event::VoidListener;
use crate::index::{Column, Line};
use crate::render::{RenderSnapshot, SnapshotConfig};
use crate::term::{Config, test::TermSize};
use crate::vte::ansi::{Handler as _, Processor};
use unicode_properties::UnicodeEmoji as _;
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

const EMOJI: &[&str] = &["👨‍👩‍👧", "🏳️‍🌈", "❤️‍🔥", "🐦‍⬛", "🙂‍↔️", "👍🏽", "🇨🇳"];

fn terminal(columns: usize) -> Term<VoidListener> {
    Term::new(Config::default(), &TermSize::new(columns, 4), VoidListener)
}

fn feed(term: &mut Term<VoidListener>, text: &str, chunk: usize) {
    let mut parser = Processor::<crate::vte::ansi::StdSyncHandler>::default();
    for bytes in text.as_bytes().chunks(chunk) {
        parser.advance(term, bytes);
    }
}

fn cell_text(cell: &Cell) -> String {
    let mut text = cell.c.to_string();
    text.extend(cell.zerowidth().into_iter().flatten());
    text
}

#[test]
fn reported_emoji_are_one_wide_cell_across_utf8_chunks_copy_and_snapshot() {
    for text in EMOJI.iter().copied().chain(["🔥", "😀", "🚀", "🎉", "🐦", "1️⃣"]) {
        assert_eq!(text.width(), 2, "reference width: {text}");
        for chunk in [1, 2, 5, text.len()] {
            let mut term = terminal(80);
            feed(&mut term, text, chunk);
            assert_eq!(
                term.grid.cursor.point,
                Point::new(Line(0), Column(2)),
                "{text}, chunk {chunk}"
            );
            assert_eq!(cell_text(&term.grid[Line(0)][Column(0)]), text);
            assert!(term.grid[Line(0)][Column(0)].flags.contains(Flags::WIDE_CHAR));
            assert!(term.grid[Line(0)][Column(1)].flags.contains(Flags::WIDE_CHAR_SPACER));
            assert_eq!(
                term.bounds_to_string(
                    Point::new(Line(0), Column(1)),
                    Point::new(Line(0), Column(1))
                ),
                text
            );
            let snap = RenderSnapshot::capture(&term, &SnapshotConfig { rows: 4, cols: 80 });
            assert_eq!(snap.segments.len(), 1);
            assert!(snap.segments[0].wide);
            assert_eq!(snap.segments[0].cells.len(), 1);
            assert_eq!(snap.segments[0].cells[0].text, text);
        }
    }
}

#[test]
fn ordinary_ascii_ends_emoji_clusters_while_keycaps_still_compose() {
    for c in ' '..='~' {
        let mut term = terminal(80);
        feed(&mut term, "👨‍👩", 1);
        term.input(c);
        assert!(term.input_cluster.is_none());
        assert_eq!(cell_text(&term.grid[Line(0)][Column(0)]), "👨‍👩");
        assert_eq!(term.grid[Line(0)][Column(2)].c, c);
    }
    for c in ['#', '*', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9'] {
        let mut term = terminal(80);
        let keycap = format!("{c}\u{fe0f}\u{20e3}");
        feed(&mut term, &keycap, 1);
        assert_eq!(cell_text(&term.grid[Line(0)][Column(0)]), keycap);
        assert_eq!(term.grid.cursor.point.column, Column(2));
    }
}

#[test]
fn ordinary_input_preserves_insert_wrap_and_mapped_characters() {
    let mut term = terminal(4);
    feed(&mut term, "ABCDX", 1);
    assert_eq!(term.grid[Line(1)][Column(0)].c, 'X');
    assert!(term.grid[Line(0)][Column(3)].flags.contains(Flags::WRAPLINE));
    feed(&mut term, "\x1b[1;2H\x1b[4hZ", 1);
    let row: String = (0..4).map(|column| term.grid[Line(0)][Column(column)].c).collect();
    assert_eq!(row, "AZBC");

    for c in ' '..='~' {
        if c.is_emoji_char() {
            continue;
        }
        let mut term = terminal(4);
        feed(&mut term, "\x1b(0", 1);
        term.input(c);
        assert!(!term.grid[Line(0)][Column(0)].c.is_emoji_char());
        assert_eq!(term.grid.cursor.point.column, Column(1));
    }
}

#[test]
fn emoji_continuity_survives_sgr_sync_and_noop_resize_but_not_cursor_or_grid_edits() {
    let mut term = terminal(80);
    feed(&mut term, "👨\x1b[31m\x1b[?2026h‍👩\x1b[?2026l‍👧", 1);
    assert_eq!(cell_text(&term.grid[Line(0)][Column(0)]), EMOJI[0]);
    assert_eq!(
        term.grid[Line(0)][Column(0)].fg,
        crate::vte::ansi::Color::Named(crate::vte::ansi::NamedColor::Foreground)
    );
    let mut term = terminal(80);
    term.input('👍');
    term.resize(TermSize::new(80, 4));
    term.input('🏽');
    assert_eq!(cell_text(&term.grid[Line(0)][Column(0)]), "👍🏽");

    for control in [
        "\x1b[3G",
        "\x1b[1D\x1b[1C",
        "\x1b[0K",
        "\x1b#8",
        "\x1b[1L",
        "\x1b[1M",
        "\x1b[1I\x1b[1Z",
        "\x1b[1A",
        "\x1b[1B",
        "\x1b[1P",
        "\x1b[1@",
        "\x1b[1X",
        "\x1b[1S",
        "\x1b[1T",
        "\x08",
        "\r",
        "\n",
        "\t",
        "\x1bM",
        "\x1b7\x1b8",
        "\x1b[2J",
        "\x1bc",
        "\x1b[?1049h\x1b[?1049l",
        "\x1b[4h\x1b[4l",
        "\x1b[2;4r",
    ] {
        let mut term = terminal(80);
        feed(&mut term, "👍", 1);
        feed(&mut term, control, 1);
        assert!(term.input_cluster.is_none(), "{control:?} must end consecutive input");
        assert!(term.input_end.is_none(), "{control:?} must invalidate the pending head");
        feed(&mut term, "🏽", 1);
        assert!(
            !term.grid[Line(0)][Column(0)].zerowidth().is_some_and(|tail| tail.contains(&'🏽'))
        );
    }
    let mut term = terminal(80);
    term.input('👍');
    term.resize(TermSize::new(79, 4));
    assert!(term.input_cluster.is_none());
    term.input('👍');
    term.swap_alt();
    assert!(term.input_cluster.is_none());
    let mut term = terminal(80);
    term.input('👍');
    let _ = term.grid_mut();
    assert!(term.input_end.is_none(), "raw grid access invalidates the pending head");
}

#[test]
fn selectors_expand_at_last_column_and_keep_wrapped_copy_and_reflow_intact() {
    let mut term = terminal(4);
    feed(&mut term, "abc❤️Z", 1);
    assert!(
        term.grid[Line(0)][Column(3)]
            .flags
            .contains(Flags::LEADING_WIDE_CHAR_SPACER | Flags::WRAPLINE)
    );
    assert_eq!(cell_text(&term.grid[Line(1)][Column(0)]), "❤️");
    assert_eq!(
        term.bounds_to_string(Point::new(Line(0), Column(0)), Point::new(Line(1), Column(2))),
        "abc❤️Z"
    );
    assert_eq!(term.line_to_string(Line(0), Column(3)..Column(3), true), "❤️");
    term.resize(TermSize::new(8, 4));
    assert_eq!(
        term.bounds_to_string(Point::new(Line(0), Column(0)), Point::new(Line(0), Column(5))),
        "abc❤️Z"
    );

    let mut term = terminal(80);
    feed(&mut term, "♈\u{fe0e}X", 1);
    assert_eq!(term.grid.cursor.point.column, Column(2));
    assert!(!term.grid[Line(0)][Column(0)].flags.contains(Flags::WIDE_CHAR));
    assert_eq!(term.grid[Line(0)][Column(1)].c, 'X');
}

#[test]
fn nonemoji_scripts_keep_existing_char_width_and_long_marks_use_incremental_context() {
    for text in ["क्ष", "가", "á", "நி", "क्‍ष्‍क"] {
        let mut term = terminal(80);
        feed(&mut term, text, 1);
        let width: usize = text.chars().map(|c| c.width().unwrap_or(0)).sum();
        assert_eq!(term.grid.cursor.point.column, Column(width), "{text}");
        assert_eq!(
            term.bounds_to_string(
                Point::new(Line(0), Column(0)),
                Point::new(Line(0), Column(width - 1))
            ),
            text
        );
    }
    let mut term = terminal(80);
    let mut text = String::from("👨");
    text.extend(std::iter::repeat_n('́', 20_000));
    text.push_str("‍👩‍👧");
    feed(&mut term, &text, 1);
    assert_eq!(cell_text(&term.grid[Line(0)][Column(0)]), text);
    assert_eq!(term.grid.cursor.point.column, Column(2));
    assert!(term.input_cluster.as_ref().unwrap().context.len() <= text.len());
}

#[test]
fn promoted_emoji_spacer_keeps_first_cell_hyperlink_and_underline_color() {
    let mut term = terminal(80);
    feed(&mut term, "\x1b]8;;https://example.test\x07\x1b[58;2;12;34;56m❤", 1);
    let before = term.grid[Line(0)][Column(0)].clone();
    assert!(before.hyperlink().is_some());
    assert!(before.underline_color().is_some());
    feed(&mut term, "\x1b[58;2;99;0;0m️", 1);
    for column in [0, 1] {
        let cell = &term.grid[Line(0)][Column(column)];
        assert_eq!(cell.hyperlink(), before.hyperlink());
        assert_eq!(cell.underline_color(), before.underline_color());
    }
    assert!(term.grid[Line(0)][Column(1)].zerowidth().unwrap().is_empty());
}

#[test]
fn leading_wide_placeholder_copy_handles_a_removed_following_row() {
    let mut term = terminal(4);
    feed(&mut term, "abc❤️", 1);
    term.goto(0, 0);
    term.insert_blank_lines(3);
    assert_eq!(term.line_to_string(Line(3), Column(3)..Column(3), true), "");
}

#[test]
fn selector_wrap_at_bottom_outside_margin_has_no_nonexistent_row_placeholder() {
    let mut term = terminal(4);
    feed(&mut term, "\x1b[1;3r\x1b[4;4H❤️", 1);
    assert_eq!(cell_text(&term.grid[Line(3)][Column(0)]), "❤️");
    assert!(
        !term.grid[Line(3)][Column(3)]
            .flags
            .intersects(Flags::LEADING_WIDE_CHAR_SPACER | Flags::WRAPLINE)
    );
    let _ = term.line_to_string(Line(3), Column(3)..Column(3), true);
}

#[test]
fn wrapped_wide_input_and_selector_promotion_preserve_insert_mode_target_text() {
    for text in ["❤️", "🙂", "光"] {
        let mut term = terminal(4);
        feed(&mut term, "\x1b[2;1HXYZ\x1b[1;4H\x1b[4h", 1);
        feed(&mut term, text, 1);
        assert_eq!(cell_text(&term.grid[Line(1)][Column(0)]), text);
        assert_eq!(term.grid[Line(1)][Column(2)].c, 'X');
        assert_eq!(term.grid[Line(1)][Column(3)].c, 'Y');
    }
}

#[test]
fn locked_unicode_data_has_no_initial_positive_bmp_emoji_extension() {
    let bases: Vec<char> = (0..=0x10ffff)
        .filter_map(char::from_u32)
        .filter(|c| c.is_emoji_char() && c.width().is_some_and(|width| width > 0))
        .collect();
    for next in bases.iter().copied().filter(|c| *c <= '\u{ffff}') {
        for first in &bases {
            let mut buffer = [0; 8];
            let first_len = first.encode_utf8(&mut buffer).len();
            let next_len = next.encode_utf8(&mut buffer[first_len..]).len();
            let pair = std::str::from_utf8(&buffer[..first_len + next_len]).unwrap();
            assert_eq!(pair.graphemes(true).count(), 2, "initial pair {pair:?}");
        }
    }
}
