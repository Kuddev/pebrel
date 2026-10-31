use super::layout::{Layout, Screen};
use nebula_terminal::Term;
use nebula_terminal::event::VoidListener;
use nebula_terminal::grid::Scroll;
use nebula_terminal::index::{Column, Line, Point, Side};
use nebula_terminal::term::Config;
use nebula_terminal::term::cell::Flags;
use nebula_terminal::term::test::TermSize;
use nebula_terminal::vte::ansi::{Processor, StdSyncHandler};

fn feed(term: &mut Term<VoidListener>, text: &str) {
    Processor::<StdSyncHandler>::default().advance(term, text.as_bytes());
}

fn fixture(columns: usize, prefix: &str, text: &str) -> Term<VoidListener> {
    let mut term = Term::new(Config::default(), &TermSize::new(columns, 4), VoidListener);
    term.nebula_add_prompt_mark();
    feed(&mut term, prefix);
    term.nebula_mark_prompt_input();
    feed(&mut term, text);
    term
}

fn at(line: i32, column: usize) -> Point {
    Point::new(Line(line), Column(column))
}

fn read(term: &Term<VoidListener>, text: &str, cursor: usize) -> Option<Layout> {
    Layout::read(&Screen::capture(term)?, text, cursor)
}

#[test]
fn shell_pointer_maps_native_unicode_indices_at_whole_grapheme_boundaries() {
    let text = "a中e\u{301}👩‍💻z";
    let term = fixture(40, "$ ", text);
    let layout = read(&term, text, text.len()).unwrap();
    for (column, side, offset) in [
        (2, Side::Left, 0),
        (2, Side::Right, 1),
        (3, Side::Right, 1),
        (4, Side::Left, 2),
        (5, Side::Right, 4),
        (6, Side::Left, 4),
        (7, Side::Right, 7),
        (8, Side::Right, 8),
        (9, Side::Left, 8),
    ] {
        assert_eq!(layout.offset(at(0, column), side), Some(offset));
    }
    assert_eq!(layout.offset(at(0, 1), Side::Right), None);
    assert_eq!(layout.offset(at(0, 10), Side::Left), None);
    // The native caret may not bisect a grapheme even at a valid UTF-8 boundary.
    assert!(read(&term, text, "a中e".len()).is_none());
}

#[test]
fn shell_pointer_requires_native_cursor_and_entire_input_to_match_the_grid() {
    let text = "a中ef";
    let mut term = fixture(40, "$ ", text);
    feed(&mut term, "\x1b[1;6H");
    assert!(read(&term, text, "a中".len()).is_some());
    assert!(read(&term, text, text.len()).is_none());
    assert!(read(&term, "a中eg", "a中".len()).is_none());
    term.grid_mut()[at(0, 2)].flags.insert(Flags::HIDDEN);
    assert!(read(&term, text, "a中".len()).is_none());
}

#[test]
fn shell_pointer_maps_verified_soft_wraps_and_wide_character_padding() {
    let text = "abc中de";
    let term = fixture(6, "$ ", text);
    let layout = read(&term, text, text.len()).unwrap();
    assert_eq!(layout.offset(at(0, 4), Side::Right), Some(3));
    assert_eq!(layout.offset(at(0, 5), Side::Left), None);
    assert_eq!(layout.offset(at(1, 0), Side::Left), Some(3));
    assert_eq!(layout.offset(at(1, 1), Side::Right), Some(4));
    assert_eq!(layout.offset(at(1, 4), Side::Left), Some(6));

    let term = fixture(5, "$ ", "abcdef");
    let layout = read(&term, "abcdef", 6).unwrap();
    assert_eq!(layout.offset(at(0, 4), Side::Right), Some(3));
    assert_eq!(layout.offset(at(1, 0), Side::Left), Some(3));
}

#[test]
fn shell_pointer_rejects_explicit_line_breaks_without_a_verified_soft_wrap() {
    let mut term = fixture(5, "$ ", "abc");
    feed(&mut term, "\r\ndef");
    assert!(read(&term, "abcdef", 6).is_none());
}

#[test]
fn shell_pointer_excludes_native_prediction_text_from_the_editable_range() {
    let mut term = fixture(40, "$ ", "abc");
    feed(&mut term, "\x1b[2m prediction\x1b[0m\x1b[1;6H");
    let layout = read(&term, "abc", 3).unwrap();
    assert_eq!(layout.offset(at(0, 5), Side::Left), Some(3));
    assert_eq!(layout.offset(at(0, 7), Side::Left), None);
}

