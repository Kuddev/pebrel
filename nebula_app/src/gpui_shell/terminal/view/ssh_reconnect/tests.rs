use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::Receiver;

use gpui::{Entity, EntityInputHandler as _, Modifiers, TestAppContext, VisualTestContext};
use gpui_component::Root;

use super::*;

fn open(cx: &mut TestAppContext) -> (Entity<TerminalView>, VisualTestContext, Receiver<Msg>) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::terminal::init(cx);
        crate::gpui_shell::math_view::register(cx);
        cx.set_global(Settings::load(nebula_settings::ThemeName::Nord));
    });
    let mut result = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let terminal = cx.new(|cx| {
            TerminalView::new(
                42,
                (80, 24),
                super::super::TerminalLaunch::Local {
                    cwd: None,
                    shell: Some(nebula_terminal::tty::Shell::new(
                        "pebrel-test-missing-shell-executable".into(),
                        Vec::new(),
                    )),
                    shell_name: None,
                },
                window,
                cx,
            )
        });
        let receiver = terminal.update(cx, |view, cx| {
            let receiver = view.install_remote_files_test_session("fixture@example.invalid", cx);
            view.spawn_at -= TerminalView::STARTUP_GRID_GRACE;
            view.copy_on_select = false;
            view.ghost_enabled = false;
            receiver
        });
        terminal.read(cx).focus_handle.clone().focus(window, cx);
        result = Some((terminal.clone(), receiver));
        Root::new(terminal, window, cx)
    });
    let mut window = window.clone();
    window.simulate_resize(gpui::size(px(700.0), px(500.0)));
    window.update(|window, _| window.activate_window());
    draw(&mut window);
    let (terminal, receiver) = result.unwrap();
    receiver.try_iter().for_each(drop);
    (terminal, window, receiver)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn fail(view: &Entity<TerminalView>, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        view.apply_ssh_stage(
            SshStage::Failed("The network connection was interrupted.".into()),
            cx,
        );
    });
    draw(cx);
}

fn input(receiver: &Receiver<Msg>) -> Vec<u8> {
    receiver
        .try_iter()
        .filter_map(|message| match message {
            Msg::Input(bytes) => Some(bytes.into_owned()),
            _ => None,
        })
        .flatten()
        .collect()
}

#[gpui::test]
fn reconnect_key_is_local_only_after_disconnect_and_never_repeats(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = open(cx);
    let events = Rc::new(RefCell::new(Vec::new()));
    let _subscription = window.update(|_, cx| {
        let events = events.clone();
        cx.subscribe(&view, move |_, event, _| match event {
            TerminalViewEvent::RetrySsh(destination) => {
                events.borrow_mut().push(destination.clone())
            },
            TerminalViewEvent::UserInput(_) => events.borrow_mut().push("input".into()),
            _ => {},
        })
    });
    window.simulate_keystrokes("r");
    assert_eq!(input(&receiver), b"r");
    assert_eq!(*events.borrow(), ["input"]);
    events.borrow_mut().clear();
    fail(&view, &mut window);
    assert!(window.debug_bounds("ssh-disconnected").is_some());
    window.simulate_keystrokes("ctrl-r alt-r x");
    // IME commits and pasted text are not reconnect shortcuts.
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.replace_text_in_range(None, "r", window, cx);
            view.replace_and_mark_text_in_range(None, "r", None, window, cx);
            view.on_key_down(
                &KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("r").unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                },
                window,
                cx,
            );
            view.replace_text_in_range(None, "r", window, cx);
            assert!(view.marked_text.is_none());
        });
    });
    assert!(events.borrow().is_empty());
    assert!(input(&receiver).is_empty());
    window.simulate_keystrokes("shift-r r r");
    assert_eq!(*events.borrow(), ["fixture@example.invalid"]);
    assert!(input(&receiver).is_empty());
}

#[gpui::test]
fn disconnected_controls_keep_history_layout_and_selection_available(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = open(cx);
    let events = Rc::new(RefCell::new(Vec::new()));
    let _subscription = window.update(|_, cx| {
        let events = events.clone();
        cx.subscribe(&view, move |_, event, _| match event {
            TerminalViewEvent::RetrySsh(_) => events.borrow_mut().push("retry"),
            TerminalViewEvent::RequestClose => events.borrow_mut().push("close"),
            _ => {},
        })
    });
    view.update(&mut window, |view, cx| {
        let history = (0..100).map(|line| format!("retained row {line}\r\n")).collect::<String>();
        super::super::startup_tests::feed(view, history.as_bytes());
        view.session.as_ref().unwrap().term.lock().scroll_display(Scroll::Top);
        cx.notify();
    });
    draw(&mut window);
    let before = view.read_with(&window, |view, _| (view.grid_rows(), view.scroll_state()));
    fail(&view, &mut window);
    assert_eq!(view.read_with(&window, |view, _| (view.grid_rows(), view.scroll_state())), before);
    window.simulate_keystrokes("x");
    assert_eq!(view.read_with(&window, |view, _| view.scroll_state()), before.1);
    assert!(input(&receiver).is_empty());

    let (start, end) = view.read_with(&window, |view, _| {
        (
            view.origin + point(view.cell_width * 0.1, view.line_height * 0.5),
            view.origin + point(view.cell_width * 7.9, view.line_height * 0.5),
        )
    });
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    window.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    window.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    window.simulate_keystrokes("ctrl-shift-c");
    let copied = window.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some("retained"));

    for width in [360.0, 900.0] {
        window.simulate_resize(gpui::size(px(width), px(500.0)));
        draw(&mut window);
        let banner = window.debug_bounds("ssh-disconnected").unwrap();
        for selector in ["ssh-disconnected-retry", "ssh-disconnected-close"] {
            let bounds = window.debug_bounds(selector).unwrap();
            assert!(bounds.size.height >= px(32.0));
            assert!(banner.contains(&bounds.origin) && banner.contains(&bounds.bottom_right()));
        }
    }
    let retry = window.debug_bounds("ssh-disconnected-retry").unwrap().center();
    window.simulate_mouse_move(retry, None, Modifiers::default());
    draw(&mut window);
    window.simulate_mouse_down(retry, MouseButton::Left, Modifiers::default());
    draw(&mut window);
    window.simulate_mouse_up(retry, MouseButton::Left, Modifiers::default());
    draw(&mut window);
    window.simulate_click(retry, Modifiers::default());
    assert_eq!(*events.borrow(), ["retry"], "pending reconnect cannot be requested twice");
    let close = window.debug_bounds("ssh-disconnected-close").unwrap().center();
    window.simulate_click(close, Modifiers::default());
    assert_eq!(*events.borrow(), ["retry", "close"]);
}

#[gpui::test]
fn initial_failure_retains_the_connection_card(cx: &mut TestAppContext) {
    let (view, mut window, _) = open(cx);
    view.update(&mut window, |view, cx| {
        view.ssh_stage = None;
        view.apply_ssh_stage(SshStage::Resolve, cx);
        assert!(view.ssh_connect.is_some());
        assert!(!view.ssh_disconnected());
    });
    fail(&view, &mut window);
    assert!(view.read_with(&window, |view, _| view.ssh_connect.as_ref().unwrap().failed()));
    assert!(window.debug_bounds("ssh-disconnected").is_none());
    window.simulate_keystrokes("r");
    assert!(!view.read_with(&window, |view, _| view.ssh_retry_pending));
}
