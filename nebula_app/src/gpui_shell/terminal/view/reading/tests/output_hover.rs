//! 按真实结果位置hover/点击，不以直接改hover状态代替交互验收。
use super::*;
use crate::gpui_shell::copy_feedback::COPY_FEEDBACK_TTL;
use nebula_terminal::term::reading::CompletedCommand;
use nebula_terminal::vte::ansi::{ClearMode, Handler as _};

#[gpui::test]
fn hover_control_consumes_middle_and_right_without_terminal_gestures(cx: &mut TestAppContext) {
    let (view, mut cx, receiver, _, commands) = fixture(cx);
    view.update(&mut cx, |view, _| view.copy_on_select = true);
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("must-not-paste".into())));
    let bounds = hover(&view, &mut cx, &commands[0]);
    for button in [MouseButton::Middle, MouseButton::Right] {
        cx.simulate_mouse_down(bounds.center(), button, Modifiers::default());
        cx.simulate_mouse_up(bounds.center(), button, Modifiers::default());
    }
    draw(&mut cx);
    assert!(input(&receiver).is_empty());
    assert!(view.read_with(&cx, |view, _| view.reading.panel.is_none()));
    assert_eq!(clipboard(&mut cx).as_deref(), Some("must-not-paste"));
}

#[gpui::test]
fn scrolling_result_out_of_view_revokes_hover_without_epoch_or_registry_loss(
    cx: &mut TestAppContext,
) {
    let (view, mut cx, _, proxy, commands) = fixture(cx);
    feed(&view, &mut cx, &proxy, "more\r\n".repeat(80).as_bytes());
    view.update(&mut cx, |view, cx| {
        view.session.as_ref().unwrap().term.lock().scroll_display(Scroll::Top);
        cx.notify();
    });
    draw(&mut cx);
    hover(&view, &mut cx, &commands[0]);
    view.update(&mut cx, |view, cx| {
        view.session.as_ref().unwrap().term.lock().scroll_display(Scroll::Bottom);
        view.reconcile_output_hover(cx);
    });
    draw(&mut cx);
    assert!(view.read_with(&cx, |view, _| {
        let term = view.session.as_ref().unwrap().term.lock();
        term.reading_epoch() == commands[0].output.epoch
            && term.completed_commands().any(|command| command.id == commands[0].id)
    }));
    assert!(view.read_with(&cx, |view, _| view.reading.output_hover.hovered.is_none()));
}

fn fixture(
    cx: &mut TestAppContext,
) -> (
    Entity<TerminalView>,
    VisualTestContext,
    Receiver<Msg>,
    session::EventProxy,
    Vec<CompletedCommand>,
) {
    let (view, mut cx, receiver, proxy) = open(cx);
    feed(
        &view,
        &mut cx,
        &proxy,
        format!("{}{}", execution("first\r\n"), execution("second\r\n")).as_bytes(),
    );
    draw(&mut cx);
    let commands = view.read_with(&cx, |view, _| {
        view.session.as_ref().unwrap().term.lock().completed_commands().cloned().collect()
    });
    (view, cx, receiver, proxy, commands)
}
fn result_point(
    view: &Entity<TerminalView>,
    cx: &VisualTestContext,
    command: &CompletedCommand,
) -> Point<Pixels> {
    view.read_with(cx, |view, _| {
        let term = view.session.as_ref().unwrap().term.lock();
        let top = term.grid().scrolled_out().saturating_add(term.history_size()) as i64
            + i64::from(term.viewport_origin_for(view.rows).0);
        point(
            view.origin.x + view.cell_width * (command.output.start.column as f32 + 0.5),
            view.origin.y
                + view.line_height * ((command.output.start.line as i64 - top) as f32 + 0.5),
        )
    })
}
fn copy_bounds(cx: &mut VisualTestContext, id: u64) -> Option<Bounds<Pixels>> {
    // 锁定GPUI的debug_bounds要求静态selector；夹具仅查询这四个结果，避免泄漏动态字符串。
    let selector = match id {
        1 => "terminal-output-copy-1",
        2 => "terminal-output-copy-2",
        3 => "terminal-output-copy-3",
        4 => "terminal-output-copy-4",
        _ => panic!("unexpected fixture command id {id}"),
    };
    cx.debug_bounds(selector)
}

