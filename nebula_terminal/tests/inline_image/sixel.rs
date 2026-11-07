use super::*;

fn sixel() -> &'static str {
    "\x1bP9;1q\"1;1;30;24#1;2;100;0;0!30~-!30~-!30~-!30~\x1b\\"
}

#[test]
fn sixel_omp_sequence_keeps_reserved_rows_at_every_sync_chunk_boundary() {
    let bytes = format!("\x1b[?2026h\x1b[8;4H\x1b7\x1b[1A{}\x1b8\x1b[?2026l", sixel());
    for split in 0..=bytes.len() {
        let (mut term, mut stream, listener) = setup();
        stream.feed(&mut term, &listener, &bytes.as_bytes()[..split]);
        stream.feed(&mut term, &listener, &bytes.as_bytes()[split..]);
        let runs = snap(&term).image_runs;
        assert_eq!((runs.len(), runs[0].row, runs[0].column), (2, 6, 3), "split {split}");
        assert_eq!(
            (term.grid().cursor.point.line, term.grid().cursor.point.column),
            (Line(7), Column(3))
        );
        assert_eq!(term.grid().history_size(), 0);
    }
}

#[test]
fn sixel_negotiation_responds_without_printing_or_moving_cursor() {
    let (mut term, mut stream, listener) = setup();
    let bytes = b"\x1b[?2;1;0S\x1b[16t\x1b[c";
    for byte in bytes {
        stream.feed(&mut term, &listener, &[*byte]);
    }
    let events = listener.0.lock().unwrap();
    let replies: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::PtyWrite(reply) => Some(reply.clone()),
            Event::TextAreaSizeRequest(reply) => Some(reply(WindowSize {
                num_cols: 20,
                num_lines: 8,
                cell_width: 10,
                cell_height: 20,
            })),
            _ => None,
        })
        .collect();
    assert_eq!(replies, ["\x1b[?2;0;4096;4096S", "\x1b[6;20;10t", "\x1b[?62;4c"]);
    assert_eq!(term.grid().cursor.point, nebula_terminal::index::Point::new(Line(0), Column(0)));
}

#[test]
fn cancelled_or_unrelated_dcs_does_not_create_images_or_inject_queries() {
    for sequence in [
        "\x1bP9;1q~\x18",
        "\x1bP9;1q~\x1a",
        "\x1bP9;1q~\x1b[H",
        "\x1bP$qpayload\x1b\\",
        "\x1b]2;[?2;1;0S\x07",
        "\x1bP9;1q!999999999~\x1b\\",
    ] {
        let (mut term, mut stream, listener) = setup();
        stream.feed(&mut term, &listener, format!("{sequence}OK").as_bytes());
        assert!(snap(&term).image_runs.is_empty(), "{sequence:?}");
        assert!(
            !listener.0.lock().unwrap().iter().any(|event| matches!(event, Event::PtyWrite(_)))
        );
        assert_eq!(term.grid()[Line(0)][Column(0)].c, 'O');
    }
}

#[test]
fn transparent_sixel_preserves_text_and_erasure_invalidates_queued_pixels() {
    let (mut term, mut stream, listener) = setup();
    stream.feed(&mut term, &listener, format!("HELLO\r{}", sixel()).as_bytes());
    assert_eq!(term.grid()[Line(0)][Column(0)].c, 'H');
    stream.feed(&mut term, &listener, b"\x1b[2J\x1b[3J");
    assert!(snap(&term).image_runs.is_empty());
    assert!(listener.0.lock().unwrap().iter().all(|event| match event {
        Event::InlineImage { placement, .. } => !placement.is_alive(),
        _ => true,
    }));
}

#[test]
fn sixel_display_mode_clips_at_screen_edges_and_preserves_cursor() {
    let (mut term, mut stream, listener) = setup();
    let large = "\x1bP9;1q\"1;1;300;240#1;2;100;0;0~\x1b\\";
    stream.feed(&mut term, &listener, format!("\x1b[8;20H\x1b[?80h{large}").as_bytes());
    let runs = snap(&term).image_runs;
    assert_eq!(runs.len(), 8);
    assert!(runs.iter().all(|run| run.column == 0 && run.columns == 20));
    assert_eq!(term.grid().cursor.point, nebula_terminal::index::Point::new(Line(7), Column(19)));
    assert_eq!(term.grid().history_size(), 0);
}

