use super::*;
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use gpui_component::Root;
use nebula_terminal::event_loop::StreamProcessor;
use std::sync::mpsc::Receiver;

mod output_hover;

fn open(
    cx: &mut TestAppContext,
) -> (Entity<TerminalView>, VisualTestContext, Receiver<Msg>, session::EventProxy) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::terminal::init(cx);
        crate::gpui_shell::math_view::register(cx);
        cx.set_global(Settings::load(nebula_settings::ThemeName::Nord));
    });
    let mut result = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            TerminalView::new(
                42,
                (80, 24),
                TerminalLaunch::Local {
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
        let (receiver, proxy) = view.update(cx, |view, _| {
            let (session, receiver, _, proxy) = session::test_session_with_events();
            view.session = Some(session);
            view.error = None;
            view.exited = None;
            view.copy_on_select = false;
            view.ghost_enabled = false;
            // 布局仍走真实路径；跳过启动宽限期避免定时器掩盖 resize 断言。
            view.spawn_at = std::time::Instant::now() - std::time::Duration::from_secs(1);
            (receiver, proxy)
        });
        let focus_handle = view.read(cx).focus_handle.clone();
        window.focus(&focus_handle, cx);
        result = Some((view.clone(), receiver, proxy));
        Root::new(view, window, cx)
    });
    let mut window = window.clone();
    window.simulate_resize(gpui::size(px(900.0), px(600.0)));
    window.update(|window, _| window.activate_window());
    draw(&mut window);
    let (view, receiver, proxy) = result.unwrap();
    // Flush the real initial geometry before measuring overlay-only changes.
    view.update(&mut window, |view, _| view.pending_resize = None);
    receiver.try_iter().for_each(drop);
    (view, window, receiver, proxy)
}
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}
fn feed(
    view: &Entity<TerminalView>,
    cx: &mut VisualTestContext,
    proxy: &session::EventProxy,
    bytes: &[u8],
) {
    view.update(cx, |view, cx| {
        StreamProcessor::default().feed(
            &mut *view.session.as_ref().unwrap().term.lock(),
            proxy,
            bytes,
        );
        view.observe_reading_output(cx);
        cx.notify();
    });
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
fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
}
fn execution(text: &str) -> String {
    format!("\x1b]133;A\x07$ \x1b]133;B\x07echo test\r\n\x1b]133;C\x07{text}\x1b]133;D;0\x07")
}

#[gpui::test]
fn reading_overlay_stays_inside_a_narrow_pane(cx: &mut TestAppContext) {
    let (view, mut cx, _, _) = open(cx);
    cx.simulate_resize(gpui::size(px(300.0), px(360.0)));
    draw(&mut cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.open_reading(false, window, cx));
    });
    draw(&mut cx);
    let bounds = cx.debug_bounds("terminal-reading-overlay").unwrap();
    assert!(bounds.origin.x >= px(0.0));
    assert!(bounds.origin.x + bounds.size.width <= px(300.0));
}

#[gpui::test]
fn search_tab_traverses_controls_without_sending_terminal_tab(cx: &mut TestAppContext) {
    let (view, mut cx, receiver, _) = open(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.open_reading(false, window, cx));
    });
    draw(&mut cx);
    let input_focus = view.read_with(&cx, |view, cx| {
        view.reading.panel.as_ref().unwrap().input.read(cx).focus_handle(cx)
    });
    assert!(cx.update(|window, _| input_focus.is_focused(window)));
    cx.simulate_keystrokes("tab");
    draw(&mut cx);
    assert!(!cx.update(|window, _| input_focus.is_focused(window)));
    assert!(input(&receiver).is_empty());
    cx.simulate_keystrokes("shift-tab");
    draw(&mut cx);
    assert!(cx.update(|window, _| input_focus.is_focused(window)));
    cx.simulate_keystrokes("escape");
    draw(&mut cx);
    cx.simulate_keystrokes("tab");
    assert_eq!(input(&receiver), b"\t");
}

#[gpui::test]
fn restored_workspace_focus_keeps_search_input_out_of_pty(cx: &mut TestAppContext) {
    let (view, mut cx, receiver, _) = open(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.open_reading(false, window, cx));
        // 沿用工作区 focus_active 的延迟路径，覆盖搜索打开后的 FocusRequested 往返。
        let focus = view.read(cx).focus_handle(cx);
        window.defer(cx, move |window, cx| window.focus(&focus, cx));
    });
    draw(&mut cx);
    let input_focus = view.read_with(&cx, |view, cx| {
        view.reading.panel.as_ref().unwrap().input.read(cx).focus_handle(cx)
    });
    assert!(cx.update(|window, _| input_focus.is_focused(window)));
    cx.simulate_input("find this");
    draw(&mut cx);
    assert_eq!(
        view.read_with(&cx, |view, cx| {
            view.reading.panel.as_ref().unwrap().input.read(cx).value().to_string()
        }),
        "find this"
    );
    assert!(input(&receiver).is_empty());
}

