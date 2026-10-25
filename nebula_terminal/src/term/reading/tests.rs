use super::*;
use crate::event::VoidListener;
use crate::event_loop::StreamProcessor;
use crate::term::Config;
use crate::term::test::TermSize;
use crate::vte::ansi::{ClearMode, Handler};

fn terminal(columns: usize, rows: usize, history: usize) -> Term<VoidListener> {
    Term::new(
        Config { scrolling_history: history, ..Config::default() },
        &TermSize::new(columns, rows),
        VoidListener,
    )
}

fn feed(term: &mut Term<VoidListener>, stream: &mut StreamProcessor, bytes: &[u8]) {
    stream.feed(term, &VoidListener, bytes);
}

fn execution(output: &str) -> String {
    format!("\x1b]133;A\x07$ \x1b]133;B\x07echo test\r\n\x1b]133;C\x07{output}\x1b]133;D;0\x07")
}

fn collect(
    term: &Term<VoidListener>,
    span: ReadingSpan,
    cells: usize,
    bytes: usize,
) -> ReadingChunk {
    let mut result = ReadingChunk { rows: Vec::new(), next: None };
    let mut cursor = None;
    loop {
        let chunk = term.reading_chunk(span, cursor, cells, bytes).unwrap();
        assert!(
            chunk
                .rows
                .iter()
                .map(|row| row.text.len() + usize::from(row.hard_break))
                .sum::<usize>()
                <= bytes
        );
        for row in &chunk.rows {
            for cell in &row.cells {
                assert!(row.text.is_char_boundary(cell.byte_start));
                assert!(row.text.is_char_boundary(cell.byte_end));
            }
        }
        result.rows.extend(chunk.rows);
        if chunk.next.is_none() {
            break;
        }
        assert_ne!(cursor, chunk.next);
        cursor = chunk.next;
    }
    result
}

#[test]
fn stream_markers_preserve_order_at_every_byte_split() {
    let bytes = execution("hello\r\n");
    for split in 0..=bytes.len() {
        let mut term = terminal(20, 4, 100);
        let mut stream = StreamProcessor::default();
        feed(&mut term, &mut stream, &bytes.as_bytes()[..split]);
        feed(&mut term, &mut stream, &bytes.as_bytes()[split..]);
        let command = term.completed_commands().next().unwrap();
        assert_eq!(command.command, "echo test");
        assert_eq!(command.exit_code, Some(0));
        assert_eq!(display_text(&collect(&term, command.output, 3, 5)), "hello", "split={split}");
    }
}

#[test]
fn two_commands_and_zero_output_have_disjoint_exact_ranges() {
    let mut term = terminal(20, 8, 100);
    let mut stream = StreamProcessor::default();
    feed(
        &mut term,
        &mut stream,
        format!("{}{}{}", execution("first\r\n"), execution("second\r\n"), execution(""))
            .as_bytes(),
    );
    let commands: Vec<_> = term.completed_commands().cloned().collect();
    assert_eq!(commands.len(), 3);
    assert!(commands[0].output.end <= commands[1].output.start);
    assert_eq!(display_text(&collect(&term, commands[0].output, 1, 4)), "first");
    assert_eq!(display_text(&collect(&term, commands[1].output, 2, 7)), "second");
    assert_eq!(commands[2].output.start, commands[2].output.end);
    assert!(collect(&term, commands[2].output, 0, 0).rows.is_empty());
}

#[test]
fn all_small_chunk_budgets_preserve_spaces_soft_wrap_and_cjk_mapping() {
    let mut term = terminal(6, 6, 100);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, "ab  中文e\u{301}Z\r\nnext".as_bytes());
    let span = ReadingSpan { end: term.reading_cursor(), ..term.reading_span() };
    for cells in 1..=15 {
        for bytes in 4..=21 {
            let result = collect(&term, span, cells, bytes);
            assert_eq!(
                display_text(&result),
                "ab  中文e\u{301}Z\nnext",
                "cells={cells},bytes={bytes}"
            );
            let mapped: Vec<_> = result
                .rows
                .iter()
                .flat_map(|row| {
                    row.cells
                        .iter()
                        .map(move |cell| (&row.text[cell.byte_start..cell.byte_end], cell.width))
                })
                .collect();
            assert!(mapped.contains(&("中", 2)));
            assert!(mapped.contains(&("文", 2)));
            assert!(mapped.contains(&("e\u{301}", 1)));
        }
    }
}