#[test]
fn sixel_default_and_cursor_right_modes_have_distinct_exit_positions() {
    for (mode, start, expected) in [
        ("", "\x1b[1;4H", (1, 3)),
        ("\x1b[?8452h", "\x1b[1;4H", (1, 6)),
        ("\x1b[?8452h", "\x1b[1;19H", (2, 0)),
    ] {
        let (mut term, mut stream, listener) = setup();
        stream.feed(&mut term, &listener, format!("{mode}{start}{}", sixel()).as_bytes());
        assert_eq!(
            term.grid().cursor.point,
            nebula_terminal::index::Point::new(Line(expected.0), Column(expected.1))
        );
        let runs = snap(&term).image_runs;
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].columns, if expected == (2, 0) { 2 } else { 3 });
    }
}

#[test]
fn sixel_modes_report_reset_and_remain_independent_of_text_modes() {
    let (mut term, mut stream, listener) = setup();
    stream.feed(
        &mut term,
        &listener,
        b"\x1b[?80$p\x1b[?8452h\x1b[?8452$p\x1b[?1070l\x1b[?1070$p\x1bc\x1b[?8452$p",
    );
    let events = listener.0.lock().unwrap();
    let replies: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::PtyWrite(reply) => Some(reply.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(replies, ["\x1b[?80;2$y", "\x1b[?8452;1$y", "\x1b[?1070;3$y", "\x1b[?8452;2$y"]);
}

#[test]
fn single_row_sixel_does_not_scroll_until_following_linefeed() {
    let (mut term, mut stream, listener) = setup();
    stream.feed(&mut term, &listener, b"\x1b[8;1H\x1bP9;1q\"1;1;10;6!10~\x1b\\");
    assert_eq!(term.grid().history_size(), 0);
    assert_eq!(snap(&term).image_runs[0].row, 7);
    stream.feed(&mut term, &listener, b"\r\nNEXT");
    assert_eq!(term.grid().history_size(), 1);
    assert_eq!(snap(&term).image_runs[0].row, 6);
    assert_eq!(term.grid()[Line(7)][Column(0)].c, 'N');
}

#[test]
fn opaque_sixel_captures_background_override_at_synchronized_replay() {
    use nebula_terminal::inline_image::ImageData;
    use nebula_terminal::vte::ansi::{Color, Rgb};
    let (mut term, mut stream, listener) = setup();
    let sequence = b"\x1b[?2026h\x1b[41m\x1b]4;1;rgb:0a/14/1e\x07\x1bP9;0q\"1;1;2;6~\x1b\\\x1b]4;1;rgb:ff/ff/ff\x07\x1b[?2026l";
    stream.feed(&mut term, &listener, sequence);
    let events = listener.0.lock().unwrap();
    let image = events
        .iter()
        .find_map(|event| match event {
            Event::InlineImage { data: ImageData::Sixel(image), .. } => Some(image),
            _ => None,
        })
        .unwrap();
    assert_eq!(image.background_color(), Some(Color::Spec(Rgb { r: 10, g: 20, b: 30 })));
}

#[test]
fn clean_grid_sixel_is_visible_and_pending_wrap_never_overwrites_text() {
    let (mut term, mut stream, listener) = setup();
    stream.feed(&mut term, &listener, sixel().as_bytes());
    assert_eq!(snap(&term).image_runs.len(), 2);
    stream.feed(&mut term, &listener, b"\x1bc12345678901234567890");
    stream.feed(&mut term, &listener, sixel().as_bytes());
    assert_eq!(term.grid()[Line(0)][Column(19)].c, '0');
    assert!(term.grid().cursor.input_needs_wrap);
    assert!(snap(&term).image_runs.is_empty());
}

#[test]
fn mixed_protocols_replay_in_order_and_share_one_cell_size_reply_at_every_split() {
    let wire = format!(
        "\x1b[?2026h\x1b[2;3H{}\x1b[4;5H\x1b_Ga=T,i=7,s=1,v=1,C=1,q=2;AQIDBA==\x1b\\\x1b[6;7H{}\x1b[16t\x1b[?2;1S",
        image("width=2;height=1"),
        sixel(),
    );
    for split in 0..=wire.len() {
        let (mut term, mut stream, listener) = setup();
        stream.feed(&mut term, &listener, &wire.as_bytes()[..split]);
        stream.feed(&mut term, &listener, &wire.as_bytes()[split..]);
        assert!(snap(&term).image_runs.is_empty(), "split={split}");
        stream.feed(&mut term, &listener, b"\x1b[?2026l");
        let runs = snap(&term).image_runs;
        let cells: Vec<_> = runs.iter().map(|run| (run.row, run.column, run.columns)).collect();
        assert_eq!(cells, [(1, 2, 2), (3, 4, 1), (5, 6, 3), (6, 6, 3)], "split={split}");
        assert!(runs[0].placement.is_none());
        assert!(runs[1].placement.is_some());
        assert!(runs[2].placement.is_none());
        let events = listener.0.lock().unwrap();
        let mut replies: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                Event::PtyWrite(reply) => Some(reply.as_str()),
                _ => None,
            })
            .collect();
        replies.sort_unstable();
        assert_eq!(replies, ["\x1b[6;20;10t", "\x1b[?2;0;4096;4096S"]);
        assert_eq!(
            events.iter().filter(|event| matches!(event, Event::InlineImage { .. })).count(),
            2
        );
        assert_eq!(
            events.iter().filter(|event| matches!(event, Event::KittyImage { .. })).count(),
            1
        );
    }
}

