use std::sync::Arc;

use nebula_terminal::Term;
use nebula_terminal::event::EventListener;
use nebula_terminal::event_loop::StreamProcessor;
use nebula_terminal::grid::{Dimensions, Grid};
use nebula_terminal::index::{Column, Line};
use nebula_terminal::inline_image::{
    ImageCell, ImageIdentity, ImageLayout, ImagePlacement, SourceRect,
};
use nebula_terminal::render::{RenderSnapshot, SnapshotConfig};
use nebula_terminal::term::Config;
use nebula_terminal::term::cell::Cell;
use nebula_terminal::term::reading::ReadingError;

#[derive(Clone, Copy)]
struct Listener;
impl EventListener for Listener {
    fn command_view(
        &self,
        shell: &str,
        command: &str,
    ) -> Option<nebula_terminal::inline_content::CommandView> {
        (shell == "pwsh:1" && command == "cat long.md").then(|| {
            nebula_terminal::inline_content::CommandView {
                mime: "text/markdown".into(),
                title: command.into(),
            }
        })
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

fn setup(history: usize) -> (Term<Listener>, StreamProcessor) {
    (
        Term::new(
            Config { scrolling_history: history, ..Config::default() },
            &Size(20, 8),
            Listener,
        ),
        StreamProcessor::default(),
    )
}

fn insert(term: &mut Term<Listener>, columns: usize, rows: usize) -> ImagePlacement {
    term.nebula_insert_inline_image(ImageLayout {
        width: columns as f32 * 10.0,
        height: rows as f32 * 20.0,
        cell_width: 10.0,
        cell_height: 20.0,
        columns,
        rows,
        source: SourceRect::full(columns as u32 * 10, rows as u32 * 20),
    })
}

fn snap(term: &Term<Listener>) -> RenderSnapshot {
    RenderSnapshot::capture(
        term,
        &SnapshotConfig { rows: term.screen_lines() as u16, cols: term.columns() as u16 },
    )
}

#[test]
fn direct_image_erasure_preserves_fragments_and_snapshot_does_not_own_pixels() {
    let (mut term, mut stream) = setup(1);
    let image = insert(&mut term, 4, 2);
    let before = snap(&term);
    assert!(image.is_alive());
    assert_eq!(before.image_runs.iter().map(|run| run.columns).sum::<u16>(), 8);
    stream.feed(&mut term, &Listener, b"\x1b[1;2H\x1b[X\x1b[2;1HX");
    let runs = snap(&term).image_runs;
    assert_eq!(runs.len(), 3);
    assert_eq!((runs[0].column, runs[0].source_column, runs[0].columns), (0, 0, 1));
    assert_eq!((runs[1].column, runs[1].source_column, runs[1].columns), (2, 2, 2));
    assert_eq!((runs[2].row, runs[2].source_row, runs[2].source_column), (1, 1, 1));
    stream.feed(&mut term, &Listener, b"\x1b[2J\x1b[3J");
    assert!(snap(&term).image_runs.is_empty());
    assert!(!image.is_alive());
    assert_eq!(before.image_runs.len(), 2, "old geometry can outlive its grid owner");
}

#[test]
fn direct_images_follow_screen_swap_and_release_when_alternate_is_reentered() {
    let (mut term, mut stream) = setup(1);
    let main = insert(&mut term, 3, 2);
    stream.feed(&mut term, &Listener, b"\x1b[?1049h");
    assert!(snap(&term).image_runs.is_empty());
    assert!(main.is_alive());
    let alternate = insert(&mut term, 2, 1);
    assert_ne!(snap(&term).image_runs[0].id, main.id);
    stream.feed(&mut term, &Listener, b"\x1b[?1049l");
    assert_eq!(snap(&term).image_runs[0].id, main.id);
    stream.feed(&mut term, &Listener, b"\x1b[?1049h");
    assert!(snap(&term).image_runs.is_empty());
    assert!(!alternate.is_alive());
}

#[test]
fn direct_image_owner_dies_at_history_eviction_and_history_clear() {
    for clear_history in [false, true] {
        let mut term =
            Term::new(Config { scrolling_history: 1, ..Config::default() }, &Size(20, 3), Listener);
        let mut stream = StreamProcessor::default();
        let image = insert(&mut term, 1, 1);
        stream.feed(&mut term, &Listener, b"\r\n\r\n\r\n");
        assert!(snap(&term).image_runs.is_empty());
        assert!(image.is_alive(), "the history row still owns the image");
        if clear_history {
            term.grid_mut().clear_history();
        } else {
            stream.feed(&mut term, &Listener, b"\r\n");
        }
        assert!(!image.is_alive(), "cached row allocations must not retain image owners");
    }
}

#[test]
fn direct_image_reflow_preserves_every_source_tile_and_reset_releases_it() {
    let (mut term, mut stream) = setup(3);
    stream.feed(&mut term, &Listener, b"\x1b[2;3H");
    let image = insert(&mut term, 4, 2);
    for columns in [3, 10] {
        term.resize(Size(columns, 8));
        let runs = snap(&term).image_runs;
        assert!(!runs.is_empty());
        for run in &runs {
            assert_eq!(run.id, image.id);
            assert!(usize::from(run.column + run.columns) <= columns);
        }
        let mut tiles = Vec::new();
        for row in -(term.history_size() as i32)..term.screen_lines() as i32 {
            for cell in &term.grid()[Line(row)][..] {
                if let Some(tile) = cell.image() {
                    assert_eq!(tile.image.id, image.id);
                    tiles.push((tile.row, tile.column));
                }
            }
        }
        tiles.sort_unstable();
        assert_eq!(
            tiles,
            (0..2).flat_map(|row| (0..4).map(move |column| (row, column))).collect::<Vec<_>>()
        );
    }
    stream.feed(&mut term, &Listener, b"\x1bc");
    assert!(!image.is_alive());
    assert!(snap(&term).image_runs.is_empty());
}

#[test]
fn direct_image_wraps_before_insertion_without_overwriting_full_text_line() {
    let (mut term, mut stream) = setup(1);
    stream.feed(&mut term, &Listener, b"12345678901234567890");
    assert!(term.grid().cursor.input_needs_wrap);
    insert(&mut term, 4, 1);
    assert_eq!(term.grid()[Line(0)][Column(19)].c, '0');
    let run = snap(&term).image_runs.remove(0);
    assert_eq!((run.row, run.column, run.columns), (1, 0, 4));
}

#[test]
fn direct_image_breaks_the_previous_text_grapheme_cluster() {
    let (mut term, mut stream) = setup(1);
    stream.feed(&mut term, &Listener, "\u{1f1e8}".as_bytes());
    let image_column = term.grid().cursor.point.column;
    insert(&mut term, 1, 1);
    let next_column = term.grid().cursor.point.column;
    stream.feed(&mut term, &Listener, "\u{1f1f3}".as_bytes());
    assert_eq!(term.grid()[Line(0)][Column(0)].c, '\u{1f1e8}');
    assert!(term.grid()[Line(0)][Column(0)].zerowidth().is_none_or(|chars| chars.is_empty()));
    assert_eq!(term.grid()[Line(0)][next_column].c, '\u{1f1f3}');
    assert_eq!(usize::from(snap(&term).image_runs[0].column), image_column.0);
}

#[test]
fn public_grid_construction_and_clone_keep_image_snapshot_tracking() {
    let (mut term, _) = setup(0);
    for cloned in [false, true] {
        let mut grid: Grid<Cell> = Grid::new(8, 20, 0);
        if cloned {
            grid = grid.clone();
        }
        let image = ImageIdentity::new();
        let lifetime = Arc::downgrade(&image);
        grid[Line(0)][Column(0)].set_image(ImageCell { image, column: 2, row: 3 });
        *term.grid_mut() = grid;
        let run = snap(&term).image_runs.remove(0);
        assert_eq!((run.source_column, run.source_row), (2, 3));
        term.grid_mut().reset_region(..);
        assert!(lifetime.upgrade().is_none());
    }
}

#[cfg(feature = "serde")]
#[test]
fn deserialized_grid_can_receive_images_without_losing_snapshot_tracking() {
    let (mut term, _) = setup(0);
    let mut grid: Grid<Cell> =
        serde_json::from_str(&serde_json::to_string(term.grid()).unwrap()).unwrap();
    grid[Line(0)][Column(0)].set_image(ImageCell {
        image: ImageIdentity::new(),
        column: 0,
        row: 0,
    });
    *term.grid_mut() = grid;
    assert_eq!(snap(&term).image_runs.len(), 1);
}

#[test]
fn image_cursor_template_transfer_enables_tracking_on_fresh_alternate_grid() {
    let (mut term, mut stream) = setup(0);
    term.grid_mut().cursor.template.set_image(ImageCell {
        image: ImageIdentity::new(),
        column: 2,
        row: 3,
    });
    stream.feed(&mut term, &Listener, b"\x1b[?1049hX");
    let run = snap(&term).image_runs.remove(0);
    assert_eq!((run.source_column, run.source_row), (2, 3));
}

const NATIVE_COMMAND: &str = "\x1b]1337;SetUserVar=pebrel_shell=cHdzaDox\x07\x1b]1337;SetUserVar=pebrel_command=cHdzaDoxCmNhdCBsb25nLm1k\x07\x1b]133;C\x07";

#[test]
fn cached_native_owner_dies_when_its_last_logical_row_leaves() {
    let mut term =
        Term::new(Config { scrolling_history: 1, ..Config::default() }, &Size(20, 3), Listener);
    let mut stream = StreamProcessor::default();
    stream.feed(
        &mut term,
        &Listener,
        format!("{NATIVE_COMMAND}# marker\r\n\x1b]133;D;0\x07").as_bytes(),
    );
    let owner = Arc::downgrade(&term.grid()[Line(0)][Column(0)].inline_marker().unwrap().surface);
    for _ in 0..10 {
        stream.feed(&mut term, &Listener, b"plain\r\n");
        let history = (term.grid().total_lines() - term.grid().screen_lines()) as i32;
        let has_marker = (-history..term.grid().screen_lines() as i32)
            .any(|row| term.grid()[Line(row)][Column(0)].inline_marker().is_some());
        if !has_marker {
            assert!(owner.upgrade().is_none(), "cached rows must release the last native owner");
            return;
        }
    }
    panic!("fixture did not evict the last logical marker");
}

#[test]
fn native_source_rejects_image_changes_and_snapshot_obeys_row_filter() {
    let (mut term, mut stream) = setup(1);
    stream.feed(
        &mut term,
        &Listener,
        format!("{NATIVE_COMMAND}# Title\r\n\x1b]133;D;0\x07").as_bytes(),
    );
    let surface = term.grid()[Line(0)][Column(0)].inline_marker().unwrap().surface.clone();
    assert!(surface.source_row_matches(0, &term.grid()[Line(0)][..]));
    term.grid_mut()[Line(0)][Column(10)].set_image(ImageCell {
        image: ImageIdentity::new(),
        column: 0,
        row: 0,
    });
    assert!(!surface.source_row_matches(0, &term.grid()[Line(0)][..]));
    assert_eq!(snap(&term).image_runs.len(), 1);
    let hidden =
        RenderSnapshot::capture_rows(&term, &SnapshotConfig { rows: 8, cols: 20 }, |row| row != 0);
    assert!(hidden.image_runs.is_empty());
}

#[test]
fn external_image_cells_invalidate_reading_and_release_after_eviction() {
    let (mut term, mut stream) = setup(0);
    stream.feed(&mut term, &Listener, b"\n\n\n\n\n\n\n\n");
    let span = term.reading_span();
    let image = ImageIdentity::new();
    let lifetime = Arc::downgrade(&image);
    term.grid_mut()[Line(0)][Column(0)].set_image(ImageCell { image, column: 0, row: 0 });
    assert_eq!(term.reading_chunk(span, None, 1, 1), Err(ReadingError::StaleEpoch));
    assert_eq!(snap(&term).image_runs.len(), 1);
    stream.feed(&mut term, &Listener, b"\n");
    assert!(snap(&term).image_runs.is_empty());
    assert!(lifetime.upgrade().is_none());
}