#[test]
fn incomplete_duplicate_and_out_of_order_markers_never_invent_commands() {
    for order in ["D", "CD", "ABD", "ACD", "ABCCD", "AABCD", "ABBCD", "ABCAD", "ABDC D"] {
        let mut term = terminal(10, 3, 100);
        let mut stream = StreamProcessor::default();
        for mark in order.bytes() {
            feed(&mut term, &mut stream, format!("\x1b]133;{}\x07", mark as char).as_bytes());
        }
        assert_eq!(term.completed_commands().count(), 0, "{order}");
        feed(&mut term, &mut stream, execution("ok").as_bytes());
        assert_eq!(term.completed_commands().count(), 1, "recovery {order}");
    }
}

#[test]
fn reset_resize_saved_clear_and_alt_switch_invalidate_addresses() {
    for action in 0..5 {
        let mut term = terminal(20, 4, 100);
        let mut stream = StreamProcessor::default();
        feed(&mut term, &mut stream, execution("result").as_bytes());
        let span = term.completed_commands().next().unwrap().output;
        match action {
            0 => term.reset_state(),
            1 => term.resize(TermSize::new(25, 4)),
            2 => term.clear_screen(ClearMode::Saved),
            3 => term.swap_alt(),
            _ => {
                let _ = term.grid_mut();
            },
        }
        assert_eq!(term.reading_chunk(span, None, 10, 100), Err(ReadingError::StaleEpoch));
        assert_eq!(term.completed_commands().count(), 0);
    }
}

#[test]
fn alt_screen_is_searchable_but_does_not_record_shell_commands() {
    let mut term = terminal(20, 4, 100);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, b"primary");
    term.swap_alt();
    feed(&mut term, &mut stream, execution("alternate").as_bytes());
    assert_eq!(term.completed_commands().count(), 0);
    let text = display_text(&collect(&term, term.reading_span(), 4, 20));
    assert!(text.contains("alternate"));
    assert!(!text.contains("primary"));
    assert_eq!(term.command_output_span(1), Err(ReadingError::UnsupportedScreen));
}

#[test]
fn scroll_append_preserves_epoch_and_eviction_is_explicit() {
    let mut term = terminal(20, 3, 4);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, execution("one\r\n").as_bytes());
    let command = term.completed_commands().next().unwrap().clone();
    feed(&mut term, &mut stream, b"next\r\nnext\r\n");
    assert_eq!(term.reading_epoch(), command.output.epoch);
    assert_eq!(term.command_output_span(command.id), Ok(command.output));
    feed(&mut term, &mut stream, b"x\r\nx\r\nx\r\nx\r\nx\r\nx\r\n");
    assert_eq!(term.reading_epoch(), command.output.epoch);
    assert_eq!(term.command_output_span(command.id), Err(ReadingError::Evicted));
    assert_eq!(term.reading_chunk(command.output, None, 1, 4), Err(ReadingError::Evicted));
}

#[test]
fn live_tail_overwrite_is_rejected_even_when_dimensions_and_endpoints_match() {
    let mut term = terminal(20, 4, 100);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, execution("safe").as_bytes());
    let command = term.completed_commands().next().unwrap().clone();
    feed(&mut term, &mut stream, b"\rEVIL");
    assert_eq!(term.reading_cursor(), command.output.end);
    assert_eq!(term.command_output_span(command.id), Err(ReadingError::ContentChanged));
    assert_eq!(term.reading_chunk(command.output, None, 1, 4), Err(ReadingError::ContentChanged));
}

#[test]
fn budget_zero_short_utf8_and_extreme_combining_sequence_fail_without_partial_cell() {
    let mut term = terminal(4, 2, 0);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, "中".as_bytes());
    let span = ReadingSpan { end: term.reading_cursor(), ..term.reading_span() };
    for (cells, bytes) in [(0, 10), (10, 0), (10, 2)] {
        assert_eq!(term.reading_chunk(span, None, cells, bytes), Err(ReadingError::BudgetTooSmall));
    }
    let cell = &mut term.grid[Line(0)][Column(0)];
    for _ in 0..40000 {
        cell.push_zerowidth('\u{301}');
    }
    assert_eq!(
        term.reading_chunk(span, None, usize::MAX, usize::MAX),
        Err(ReadingError::BudgetTooSmall)
    );
}

#[test]
fn long_rows_and_all_whitespace_still_yield_bounded_chunks() {
    let mut term = terminal(100000, 2, 0);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, b"text");
    let chunk = term.reading_chunk(term.reading_span(), None, 7, 7).unwrap();
    assert_eq!(chunk.rows.len(), 1);
    assert_eq!(chunk.rows[0].end_column, 7);
    assert_eq!(chunk.next, Some(ReadingPoint { line: 0, column: 7 }));
    let chunk = term.reading_chunk(term.reading_span(), chunk.next, 7, 7).unwrap();
    assert_eq!(chunk.rows[0].text, "       ");
    assert_eq!(chunk.next, Some(ReadingPoint { line: 0, column: 14 }));
}

