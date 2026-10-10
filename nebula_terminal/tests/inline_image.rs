use std::sync::{Arc, Mutex};

use nebula_terminal::Term;
use nebula_terminal::event::{Event, EventListener, WindowSize};
use nebula_terminal::event_loop::StreamProcessor;
use nebula_terminal::grid::Dimensions;
use nebula_terminal::index::{Column, Line};
use nebula_terminal::inline_image::ImageOptions;
use nebula_terminal::render::{RenderSnapshot, SnapshotConfig};
use nebula_terminal::term::Config;

#[derive(Clone, Default)]
struct Listener(Arc<Mutex<Vec<Event>>>);
impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        self.0.lock().unwrap().push(event);
    }
}

#[derive(Clone, Copy)]
struct Size(usize, usize);
impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.1
    }
    fn screen_lines(&self) -> usize {
        self.1
    }
    fn columns(&self) -> usize {
        self.0
    }
}

fn setup() -> (Term<Listener>, StreamProcessor, Listener) {
    let listener = Listener::default();
    let term = Term::new(Config::default(), &Size(20, 8), listener.clone());
    let mut stream = StreamProcessor::default();
    stream.resize(WindowSize { num_cols: 20, num_lines: 8, cell_width: 10, cell_height: 20 });
    (term, stream, listener)
}

fn image(args: &str) -> String {
    format!(
        "\x1b]1337;File=inline=1;{args}:iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==\x07"
    )
}

fn snap(term: &Term<Listener>) -> RenderSnapshot {
    RenderSnapshot::capture(
        term,
        &SnapshotConfig { rows: term.screen_lines() as u16, cols: term.columns() as u16 },
    )
}

#[test]
fn image_geometry_honors_units_and_preserves_explicit_footprint() {
    let viewport = WindowSize { num_cols: 80, num_lines: 24, cell_width: 10, cell_height: 20 };
    let layout =
        ImageOptions::parse(b"width=10;height=auto").unwrap().layout(100, 50, viewport, 0).unwrap();
    assert_eq!((layout.width, layout.height, layout.columns, layout.rows), (100.0, 50.0, 10, 3));
    let layout = ImageOptions::parse(b"width=50%;height=80px")
        .unwrap()
        .layout(100, 50, viewport, 0)
        .unwrap();
    assert_eq!((layout.width, layout.height, layout.columns, layout.rows), (160.0, 80.0, 40, 4));
    let layout = ImageOptions::parse(b"width=50%;height=80px;preserveAspectRatio=0")
        .unwrap()
        .layout(100, 50, viewport, 0)
        .unwrap();
    assert_eq!((layout.width, layout.height), (400.0, 80.0));
    assert!(ImageOptions::parse(b"width=0px").is_none());
    let narrow = WindowSize { num_cols: 20, num_lines: 8, ..viewport };
    let layout = ImageOptions::default().layout(200, 200, narrow, 10).unwrap();
    assert_eq!((layout.width, layout.height, layout.columns, layout.rows), (100.0, 100.0, 10, 5));
    assert!(
        ImageOptions::parse(b"width=1;height=4294967295")
            .unwrap()
            .layout(1, 1, viewport, 0)
            .is_none()
    );
}

#[test]
fn omp_save_move_image_restore_does_not_scroll_reserved_bottom_rows() {
    let (mut term, mut stream, listener) = setup();
    let bytes =
        format!("\x1b[8;4H\x1b7\x1b[1A{}\x1b8", image("width=4;height=2;preserveAspectRatio=0"));
    stream.feed(&mut term, &listener, bytes.as_bytes());
    assert_eq!(term.grid().history_size(), 0);
    assert_eq!(term.grid().cursor.point.line, Line(7));
    assert_eq!(term.grid().cursor.point.column, Column(3));
    let runs = snap(&term).image_runs;
    assert_eq!(runs.len(), 2);
    assert_eq!((runs[0].row, runs[0].column, runs[0].columns), (6, 3, 4));
    assert_eq!((runs[1].row, runs[1].source_row), (7, 1));
    assert!(!listener.0.lock().unwrap().iter().any(|event| matches!(event, Event::Title(_))));
}

#[test]
fn image_from_bottom_scrolls_only_between_image_rows() {
    let (mut term, mut stream, listener) = setup();
    stream.feed(&mut term, &listener, format!("\x1b[8;3H{}", image("width=4;height=2")).as_bytes());
    assert_eq!(term.grid().history_size(), 1);
    assert_eq!(term.grid().cursor.point.line, Line(7));
    assert_eq!(term.grid().cursor.point.column, Column(6));
    assert_eq!(snap(&term).image_runs[0].row, 6);
}

#[test]
fn image_at_every_chunk_boundary_obeys_synchronized_cursor_order() {
    let bytes = format!("\x1b[?2026h\x1b[3;5H{}\x1b[7;9H\x1b[?2026l", image("width=3;height=2"));
    for split in 0..=bytes.len() {
        let (mut term, mut stream, listener) = setup();
        stream.feed(&mut term, &listener, &bytes.as_bytes()[..split]);
        stream.feed(&mut term, &listener, &bytes.as_bytes()[split..]);
        let runs = snap(&term).image_runs;
        assert_eq!((runs[0].row, runs[0].column), (2, 4), "split={split}");
        assert_eq!(term.grid().cursor.point.line, Line(6));
        assert_eq!(term.grid().cursor.point.column, Column(8));
    }
}