#[gpui::test]
fn search_input_and_overlay_mouse_never_write_pty_or_resize(cx: &mut TestAppContext) {
    let (view, mut cx, receiver, proxy) = open(cx);
    feed(&view, &mut cx, &proxy, b"hello world");
    let size = view.read_with(&cx, |view, _| (view.cols, view.rows));
    cx.update(|window, cx| view.update(cx, |view, cx| view.open_reading(false, window, cx)));
    draw(&mut cx);
    assert!(view.read_with(&cx, |view, _| view.reading.panel.is_some()));
    cx.simulate_input("world");
    cx.simulate_keystrokes("left backspace");
    draw(&mut cx);
    let overlay = cx.debug_bounds("terminal-reading-overlay").unwrap();
    // 紧凑搜索条的中央属于输入框；右键会打开 Linux 菜单并先消费 Esc。
    // 此处验证面板留白的鼠标隔离，明确命中输入框之外的内边距。
    let at = gpui::point(overlay.left() + px(2.0), overlay.center().y);
    assert!(overlay.contains(&at));
    assert!(!cx.debug_bounds("reading-input").unwrap().contains(&at));
    for button in [MouseButton::Middle, MouseButton::Right] {
        cx.simulate_mouse_down(at, button, Modifiers::default());
        cx.simulate_mouse_up(at, button, Modifiers::default());
    }
    draw(&mut cx);
    assert_eq!(view.read_with(&cx, |view, _| (view.cols, view.rows)), size);
    assert!(!receiver.try_iter().any(|message| matches!(message, Msg::Input(_) | Msg::Resize(_))));
    cx.simulate_keystrokes("escape");
    draw(&mut cx);
    assert!(view.read_with(&cx, |view, _| view.reading.panel.is_none()));
    assert!(input(&receiver).is_empty());
    cx.simulate_keystrokes("escape");
    assert_eq!(input(&receiver), b"\x1b");
}

#[gpui::test]
fn ctrl_f_is_raw_for_shell_agent_alt_and_mouse_but_explicit_search_works(cx: &mut TestAppContext) {
    let (view, mut cx, receiver, proxy) = open(cx);
    cx.simulate_keystrokes("ctrl-f");
    assert_eq!(input(&receiver), b"\x06");
    assert!(view.read_with(&cx, |view, _| view.reading.panel.is_none()));
    view.update(&mut cx, |view, _| view.running_program = Some("codex".into()));
    cx.simulate_keystrokes("ctrl-f");
    assert_eq!(input(&receiver), b"\x06");
    assert!(view.read_with(&cx, |view, _| view.reading.panel.is_none()));
    view.update(&mut cx, |view, _| view.running_program = None);
    for modes in [b"\x1b[?1049h".as_slice(), b"\x1b[?1049l\x1b[?1000h"] {
        feed(&view, &mut cx, &proxy, modes);
        cx.simulate_keystrokes("ctrl-f");
        assert_eq!(input(&receiver), b"\x06");
        assert!(view.read_with(&cx, |view, _| view.reading.panel.is_none()));
    }
    cx.update(|window, cx| view.update(cx, |view, cx| view.open_reading(false, window, cx)));
    draw(&mut cx);
    cx.simulate_input("explicit");
    assert!(input(&receiver).is_empty());
}

#[gpui::test]
fn two_command_copies_are_separate_and_invalid_scope_preserves_clipboard(cx: &mut TestAppContext) {
    let (view, mut cx, receiver, proxy) = open(cx);
    feed(
        &view,
        &mut cx,
        &proxy,
        format!("{}{}", execution("first\r\n"), execution("second\r\n")).as_bytes(),
    );
    cx.update(|window, cx| view.update(cx, |view, cx| view.open_reading(true, window, cx)));
    let commands =
        view.read_with(&cx, |view, _| view.reading.panel.as_ref().unwrap().commands.clone());
    assert_eq!(commands.len(), 2);
    for (id, expected) in [(commands[1].id, "first\n"), (commands[0].id, "second\n")] {
        view.update(&mut cx, |view, cx| {
            view.choose_reading_scope(Some(id), cx);
            view.copy_reading_command(cx);
        });
        cx.run_until_parked();
        assert_eq!(clipboard(&mut cx).as_deref(), Some(expected));
    }
    // Reset invalidates the frozen source; failure must not publish a partial payload.
    feed(&view, &mut cx, &proxy, b"\x1bc");
    view.update(&mut cx, |view, cx| view.copy_reading_command(cx));
    cx.run_until_parked();
    assert_eq!(clipboard(&mut cx).as_deref(), Some("second\n"));
    assert!(input(&receiver).is_empty());
}

#[gpui::test]
fn live_hit_guard_rejects_repaint_without_epoch_change(cx: &mut TestAppContext) {
    let (view, mut cx, _, proxy) = open(cx);
    feed(&view, &mut cx, &proxy, b"same");
    let source = view.read_with(&cx, |view, _| view.session.as_ref().unwrap().term.clone());
    let available = source.lock().reading_span();
    let span = ReadingSpan {
        epoch: available.epoch,
        start: available.start,
        end: ReadingPoint { line: available.start.line, column: 4 },
    };
    let guard = worker::validate_hit(&source, span, "same", true).unwrap();
    feed(&view, &mut cx, &proxy, b"\rnew!");
    assert_eq!(source.lock().reading_epoch(), span.epoch);
    assert!(!worker::guard_valid(&source.lock(), span, &guard));
}
