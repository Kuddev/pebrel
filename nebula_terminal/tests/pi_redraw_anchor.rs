use nebula_terminal::Term;
use nebula_terminal::event::VoidListener;
use nebula_terminal::event_loop::StreamProcessor;
use nebula_terminal::grid::{Dimensions, Scroll};
use nebula_terminal::index::{Column, Line};
use nebula_terminal::term::{Config, test::TermSize};

type Terminal = Term<VoidListener>;

fn feed(stream: &mut StreamProcessor, term: &mut Terminal, bytes: &[u8]) {
    stream.feed(term, &VoidListener, bytes);
}

fn numbered(count: usize) -> String {
    (0..count).map(|n| format!("ROW_{n:03}")).collect::<Vec<_>>().join("\r\n")
}

fn frame(text: &str) -> String {
    format!("\x1b[?2026h\x1b[2J\x1b[H\x1b[3J{text}\x1b[?2026l")
}

fn row(term: &Terminal, line: i32) -> String {
    (0..term.columns())
        .map(|col| term.grid()[Line(line)][Column(col)].c)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

fn top(term: &Terminal) -> String {
    row(term, -(term.grid().display_offset() as i32))
}

fn initial(enabled: bool) -> (Terminal, StreamProcessor) {
    let mut term = Term::new(Config::default(), &TermSize::new(60, 5), VoidListener);
    let mut stream = StreamProcessor::default();
    term.set_redraw_anchor_enabled(enabled);
    feed(&mut stream, &mut term, format!("SECRET_OLD\r\n{}", numbered(10)).as_bytes());
    term.scroll_display(Scroll::Top);
    term.scroll_display(Scroll::Delta(-3));
    assert_eq!(top(&term), "ROW_002");
    (term, stream)
}

#[test]
fn complete_sync_redraw_reanchors_new_content_without_resurrecting_history() {
    let (mut term, mut stream) = initial(true);
    feed(&mut stream, &mut term, frame(&numbered(12)).as_bytes());
    assert_eq!(top(&term), "ROW_002");
    assert_eq!(term.history_size(), 7);
    let rows: Vec<_> = (-7..5).map(|line| row(&term, line)).collect();
    assert_eq!(rows, numbered(12).split("\r\n").collect::<Vec<_>>());
}

#[test]
fn disabled_policy_preserves_standard_clear_behavior() {
    let (mut term, mut stream) = initial(false);
    feed(&mut stream, &mut term, frame(&numbered(12)).as_bytes());
    assert_eq!(term.grid().display_offset(), 0);
    assert_eq!(top(&term), "ROW_007");
}

#[test]
fn byte_boundaries_do_not_change_reanchoring() {
    let bytes = frame(&numbered(12)).into_bytes();
    for split in 0..=bytes.len() {
        let (mut term, mut stream) = initial(true);
        feed(&mut stream, &mut term, &bytes[..split]);
        feed(&mut stream, &mut term, &bytes[split..]);
        assert_eq!(top(&term), "ROW_002", "split={split}");
    }
}

#[test]
fn plain_clear_and_missing_or_ambiguous_anchor_do_not_reanchor() {
    for bytes in [
        "\x1b[3J".to_owned(),
        frame("new\r\ncontent"),
        frame(&format!("{}\r\n{}", numbered(12), numbered(12))),
        format!("\x1b[2J\x1b[H\x1b[3J{}", numbered(12)),
    ] {
        let (mut term, mut stream) = initial(true);
        feed(&mut stream, &mut term, bytes.as_bytes());
        assert_eq!(term.grid().display_offset(), 0);
    }
}

#[test]
fn user_scroll_or_disable_during_sync_cancels_pending_anchor() {
    for disable in [false, true] {
        let (mut term, mut stream) = initial(true);
        feed(&mut stream, &mut term, b"\x1b[?2026h");
        if disable {
            term.set_redraw_anchor_enabled(false);
        } else {
            term.scroll_display(Scroll::Bottom);
        }
        feed(
            &mut stream,
            &mut term,
            format!("\x1b[2J\x1b[H\x1b[3J{}\x1b[?2026l", numbered(12)).as_bytes(),
        );
        assert_eq!(term.grid().display_offset(), 0);
    }
}

#[test]
fn forced_sync_stop_does_not_restore_a_partial_redraw() {
    let (mut term, mut stream) = initial(true);
    feed(
        &mut stream,
        &mut term,
        format!("\x1b[?2026h\x1b[2J\x1b[H\x1b[3J{}", numbered(12)).as_bytes(),
    );
    stream.stop_sync(&mut term);
    assert_eq!(term.grid().display_offset(), 0);
    feed(&mut stream, &mut term, b"\x1b[?2026l");
    assert_eq!(term.grid().display_offset(), 0);
}

#[test]
fn cancellation_resize_reset_and_alt_screen_do_not_restore_old_anchor() {
    for operation in 0..4 {
        let (mut term, mut stream) = initial(true);
        feed(&mut stream, &mut term, b"\x1b[?2026h");
        match operation {
            0 => term.cancel_redraw_anchor(),
            1 => term.resize(TermSize::new(61, 5)),
            2 => term.swap_alt(),
            _ => (),
        }
        let reset = if operation == 3 { "\x1bc" } else { "" };
        feed(
            &mut stream,
            &mut term,
            format!("{reset}\x1b[2J\x1b[H\x1b[3J{}\x1b[?2026l", numbered(12)).as_bytes(),
        );
        assert_eq!(term.grid().display_offset(), 0, "operation={operation}");
    }
}

#[test]
fn oversized_sync_update_cancels_even_for_single_large_input() {
    let (mut term, mut stream) = initial(true);
    let bytes = format!(
        "\x1b[?2026h{}\x1b[2J\x1b[H\x1b[3J{}\x1b[?2026l",
        "\x00".repeat(2 * 1024 * 1024),
        numbered(12)
    );
    feed(&mut stream, &mut term, bytes.as_bytes());
    assert_eq!(term.grid().display_offset(), 0);
}

#[test]
fn bottom_follow_and_append_remain_unchanged() {
    let (mut term, mut stream) = initial(true);
    feed(&mut stream, &mut term, b"\r\nAPPEND");
    assert_eq!(top(&term), "ROW_002");
    term.scroll_display(Scroll::Bottom);
    feed(&mut stream, &mut term, frame(&numbered(12)).as_bytes());
    assert_eq!(term.grid().display_offset(), 0);
}

#[test]
fn paragraph_spacing_preserves_anchor_including_blank_top_row() {
    for blank_top in [false, true] {
        let (mut term, mut stream) = initial(true);
        let paragraphs =
            (0..12).map(|n| format!("PARAGRAPH_{n:03}")).collect::<Vec<_>>().join("\r\n\r\n");
        feed(&mut stream, &mut term, frame(&paragraphs).as_bytes());
        term.scroll_display(Scroll::Top);
        term.scroll_display(Scroll::Delta(if blank_top { -5 } else { -4 }));
        let before: Vec<_> =
            (0..3).map(|n| row(&term, -(term.grid().display_offset() as i32) + n)).collect();
        feed(
            &mut stream,
            &mut term,
            frame(&format!("{paragraphs}\r\n\r\nNEW_PARAGRAPH")).as_bytes(),
        );
        let after: Vec<_> =
            (0..3).map(|n| row(&term, -(term.grid().display_offset() as i32) + n)).collect();
        assert_eq!(after, before, "blank_top={blank_top}");
    }
}

#[test]
fn table_rules_widen_without_losing_the_reading_row() {
    let table = |padding: usize| {
        (0..12)
            .map(|n| {
                format!(
                    "│ ROW_{n:03}{}│ value {n} │\r\n├{}┼──────────┤",
                    " ".repeat(padding),
                    "─".repeat(9 + padding)
                )
            })
            .collect::<Vec<_>>()
            .join("\r\n")
    };
    let (mut term, mut stream) = initial(true);
    feed(&mut stream, &mut term, frame(&table(1)).as_bytes());
    term.scroll_display(Scroll::Top);
    term.scroll_display(Scroll::Delta(-4));
    assert_eq!(top(&term), "│ ROW_002 │ value 2 │");
    feed(&mut stream, &mut term, frame(&table(6)).as_bytes());
    assert_eq!(top(&term), "│ ROW_002      │ value 2 │");
}

#[test]
fn partial_clear_does_not_trigger_compatibility_relocation() {
    for clear in ["\x1b[2J\x1b[H", "\x1b[3J"] {
        let (mut enabled, mut stream) = initial(true);
        let (mut standard, mut control) = initial(false);
        let bytes = format!("\x1b[?2026h{clear}{}\x1b[?2026l", numbered(12));
        feed(&mut stream, &mut enabled, bytes.as_bytes());
        feed(&mut control, &mut standard, bytes.as_bytes());
        assert_eq!(top(&enabled), top(&standard));
        assert_eq!(enabled.grid().display_offset(), standard.grid().display_offset());
    }
}

#[test]
fn repeated_sync_frames_recapture_the_current_reading_position() {
    let (mut term, mut stream) = initial(true);
    for count in 12..20 {
        feed(&mut stream, &mut term, frame(&numbered(count)).as_bytes());
        assert_eq!(top(&term), "ROW_002");
    }
}

#[test]
fn wide_and_combining_characters_survive_reanchoring() {
    let content = |count| {
        (0..count).map(|n| format!("中文_{n:03} e\u{301}")).collect::<Vec<_>>().join("\r\n")
    };
    let (mut term, mut stream) = initial(true);
    feed(&mut stream, &mut term, frame(&content(12)).as_bytes());
    term.scroll_display(Scroll::Top);
    term.scroll_display(Scroll::Delta(-2));
    let before = top(&term);
    feed(&mut stream, &mut term, frame(&content(15)).as_bytes());
    assert_eq!(top(&term), before);
    assert_eq!(term.grid().display_offset(), 8);
}

#[test]
fn ambiguous_normalized_padding_does_not_choose_a_matching_copy() {
    let (mut term, mut stream) = initial(true);
    let first = (0..12).map(|n| format!("ROW_{n:03} value")).collect::<Vec<_>>().join("\r\n");
    feed(&mut stream, &mut term, frame(&first).as_bytes());
    term.scroll_display(Scroll::Top);
    term.scroll_display(Scroll::Delta(-2));
    let second = first.replace(" value", "    value");
    feed(&mut stream, &mut term, frame(&format!("{first}\r\n{second}")).as_bytes());
    assert_eq!(term.grid().display_offset(), 0);
}

#[test]
fn table_padding_changes_preserve_three_row_anchor() {
    let (mut term, mut stream) = initial(true);
    let table = |padding: usize| {
        (0..12)
            .map(|n| format!("│ ROW_{n:03}{}│ value {n} │", " ".repeat(padding)))
            .collect::<Vec<_>>()
            .join("\r\n")
    };
    feed(&mut stream, &mut term, frame(&table(1)).as_bytes());
    term.scroll_display(Scroll::Top);
    term.scroll_display(Scroll::Delta(-2));
    assert_eq!(top(&term), "│ ROW_002 │ value 2 │");
    feed(&mut stream, &mut term, frame(&table(6)).as_bytes());
    assert_eq!(top(&term), "│ ROW_002      │ value 2 │");
}