#[test]
fn reset_and_stream_drop_release_shared_encoded_quota_without_losing_kitty() {
    let (mut term, mut stream, listener) = setup();
    stream.feed(&mut term, &listener, b"\x1b[?2026h");
    for i in 0..16 {
        let wire = if i % 2 == 0 { sixel().to_owned() } else { image("width=1;height=1") };
        stream.feed(&mut term, &listener, wire.as_bytes());
    }
    stream.feed(&mut term, &listener, sixel().as_bytes());
    stream.feed(&mut term, &listener, b"\x1b_Ga=T,i=7,s=1,v=1,C=1,q=2;AQIDBA==\x1b\\");
    stream.stop_sync(&mut term);
    {
        let events = listener.0.lock().unwrap();
        assert_eq!(
            events.iter().filter(|event| matches!(event, Event::InlineImage { .. })).count(),
            16
        );
        assert_eq!(
            events.iter().filter(|event| matches!(event, Event::KittyImage { .. })).count(),
            1
        );
    }
    stream.feed(&mut term, &listener, b"\x1b[?2026h");
    for _ in 0..16 {
        stream.feed(&mut term, &listener, sixel().as_bytes());
    }
    drop(stream);
    let mut stream = StreamProcessor::default();
    stream.feed(
        &mut term,
        &listener,
        format!("\x1bc\x1b[?2026h{}\x1bc{}\x1b[?2026l", sixel(), sixel()).as_bytes(),
    );
    assert_eq!(snap(&term).image_runs.len(), 2);
    assert!(snap(&term).image_runs.iter().all(|run| run.placement.is_none()));
}

#[test]
fn sixel_below_scroll_region_clips_without_overwriting_the_last_source_row() {
    let (mut term, mut stream, listener) = setup();
    stream.feed(
        &mut term,
        &listener,
        b"\x1b[1;4r\x1b[8;1H\x1bP9;1q\"1;1;10;60#1;2;100;0;0!10~\x1b\\",
    );
    let runs = snap(&term).image_runs;
    assert_eq!(runs.len(), 1);
    assert_eq!((runs[0].row, runs[0].source_row), (7, 0));
    assert_eq!(term.grid().history_size(), 0);
}