fn hover(
    view: &Entity<TerminalView>,
    cx: &mut VisualTestContext,
    command: &CompletedCommand,
) -> Bounds<Pixels> {
    let point = result_point(view, cx, command);
    cx.simulate_mouse_move(point, None, Modifiers::default());
    draw(cx);
    let bounds = copy_bounds(cx, command.id).expect("hovered result copy");
    let line_height = view.read_with(cx, |view, _| view.line_height);
    assert!(
        (bounds.origin.y - (point.y - line_height * 0.5)).as_f32().abs() <= 1.0,
        "copy control must be anchored to this output, not a later layout row: {bounds:?}"
    );
    bounds
}
fn click(cx: &mut VisualTestContext, bounds: Bounds<Pixels>) {
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    cx.simulate_mouse_down(bounds.center(), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(bounds.center(), MouseButton::Left, Modifiers::default());
    draw(cx);
}

#[gpui::test]
fn no_hover_has_no_global_controls_and_each_hover_copies_its_result(cx: &mut TestAppContext) {
    let (view, mut cx, receiver, _, commands) = fixture(cx);
    assert_eq!(commands.len(), 2);
    assert!(cx.debug_bounds("terminal-reading-tools").is_none());
    for command in &commands {
        assert!(copy_bounds(&mut cx, command.id).is_none());
    }
    let size = view.read_with(&cx, |view, _| (view.cols, view.rows));
    receiver.try_iter().for_each(drop);
    for (command, expected) in [(&commands[0], "first\n"), (&commands[1], "second\n")] {
        let bounds = hover(&view, &mut cx, command);
        // 正文→按钮的连续移动不撤销当前目标。
        cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
        draw(&mut cx);
        assert!(copy_bounds(&mut cx, command.id).is_some());
        click(&mut cx, bounds);
        assert_eq!(clipboard(&mut cx).as_deref(), Some(expected));
        assert!(view.read_with(&cx, |view, _| view.reading.panel.is_none()));
    }
    assert_eq!(view.read_with(&cx, |view, _| (view.cols, view.rows)), size);
    assert!(
        !receiver
            .try_iter()
            .any(|message| matches!(message, Msg::Input(_) | Msg::Resize(_) | Msg::ResizeGrid(_)))
    );
}

#[gpui::test]
fn copy_click_preserves_selection_and_following_text_reaches_original_terminal(
    cx: &mut TestAppContext,
) {
    let (view, mut cx, receiver, _, commands) = fixture(cx);
    view.update(&mut cx, |view, _| {
        let mut selection =
            Selection::new(SelectionType::Simple, TermPoint::new(Line(0), Column(0)), Side::Left);
        selection.update(TermPoint::new(Line(0), Column(3)), Side::Right);
        view.session.as_ref().unwrap().term.lock().selection = Some(selection);
    });
    let before = view
        .read_with(&cx, |view, _| view.session.as_ref().unwrap().term.lock().selection_to_string());
    assert!(before.is_some());
    let bounds = hover(&view, &mut cx, &commands[0]);
    click(&mut cx, bounds);
    assert_eq!(clipboard(&mut cx).as_deref(), Some("first\n"));
    assert_eq!(
        view.read_with(&cx, |view, _| view
            .session
            .as_ref()
            .unwrap()
            .term
            .lock()
            .selection_to_string()),
        before
    );
    assert!(view.read_with(&cx, |view, _| view.reading.panel.is_none()));
    assert!(input(&receiver).is_empty());
    cx.simulate_input("z");
    assert_eq!(input(&receiver), b"z");
}

#[gpui::test]
fn copied_feedback_stays_on_original_result_until_expiry_after_pointer_moves(
    cx: &mut TestAppContext,
) {
    let (view, mut cx, _, _, commands) = fixture(cx);
    let bounds = hover(&view, &mut cx, &commands[0]);
    click(&mut cx, bounds);
    hover(&view, &mut cx, &commands[1]);
    assert!(copy_bounds(&mut cx, commands[0].id).is_some());
    assert!(copy_bounds(&mut cx, commands[1].id).is_some());
    assert!(view.read_with(&cx, |view, cx| {
        view.reading.output_hover.feedback.as_ref().unwrap().read(cx).is_copied()
    }));
    cx.executor().advance_clock(COPY_FEEDBACK_TTL);
    draw(&mut cx);
    assert!(copy_bounds(&mut cx, commands[0].id).is_none());
    assert!(copy_bounds(&mut cx, commands[1].id).is_some());
}

#[gpui::test]
fn clear_resize_and_registry_eviction_remove_hover_without_changing_clipboard(
    cx: &mut TestAppContext,
) {
    let (view, mut cx, _, proxy, commands) = fixture(cx);
    hover(&view, &mut cx, &commands[0]);
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("untouched".into())));
    view.update(&mut cx, |view, cx| {
        view.session.as_ref().unwrap().term.lock().clear_screen(ClearMode::Saved);
        view.reconcile_output_hover(cx);
    });
    draw(&mut cx);
    assert!(view.read_with(&cx, |view, _| view.reading.output_hover.hovered.is_none()));
    feed(&view, &mut cx, &proxy, execution("third\r\n").as_bytes());
    let third = view.read_with(&cx, |view, _| {
        view.session.as_ref().unwrap().term.lock().completed_commands().last().unwrap().clone()
    });
    hover(&view, &mut cx, &third);
    view.update(&mut cx, |view, cx| {
        let mut term = view.session.as_ref().unwrap().term.lock();
        let size =
            session::GridSize { columns: term.columns() + 1, screen_lines: term.screen_lines() };
        term.resize(size);
        drop(term);
        view.reconcile_output_hover(cx);
    });
    assert!(view.read_with(&cx, |view, _| view.reading.output_hover.hovered.is_none()));
    feed(&view, &mut cx, &proxy, execution("fourth\r\n").as_bytes());
    let fourth = view.read_with(&cx, |view, _| {
        view.session.as_ref().unwrap().term.lock().completed_commands().last().unwrap().clone()
    });
    hover(&view, &mut cx, &fourth);
    // 空输出命令仍占注册项；不靠滚出视口冒充32项注册表淘汰。
    let empty = "\x1b]133;A\x07$\x1b]133;B\x07x\x1b]133;C\x07\x1b]133;D;0\x07";
    feed(&view, &mut cx, &proxy, empty.repeat(33).as_bytes());
    assert!(!view.read_with(&cx, |view, _| {
        view.session.as_ref().unwrap().term.lock().completed_commands().any(|c| c.id == fourth.id)
    }));
    assert!(view.read_with(&cx, |view, _| view.reading.output_hover.hovered.is_none()));
    assert_eq!(clipboard(&mut cx).as_deref(), Some("untouched"));
}