#[test]
fn read_queries_leave_selection_cursor_modes_and_viewport_untouched() {
    use crate::index::{Point, Side};
    use crate::selection::{Selection, SelectionType};
    let mut term = terminal(20, 4, 100);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, execution("text").as_bytes());
    term.selection =
        Some(Selection::new(SelectionType::Simple, Point::new(Line(0), Column(0)), Side::Left));
    let before = (
        format!("{:?}", term.selection),
        term.grid.cursor.point,
        *term.mode(),
        term.grid.display_offset(),
    );
    collect(&term, term.reading_span(), 2, 4);
    let id = term.completed_commands().next().unwrap().id;
    term.command_output_span(id).unwrap();
    assert_eq!(
        before,
        (
            format!("{:?}", term.selection),
            term.grid.cursor.point,
            *term.mode(),
            term.grid.display_offset()
        )
    );
}

#[test]
fn command_metadata_and_live_tail_have_hard_limits() {
    let mut term = terminal(20, 4, 1000);
    let mut stream = StreamProcessor::default();
    for _ in 0..100 {
        feed(&mut term, &mut stream, execution("x\r\n").as_bytes());
    }
    assert!(term.reading.commands.len() <= COMMANDS);
    assert!(term.reading.metadata <= METADATA_BYTES);
    let mut huge = terminal(100000, 2, 0);
    let mut stream = StreamProcessor::default();
    feed(&mut huge, &mut stream, execution(&"x".repeat(40000)).as_bytes());
    assert_eq!(huge.completed_commands().count(), 0);
}

#[test]
fn live_guard_accepts_normal_200_by_50_window_after_long_output() {
    let mut term = terminal(200, 50, 2000);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, execution(&"ordinary output\r\n".repeat(1000)).as_bytes());
    let command = term.completed_commands().next().expect("normal large window must retain output");
    assert_eq!(term.command_output_span(command.id), Ok(command.output));
    assert_eq!(command.output.end.line - command.output.start.line, 1000);
    assert!(term.reading.metadata < 64 * 1024);
}

#[test]
fn live_guard_bounds_hostile_width_without_rejecting_its_short_output() {
    let mut term = terminal(100000, 2, 100);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, execution("short").as_bytes());
    let command = term.completed_commands().next().unwrap();
    assert_eq!(term.command_output_span(command.id), Ok(command.output));
    assert!(term.reading.metadata < 4096);
    let mut term = terminal(100000, 2, 100);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, execution(&"x".repeat(40000)).as_bytes());
    assert_eq!(term.completed_commands().count(), 0);
}

#[test]
fn guard_detects_live_overwrite_even_after_the_modified_tail_freezes_into_history() {
    let mut term = terminal(20, 4, 100);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, execution("safe\r\n").as_bytes());
    let command = term.completed_commands().next().unwrap().clone();
    feed(&mut term, &mut stream, b"\x1b[2;1HEVIL\x1b[4;1H\r\n\r\n\r\n\r\n");
    assert!(command.output.end.line < term.grid.scrolled_out() + term.history_size());
    assert_eq!(term.command_output_span(command.id), Err(ReadingError::ContentChanged));
}

#[test]
fn guard_partial_checks_do_not_rescan_distant_tail_but_final_validation_rejects_it() {
    let mut term = terminal(200, 50, 2000);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, execution(&"ordinary output\r\n".repeat(1000)).as_bytes());
    let command = term.completed_commands().next().unwrap().clone();
    let changed =
        ReadingPoint { line: term.grid.scrolled_out() + term.history_size() + 48, column: 0 };
    term.grid[Line(48)][Column(0)].c = '!';
    assert!(term.reading_chunk(command.output, None, 256, 4096).is_ok());
    assert_eq!(
        term.reading_chunk(command.output, Some(changed), 256, 4096),
        Err(ReadingError::ContentChanged)
    );
    assert_eq!(term.command_output_span(command.id), Err(ReadingError::ContentChanged));
}

