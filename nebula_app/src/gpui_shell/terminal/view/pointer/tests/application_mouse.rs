//! Exercise real GPUI hit testing and the bytes delivered to the session.
use super::*;

const FULLSCREEN: &[u8] = b"\x1b[?1049h\x1b[?1002h\x1b[?1006hhello world";

fn input(receiver: &std::sync::mpsc::Receiver<Msg>) -> Vec<Vec<u8>> {
    receiver
        .try_iter()
        .filter_map(|message| match message {
            Msg::Input(bytes) => Some(bytes.into_owned()),
            _ => None,
        })
        .collect()
}

#[gpui::test]
fn application_drag_releases_outside_the_pane(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = link_fixture(cx, FULLSCREEN);
    let start = cell(&view, &window, 1);
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    draw(&mut window);
    // The input control above the terminal owns this hitbox.
    let outside = point(start.x, px(10.0));
    window.simulate_mouse_move(outside, Some(MouseButton::Left), Modifiers::default());
    window.simulate_mouse_up(outside, MouseButton::Left, Modifiers::default());
    let reports = input(&receiver);
    assert_eq!(reports.first().unwrap(), b"\x1b[<0;2;1M");
    assert_eq!(reports.last().unwrap(), b"\x1b[<0;2;1m");
    assert_eq!(reports.iter().filter(|r| r.ends_with(b"m")).count(), 1);
}

#[gpui::test]
fn application_drag_keeps_ownership_when_shift_changes(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = link_fixture(cx, FULLSCREEN);
    let start = cell(&view, &window, 1);
    let end = cell(&view, &window, 6);
    let shift = Modifiers { shift: true, ..Modifiers::default() };
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    window.simulate_mouse_move(end, Some(MouseButton::Left), shift);
    window.simulate_mouse_up(end, MouseButton::Left, shift);
    assert_eq!(
        input(&receiver),
        vec![b"\x1b[<0;2;1M".to_vec(), b"\x1b[<36;7;1M".to_vec(), b"\x1b[<4;7;1m".to_vec(),]
    );
    view.read_with(&window, |view, _| assert!(view.selection_is_empty()));
}

#[gpui::test]
fn foreign_drag_and_release_do_not_reach_application(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = link_fixture(cx, FULLSCREEN);
    let end = cell(&view, &window, 6);
    window.simulate_mouse_down(point(end.x, px(10.0)), MouseButton::Left, Modifiers::default());
    window.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    window.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    assert!(input(&receiver).is_empty(), "an application must first receive a press");
}

#[gpui::test]
fn focus_loss_releases_application_once(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = link_fixture(cx, FULLSCREEN);
    let start = cell(&view, &window, 1);
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    window.deactivate_window();
    window.run_until_parked();
    assert_eq!(input(&receiver), vec![b"\x1b[<0;2;1M".to_vec(), b"\x1b[<0;2;1m".to_vec()]);
    window.simulate_mouse_up(start, MouseButton::Left, Modifiers::default());
    assert!(input(&receiver).is_empty(), "a late release must not be sent twice");
}

#[gpui::test]
fn native_shift_selection_does_not_turn_into_application_drag(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = link_fixture(cx, FULLSCREEN);
    let start = cell(&view, &window, 0);
    let end = cell(&view, &window, 5);
    window.simulate_mouse_down(
        start,
        MouseButton::Left,
        Modifiers { shift: true, ..Modifiers::default() },
    );
    window.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    window.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    assert!(input(&receiver).is_empty());
    view.read_with(&window, |view, _| assert!(!view.selection_is_empty()));
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    window.simulate_mouse_up(start, MouseButton::Left, Modifiers::default());
    view.read_with(&window, |view, _| {
        assert!(view.selection_is_empty(), "the TUI owns the new selection")
    });
    assert_eq!(input(&receiver).len(), 2);
}