#[gpui::test]
fn partially_evicted_result_loses_hover_even_when_the_visible_anchor_is_unchanged(
    cx: &mut TestAppContext,
) {
    let (view, mut cx, _, proxy) = open(cx);
    view.update(&mut cx, |view, _| {
        view.session.as_ref().unwrap().term.lock().grid_mut().update_history(250);
    });
    feed(&view, &mut cx, &proxy, execution(&"body\r\n".repeat(200)).as_bytes());
    let command = view.read_with(&cx, |view, _| {
        view.session.as_ref().unwrap().term.lock().completed_commands().last().unwrap().clone()
    });
    view.update(&mut cx, |view, cx| {
        view.session.as_ref().unwrap().term.lock().scroll_display(Scroll::Delta(100));
        cx.notify();
    });
    draw(&mut cx);
    let at = view.read_with(&cx, |view, _| {
        view.origin + point(view.cell_width * 2.0, view.line_height * 5.0)
    });
    cx.simulate_mouse_move(at, None, Modifiers::default());
    draw(&mut cx);
    let before = view.read_with(&cx, |view, _| view.reading.output_hover.viewport);
    assert!(view.read_with(&cx, |view, _| view.reading.output_hover.hovered.is_some()));
    feed(&view, &mut cx, &proxy, "more\r\n".repeat(100).as_bytes());
    assert!(view.read_with(&cx, |view, _| {
        let term = view.session.as_ref().unwrap().term.lock();
        term.reading_epoch() == command.output.epoch
            && term.completed_commands().any(|c| c.id == command.id)
            && term.reading_span().start > command.output.start
            && (
                (term.grid().scrolled_out() + term.history_size()) as i64
                    + i64::from(term.viewport_origin_for(view.rows).0),
                view.cols,
                view.rows,
            ) == before.unwrap()
    }));
    assert!(view.read_with(&cx, |view, _| view.reading.output_hover.hovered.is_none()));
}

#[gpui::test]
fn invalidated_pending_copy_cannot_publish(cx: &mut TestAppContext) {
    let (view, mut cx, _, _, commands) = fixture(cx);
    hover(&view, &mut cx, &commands[0]);
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("before".into())));
    view.update(&mut cx, |view, cx| {
        let target = view.reading.output_hover.hovered.unwrap();
        view.copy_hovered_output(target, cx);
        view.session.as_ref().unwrap().term.lock().clear_screen(ClearMode::Saved);
        view.reconcile_output_hover(cx);
    });
    draw(&mut cx);
    assert_eq!(clipboard(&mut cx).as_deref(), Some("before"));
    assert!(view.read_with(&cx, |view, _| view.reading.output_hover.feedback_target.is_none()));
}

#[gpui::test]
fn agent_alt_and_mouse_modes_never_show_result_controls_even_with_shift(cx: &mut TestAppContext) {
    let (view, mut cx, receiver, proxy, commands) = fixture(cx);
    let at = result_point(&view, &cx, &commands[0]);
    view.update(&mut cx, |view, _| {
        view.running_program = Some("codex".into());
        view.command_running_disproved = true;
    });
    cx.simulate_mouse_move(at, None, Modifiers::default());
    draw(&mut cx);
    assert!(view.read_with(&cx, |view, _| view.reading.output_hover.hovered.is_none()));
    view.update(&mut cx, |view, _| view.running_program = None);
    for mode in [b"\x1b[?1000h".as_slice(), b"\x1b[?1000l\x1b[?1049h"] {
        feed(&view, &mut cx, &proxy, mode);
        cx.simulate_mouse_move(at, None, Modifiers { shift: true, ..Modifiers::default() });
        draw(&mut cx);
        assert!(view.read_with(&cx, |view, _| view.reading.output_hover.hovered.is_none()));
    }
    assert!(input(&receiver).is_empty());
}