#[test]
fn image_sync_timeout_is_atomic_and_post_image_clear_invalidates_decode() {
    let (mut term, mut stream, listener) = setup();
    stream.feed(
        &mut term,
        &listener,
        format!("\x1b[?2026h\x1b[2;3H{}", image("width=4;height=2")).as_bytes(),
    );
    assert!(snap(&term).image_runs.is_empty());
    stream.stop_sync(&mut term);
    assert_eq!(snap(&term).image_runs[0].column, 2);
    stream.feed(&mut term, &listener, b"\x1b[2J\x1b[3J");
    assert!(snap(&term).image_runs.is_empty());
    let events = listener.0.lock().unwrap();
    let placement = events
        .iter()
        .find_map(|event| match event {
            Event::InlineImage { placement, .. } => Some(placement),
            _ => None,
        })
        .unwrap();
    assert!(!placement.is_alive());
}

#[test]
fn alternate_screen_hides_main_images_and_restores_them_on_return() {
    let (mut term, mut stream, listener) = setup();
    stream.feed(&mut term, &listener, image("width=3;height=2").as_bytes());
    let main_id = snap(&term).image_runs[0].id;
    stream.feed(&mut term, &listener, b"\x1b[?1049h");
    assert!(snap(&term).image_runs.is_empty());
    stream.feed(&mut term, &listener, image("width=2;height=1").as_bytes());
    assert_ne!(snap(&term).image_runs[0].id, main_id);
    stream.feed(&mut term, &listener, b"\x1b[?1049l");
    assert_eq!(snap(&term).image_runs[0].id, main_id);
    stream.feed(&mut term, &listener, b"\x1b[?1049h");
    assert!(snap(&term).image_runs.is_empty());
}

#[test]
fn interrupted_sync_stream_releases_pending_image_quota() {
    let (mut term, mut stream, listener) = setup();
    stream.feed(&mut term, &listener, b"\x1b[?2026h");
    for _ in 0..16 {
        stream.feed(&mut term, &listener, image("width=2;height=1").as_bytes());
    }
    assert!(snap(&term).image_runs.is_empty());
    drop(stream);
    let mut stream = StreamProcessor::default();
    stream.feed(&mut term, &listener, format!("\x1bc{}", image("width=2;height=1")).as_bytes());
    assert_eq!(snap(&term).image_runs.len(), 1);
}

#[test]
fn reset_during_sync_preserves_images_queued_after_reset() {
    let (mut term, mut stream, listener) = setup();
    let bytes = format!(
        "\x1b[?2026h{}\x1bc{}\x1b[?2026l",
        image("width=2;height=1"),
        image("width=3;height=1")
    );
    stream.feed(&mut term, &listener, bytes.as_bytes());
    let runs = snap(&term).image_runs;
    assert_eq!((runs.len(), runs[0].columns), (1, 3));
    assert!(!listener.0.lock().unwrap().iter().any(|event| matches!(event, Event::Title(_))));
}

#[test]
fn partial_erasure_preserves_source_coordinates_of_remaining_pixels() {
    let (mut term, mut stream, listener) = setup();
    stream.feed(&mut term, &listener, image("width=4;height=2").as_bytes());
    stream.feed(&mut term, &listener, b"\x1b[1;2H\x1b[X\x1b[2;1HX");
    let runs = snap(&term).image_runs;
    assert_eq!(runs.len(), 3);
    assert_eq!((runs[0].column, runs[0].columns), (0, 1));
    assert_eq!((runs[1].column, runs[1].source_column, runs[1].columns), (2, 2, 2));
    assert_eq!((runs[2].row, runs[2].column, runs[2].source_column), (1, 1, 1));
}

#[test]
fn resize_and_reset_do_not_leave_images_at_old_absolute_rows() {
    let (mut term, mut stream, listener) = setup();
    stream.feed(&mut term, &listener, format!("\x1b[2;3H{}", image("width=4;height=2")).as_bytes());
    let id = snap(&term).image_runs[0].id;
    term.resize(Size(10, 8));
    assert!(snap(&term).image_runs.iter().all(|run| run.id == id && run.column < 10));
    stream.feed(&mut term, &listener, b"\x1bc");
    assert!(snap(&term).image_runs.is_empty());
}

#[test]
fn image_at_pending_right_margin_does_not_overwrite_last_text_cell() {
    let (mut term, mut stream, listener) = setup();
    stream.feed(&mut term, &listener, b"12345678901234567890");
    stream.feed(&mut term, &listener, image("width=2;height=1").as_bytes());
    assert_eq!(term.grid()[Line(0)][Column(19)].c, '0');
    assert!(term.grid().cursor.input_needs_wrap);
    assert!(snap(&term).image_runs.is_empty());
    stream.feed(&mut term, &listener, format!("\r\n{}", image("width=2;height=1")).as_bytes());
    assert_eq!((snap(&term).image_runs[0].row, snap(&term).image_runs[0].column), (1, 0));
}

#[test]
fn image_replay_does_not_change_application_title_stack() {
    let (mut term, mut stream, listener) = setup();
    let sequence = format!(
        "\x1b]2;outer\x07\x1b[22;0t\x1b[?2026h{}\x1b]2;inner\x07\x1b[?2026l\x1b[23;0t",
        image("width=2;height=1")
    );
    stream.feed(&mut term, &listener, sequence.as_bytes());
    let events = listener.0.lock().unwrap();
    let titles: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::Title(title) => Some(title.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(titles, ["outer", "inner", "outer"]);
}