#[test]
fn guard_preserves_zero_width_and_consumed_breaks_but_allows_append_after_full_width_end() {
    let mut term = terminal(6, 5, 100);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, execution("xxxxxx").as_bytes());
    let command = term.completed_commands().next().unwrap().clone();
    feed(&mut term, &mut stream, b"z");
    assert_eq!(term.command_output_span(command.id), Ok(command.output));
    let mut term = terminal(6, 5, 100);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, execution("e\u{301}").as_bytes());
    let command = term.completed_commands().next().unwrap().clone();
    feed(&mut term, &mut stream, "\u{300}".as_bytes());
    assert_eq!(term.command_output_span(command.id), Err(ReadingError::ContentChanged));
    let mut term = terminal(6, 5, 100);
    let mut stream = StreamProcessor::default();
    feed(
        &mut term,
        &mut stream,
        b"\x1b]133;A\x07\x1b]133;B\x07abcdef\x1b]133;C\x07X\x1b]133;D\x07",
    );
    let command = term.completed_commands().next().unwrap().clone();
    assert_eq!(term.command_output_span(command.id), Ok(command.output));
    term.grid[Line(0)][Column(5)].flags.remove(Flags::WRAPLINE);
    assert_eq!(term.command_output_span(command.id), Err(ReadingError::ContentChanged));
    assert_eq!(term.reading_chunk(command.output, None, 1, 1), Err(ReadingError::ContentChanged));
}

#[test]
fn guard_compact_metadata_stays_under_total_budget_for_many_large_windows() {
    let mut term = terminal(200, 50, 10000);
    let mut stream = StreamProcessor::default();
    for _ in 0..40 {
        feed(&mut term, &mut stream, execution(&"ordinary output\r\n".repeat(100)).as_bytes());
    }
    assert!(term.reading.commands.len() > 20);
    assert!(term.reading.commands.len() < COMMANDS);
    assert!(term.reading.metadata <= METADATA_BYTES);
}

#[test]
fn hundred_thousand_line_output_records_only_its_bounded_live_tail() {
    let mut term = terminal(16, 3, 100010);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, execution(&"x\r\n".repeat(100000)).as_bytes());
    let command = term.completed_commands().next().unwrap();
    assert_eq!(command.output.end.line - command.output.start.line, 100000);
    assert_eq!(term.command_output_span(command.id), Ok(command.output));
    assert!(term.reading.metadata < 8192);
    let chunk = term.reading_chunk(command.output, None, 16, 32).unwrap();
    assert!(chunk.next.is_some());
    assert_eq!(chunk.rows[0].text.trim_end_matches(' '), "x");
}

#[test]
fn synchronized_uncommitted_markers_are_not_assigned_stale_coordinates() {
    let mut term = terminal(20, 4, 100);
    let mut stream = StreamProcessor::default();
    feed(
        &mut term,
        &mut stream,
        format!("\x1b[?2026h{}\x1b[?2026l", execution("hidden")).as_bytes(),
    );
    assert_eq!(term.completed_commands().count(), 0);
}

#[test]
fn pending_wrap_zero_output_and_leading_wide_spacer_are_not_newlines() {
    let mut term = terminal(6, 3, 100);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, b"\x1b]133;A\x07\x1b]133;B\x07abcdef\x1b]133;C\x07\x1b]133;D\x07");
    let output = term.completed_commands().next().unwrap().output;
    assert_eq!(output.start, ReadingPoint { line: 0, column: 6 });
    assert_eq!(output.start, output.end);
    let mut term = terminal(6, 3, 100);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, "abcde中z".as_bytes());
    let span = ReadingSpan { end: term.reading_cursor(), ..term.reading_span() };
    assert_eq!(display_text(&collect(&term, span, 1, 3)), "abcde中z");
}

#[test]
fn exact_byte_budget_defers_hard_break_without_losing_it() {
    let mut term = terminal(2, 2, 0);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, b"ab\r\nz");
    let span = ReadingSpan { end: term.reading_cursor(), ..term.reading_span() };
    let first = term.reading_chunk(span, None, 2, 2).unwrap();
    assert_eq!(first.rows[0].text, "ab");
    assert!(!first.rows[0].hard_break);
    assert_eq!(first.next, Some(ReadingPoint { line: 0, column: 2 }));
    let second = term.reading_chunk(span, first.next, 1, 1).unwrap();
    assert!(second.rows[0].hard_break);
    assert_eq!(second.rows[0].text, "");
    assert_eq!(display_text(&collect(&term, span, 1, 1)), "ab\nz");
}