#[test]
fn shell_pointer_rejects_missing_boundaries_hidden_prompts_and_unsafe_input() {
    let mut term = fixture(40, "$ ", "abc");
    for text in ["ab\nc", "ab\tc", "ab\rc", "ab\x7fc"] {
        assert!(read(&term, text, text.len()).is_none());
    }
    assert!(read(&term, &"a".repeat(4097), 0).is_none());
    assert!(read(&term, "中文", 1).is_none());
    term.nebula_end_prompt();
    assert!(read(&term, "abc", 3).is_none());

    let mut term = fixture(5, "$ ", "abc");
    feed(&mut term, "\r\n1\r\n2\r\n3\r\n4");
    assert!(read(&term, "abc", 3).is_none());
    term.scroll_display(Scroll::Top);
    assert!(read(&term, "abc", 3).is_none());
}

#[test]
fn shell_pointer_handles_empty_input_and_pending_wrap_without_guessing_cells() {
    let term = fixture(5, "12345", "");
    let layout = read(&term, "", 0).unwrap();
    assert_eq!(layout.offset(at(0, 5), Side::Left), Some(0));
    assert_eq!(layout.offset(at(0, 4), Side::Right), None);

    let mut term = fixture(5, "$ ", "abc");
    assert!(read(&term, "abc", 3).is_some());
    feed(&mut term, "d\x1b[1;3H");
    assert!(read(&term, "abcd", 0).is_some());
}

#[test]
fn shell_pointer_query_redraw_cannot_change_the_clicked_cell_snapshot() {
    let mut term = fixture(40, "$ ", "abc");
    let screen = Screen::capture(&term).unwrap();
    // Bash bind-x clears the input before the OSC response, then redisplays it.
    feed(&mut term, "\r\x1b[K");
    assert!(read(&term, "abc", 3).is_none());
    let layout = Layout::read(&screen, "abc", 3).unwrap();
    assert_eq!(layout.offset(at(0, 3), Side::Right), Some(2));
    assert!(Layout::read(&screen, "abd", 3).is_none());
}

#[test]
fn shell_pointer_capture_rejects_invalid_or_resized_input_boundaries() {
    for point in [at(4, 0), at(0, 41)] {
        let mut term = fixture(40, "$ ", "abc");
        term.grid_mut().cursor.point = point;
        term.nebula_mark_prompt_input();
        assert!(Screen::capture(&term).is_none());
    }
    let mut term = fixture(40, "$ ", "abc");
    term.resize(TermSize::new(20, 4));
    assert!(Screen::capture(&term).is_none());
}

#[cfg(feature = "gpui-test-support")]
#[gpui::test]
fn shell_pointer_expired_reply_does_not_edit_or_exhaust_future_requests(
    cx: &mut gpui::TestAppContext,
) {
    use super::*;
    let (view, window, receiver) = super::super::startup_tests::open(cx);
    view.update(window, |view, cx| {
        let owner = "bash:42:expiry-fixture";
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        view.suggest.completion_shell_report("pebrel_shell", owner);
        view.observe_shell_pointer("pebrel_pointer_ready", owner, cx);
        let screen = || Screen::capture(&fixture(40, "$ ", "abc")).unwrap();
        let mut term = view.session.as_ref().unwrap().term.lock();
        term.nebula_add_prompt_mark();
        term.nebula_mark_prompt_input();
        drop(term);
        let request = |nonce, created| Request {
            nonce,
            epoch: view.prompt_input_epoch,
            created,
            start: Position(at(0, 3), Side::Left),
            end: Position(at(0, 3), Side::Right),
            origin: view.origin,
            dimensions: (view.cols, view.rows),
            delete_requested: true,
            screen: screen(),
        };
        let expired = request(20000, Instant::now() - Duration::from_secs(2));
        let fresh = request(20001, Instant::now());
        let lost = request(20002, Instant::now() - Duration::from_secs(2));
        view.shell_pointer.pending = Some(lost);
        assert!(view.delete_shell_pointer_selection("backspace", cx));
        assert!(view.shell_pointer.pending.is_none());
        assert!(
            !view.delete_shell_pointer_selection("backspace", cx),
            "a lost reply cannot swallow subsequent keys forever"
        );
        view.shell_pointer.pending = Some(expired);
        view.observe_shell_pointer("pebrel_pointer", &format!("{owner}\nutf8:20000\n3\nabc"), cx);
        assert!(view.shell_pointer.pending.is_none());
        assert!(!receiver.try_iter().any(|message| matches!(message, Msg::Input(_))));
        view.shell_pointer.pending = Some(fresh);
        view.observe_shell_pointer("pebrel_pointer", &format!("{owner}\nutf8:20000\n3\nabc"), cx);
        assert_eq!(view.shell_pointer.pending.as_ref().unwrap().nonce, 20001);
        view.observe_shell_pointer("pebrel_pointer", &format!("{owner}\nutf8:20001\n3\nabc"), cx);
        let actual: Vec<u8> = receiver
            .try_iter()
            .filter_map(|message| match message {
                Msg::Input(bytes) => Some(bytes.into_owned()),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(actual, [command(3), command(4099), command(8197)].concat());
    });
}