#[gpui::test]
fn application_mode_round_trip_expires_the_old_gesture(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = link_fixture(cx, FULLSCREEN);
    let start = cell(&view, &window, 1);
    for transition in [
        b"\x1b[?1002l\x1b[?1002h".as_slice(),
        b"\x1b[?1006l\x1b[?1006h",
        b"\x1b[?1049l\x1b[?1049h",
        b"\x1bc\x1b[?1002h\x1b[?1006h",
    ] {
        window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        assert_eq!(input(&receiver).len(), 1);
        view.update(&mut window, |view, _| {
            super::super::super::startup_tests::feed(view, transition)
        });
        window.simulate_mouse_move(
            cell(&view, &window, 4),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        window.simulate_mouse_up(start, MouseButton::Left, Modifiers::default());
        assert!(input(&receiver).is_empty(), "old gesture survived {transition:?}");
    }
}

#[gpui::test]
fn changed_session_does_not_receive_old_release(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = link_fixture(cx, FULLSCREEN);
    let start = cell(&view, &window, 1);
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    assert_eq!(input(&receiver).len(), 1);
    let replacement = view.update(&mut window, |view, _| {
        let receiver = view.install_completion_test_session();
        super::super::super::startup_tests::feed(view, FULLSCREEN);
        receiver
    });
    window.simulate_mouse_up(start, MouseButton::Left, Modifiers::default());
    assert!(input(&replacement).is_empty());
}

#[gpui::test]
fn resize_and_hidden_pane_release_before_changing_coordinates(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = link_fixture(cx, FULLSCREEN);
    let start = cell(&view, &window, 1);
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    window.simulate_resize(gpui::size(px(500.0), px(400.0)));
    draw(&mut window);
    assert_eq!(input(&receiver), vec![b"\x1b[<0;2;1M".to_vec(), b"\x1b[<0;2;1m".to_vec()]);
    window.simulate_mouse_up(start, MouseButton::Left, Modifiers::default());
    assert!(input(&receiver).is_empty());
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    view.update(&mut window, |view, cx| view.set_output_visible(false, cx));
    assert_eq!(input(&receiver).len(), 2);
    window.simulate_mouse_up(start, MouseButton::Left, Modifiers::default());
    assert!(input(&receiver).is_empty());
}

#[gpui::test]
fn legacy_mouse_encoding_and_click_only_tracking_are_preserved(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = link_fixture(cx, b"\x1b[?1049h\x1b[?1000hhello");
    let start = cell(&view, &window, 1);
    let end = cell(&view, &window, 6);
    for encoding in [b"\x1b[?1005l".as_slice(), b"\x1b[?1005h"] {
        view.update(&mut window, |view, _| {
            super::super::super::startup_tests::feed(view, encoding)
        });
        window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        window.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        window.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        assert_eq!(
            input(&receiver),
            vec![b"\x1b[M\x20\x22\x21".to_vec(), b"\x1b[M\x23\x27\x21".to_vec()]
        );
    }
}

struct Pair(Vec<Entity<TerminalView>>);

#[gpui::test]
fn application_redraw_can_reassert_tracking_without_losing_release(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = link_fixture(cx, FULLSCREEN);
    let start = cell(&view, &window, 1);
    let end = cell(&view, &window, 6);
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    view.update(&mut window, |view, _| {
        super::super::super::startup_tests::feed(
            view,
            b"\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h",
        )
    });
    window.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    window.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    assert_eq!(
        input(&receiver),
        vec![b"\x1b[<0;2;1M".to_vec(), b"\x1b[<32;7;1M".to_vec(), b"\x1b[<0;7;1m".to_vec(),]
    );
}

impl Render for Pair {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().children(
            self.0.iter().map(|view| div().flex_1().min_w_0().h_full().child(view.clone())),
        )
    }
}

#[gpui::test]
fn application_drag_stays_with_the_original_pane(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::scientific_render::init(cx);
        cx.set_global(Settings::load(nebula_settings::ThemeName::Nord));
    });
    let mut panes = Vec::new();
    let mut receivers = Vec::new();
    let (_, window) = cx.add_window_view(|window, cx| {
        for id in 1..=2 {
            let view = cx.new(|cx| {
                TerminalView::new(
                    id,
                    (40, 24),
                    super::super::super::TerminalLaunch::Local {
                        cwd: None,
                        shell: Some(nebula_terminal::tty::Shell::new(
                            "pebrel-test-missing-shell-executable".into(),
                            vec![],
                        )),
                        shell_name: None,
                    },
                    window,
                    cx,
                )
            });
            receivers.push(view.update(cx, |view, _| {
                let receiver = view.install_completion_test_session();
                super::super::super::startup_tests::feed(view, FULLSCREEN);
                receiver
            }));
            panes.push(view);
        }
        let pair = cx.new(|_| Pair(panes.clone()));
        Root::new(pair, window, cx)
    });
    let mut window = window.clone();
    window.simulate_resize(gpui::size(px(800.0), px(400.0)));
    window.update(|window, _| window.activate_window());
    draw(&mut window);
    for (source, target) in [(0, 1), (1, 0)] {
        let start = cell(&panes[source], &window, 2);
        let end = cell(&panes[target], &window, 5);
        let last_column =
            panes[source].read_with(&window, |view, _| if source == 0 { view.cols } else { 1 });
        window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        draw(&mut window);
        window.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        window.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        assert!(
            input(&receivers[target]).is_empty(),
            "the neighbouring app received a foreign gesture"
        );
        assert_eq!(
            input(&receivers[source]),
            vec![
                b"\x1b[<0;3;1M".to_vec(),
                format!("\x1b[<32;{last_column};1M").into_bytes(),
                format!("\x1b[<0;{last_column};1m").into_bytes(),
            ]
        );
    }
}

#[gpui::test]
fn missing_os_release_and_keyboard_focus_change_end_drag_once(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = link_fixture(cx, FULLSCREEN);
    let start = cell(&view, &window, 1);
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    window.simulate_mouse_move(cell(&view, &window, 5), None, Modifiers::default());
    assert_eq!(input(&receiver), vec![b"\x1b[<0;2;1M".to_vec(), b"\x1b[<0;2;1m".to_vec()]);
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    window.update(|window, cx| {
        // Opening another focus target while still holding the mouse ends the
        // terminal gesture without waiting for a later release in that control.
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
    });
    window.run_until_parked();
    assert_eq!(input(&receiver).len(), 2);
    window.simulate_mouse_up(start, MouseButton::Left, Modifiers::default());
    assert!(input(&receiver).is_empty());
}