/// 手动成本样本，不把机器速度变成 CI 门槛；只比较同预算查询的历史/列宽规模。
#[test]
#[ignore]
fn bounded_reading_cost_sample() {
    use std::hint::black_box;
    use std::time::Instant;
    for (columns, count) in [(80, 100), (80, 100000), (100000, 1)] {
        let mut term = terminal(columns, 3, count);
        let mut stream = StreamProcessor::default();
        let input = "sample output\r\n".repeat(count);
        let start = Instant::now();
        feed(&mut term, &mut stream, input.as_bytes());
        let feed_elapsed = start.elapsed();
        let span = term.reading_span();
        let start = Instant::now();
        for _ in 0..2000 {
            black_box(term.reading_chunk(span, None, 256, 4096).unwrap());
        }
        eprintln!(
            "columns={columns} history_target={count} input_bytes={} feed={feed_elapsed:?} read_2000={:?}",
            input.len(),
            start.elapsed()
        );
    }
}

/// 计时覆盖查询调用本身（等价于宿主持锁范围），组装与裁掉行尾空格在计时范围外。
#[test]
#[ignore]
fn compact_guard_and_copy_cost_sample() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    let mut worst = terminal(256, 128, 100);
    let mut stream = StreamProcessor::default();
    let wire = execution(&"x".repeat(guard::MAX_CELLS));
    let mark = wire.rfind("\x1b]133;D").unwrap();
    feed(&mut worst, &mut stream, &wire.as_bytes()[..mark]);
    let start = Instant::now();
    feed(&mut worst, &mut stream, &wire.as_bytes()[mark..]);
    let capture = start.elapsed();
    let command = worst.completed_commands().next().unwrap().clone();
    let mut max_full = Duration::ZERO;
    let start = Instant::now();
    for _ in 0..1000 {
        let call = Instant::now();
        black_box(worst.command_output_span(command.id).unwrap());
        max_full = max_full.max(call.elapsed());
    }
    let full_total = start.elapsed();
    let mut max_partial = Duration::ZERO;
    let start = Instant::now();
    for _ in 0..2000 {
        let call = Instant::now();
        black_box(worst.reading_chunk(command.output, None, 256, 4096).unwrap());
        max_partial = max_partial.max(call.elapsed());
    }
    eprintln!(
        "guard_cells={} metadata={} capture={capture:?} full_check_1000={full_total:?} full_check_max={max_full:?} partial_2000={:?} partial_max={max_partial:?}",
        guard::MAX_CELLS,
        worst.reading.metadata,
        start.elapsed()
    );

    let mut term = terminal(200, 50, 100010);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, execution(&"sample output\r\n".repeat(100000)).as_bytes());
    let command = term.completed_commands().next().unwrap().clone();
    let start = Instant::now();
    term.command_output_span(command.id).unwrap();
    let mut output = String::new();
    let mut pending = String::new();
    let mut cursor = None;
    let mut max_chunk = Duration::ZERO;
    let mut calls = 0;
    let mut query_time = Duration::ZERO;
    loop {
        let call = Instant::now();
        let chunk = term.reading_chunk(command.output, cursor, 4096, 32768).unwrap();
        let elapsed = call.elapsed();
        query_time += elapsed;
        max_chunk = max_chunk.max(elapsed);
        calls += 1;
        for row in chunk.rows {
            pending.push_str(&row.text);
            if row.hard_break {
                output.push_str(pending.trim_end_matches(' '));
                output.push('\n');
                pending.clear();
            }
        }
        cursor = chunk.next;
        if cursor.is_none() {
            break;
        }
    }
    output.push_str(pending.trim_end_matches(' '));
    term.command_output_span(command.id).unwrap();
    let total = start.elapsed();
    assert_eq!(output.lines().count(), 100000);
    eprintln!(
        "copy_lines=100000 columns=200 calls={calls} output_bytes={} total={total:?} query_total={query_time:?} max_query={max_chunk:?} throughput_MiB_s={:.2} metadata={}",
        output.len(),
        output.len() as f64 / (1024.0 * 1024.0) / total.as_secs_f64(),
        term.reading.metadata
    );
    black_box(output);
}

#[test]
fn backwards_output_and_invalid_cursors_are_rejected() {
    let mut term = terminal(20, 4, 100);
    let mut stream = StreamProcessor::default();
    feed(&mut term, &mut stream, execution("\x1b[Hbad").as_bytes());
    assert_eq!(term.completed_commands().count(), 0);
    let span = term.reading_span();
    assert_eq!(
        term.reading_chunk(span, Some(ReadingPoint { line: 0, column: 21 }), 5, 5),
        Err(ReadingError::InvalidCursor)
    );
    assert_eq!(
        term.reading_chunk(ReadingSpan { start: span.end, end: span.start, ..span }, None, 5, 5),
        Err(ReadingError::InvalidRange)
    );
}
