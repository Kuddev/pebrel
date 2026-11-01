//! Drive the mounted terminal element; the fake PTY owns only protocol replies.
use super::*;
use std::sync::mpsc::Receiver;

const OWNER: &str = "bash:42:pointer-fixture";

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

fn wire(argument: usize) -> Vec<u8> {
    let mut bytes: Vec<_> = argument.to_string().bytes().flat_map(|digit| [0x1b, digit]).collect();
    bytes.extend(b"\x1b[45~");
    bytes
}

fn report(view: &mut TerminalView, name: &str, value: &str, cx: &mut Context<TerminalView>) {
    view.process_event(TermEvent::UserVar { name: name.into(), value: value.into() }, cx);
}

fn fixture(cx: &mut TestAppContext) -> (Entity<TerminalView>, VisualTestContext, Receiver<Msg>) {
    let (view, mut window, receiver) = link_fixture(cx, b"");
    view.update(&mut window, |view, cx| {
        view.suggest.suggest_env = crate::display::SuggestEnv::Local;
        report(view, "pebrel_shell", OWNER, cx);
        report(view, "pebrel_pointer_ready", OWNER, cx);
        view.session.as_ref().unwrap().term.lock().nebula_add_prompt_mark();
        super::super::super::startup_tests::feed(view, b"$ ");
        view.session.as_ref().unwrap().term.lock().nebula_mark_prompt_input();
        super::super::super::startup_tests::feed(view, b"abcdef");
    });
    draw(&mut window);
    input(&receiver);
    (view, window, receiver)
}

fn position(view: &Entity<TerminalView>, window: &VisualTestContext, column: f32) -> Point<Pixels> {
    view.read_with(window, |view, _| {
        point(view.origin.x + view.cell_width * column, view.origin.y + view.line_height * 0.5)
    })
}

fn gesture(view: &Entity<TerminalView>, window: &mut VisualTestContext, start: f32, end: f32) {
    let start = position(view, window, start);
    let end = position(view, window, end);
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    if start != end {
        window.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    }
    window.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    draw(window);
}

fn answer(view: &Entity<TerminalView>, window: &mut VisualTestContext, nonce: usize) {
    view.update(window, |view, cx| {
        report(view, "pebrel_pointer", &format!("{OWNER}\nutf8:{nonce}\n6\nabcdef"), cx);
    });
}

#[gpui::test]
fn shell_pointer_remains_available_after_bracketed_paste_and_native_undo(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = fixture(cx);
    view.update(&mut window, |view, cx| {
        super::super::super::startup_tests::feed(view, b"\x1b[?2004h");
        view.paste_now("g", cx);
        super::super::super::startup_tests::feed(view, b"g");
    });
    assert_eq!(input(&receiver), b"\x1b[200~g\x1b[201~");
    gesture(&view, &mut window, 3.1, 3.1);
    assert_eq!(input(&receiver), wire(20000));
    view.update(&mut window, |view, cx| {
        report(view, "pebrel_pointer", &format!("{OWNER}\nutf8:20000\n7\nabcdefg"), cx);
    });
    assert_eq!(input(&receiver), wire(2));
    view.update(&mut window, |view, cx| view.write_input(vec![31], cx));
    assert_eq!(input(&receiver), b"\x1f");
    gesture(&view, &mut window, 4.1, 4.1);
    assert_eq!(input(&receiver), wire(20001));
}

#[gpui::test]
fn shell_pointer_click_moves_native_cursor_and_drag_delete_preserves_prompt(
    cx: &mut TestAppContext,
) {
    let (view, mut window, receiver) = fixture(cx);
    gesture(&view, &mut window, 3.1, 3.1);
    assert_eq!(input(&receiver), wire(20000));
    answer(&view, &mut window, 20000);
    assert_eq!(input(&receiver), wire(2)); // native index 1

    gesture(&view, &mut window, 3.1, 4.9);
    assert_eq!(input(&receiver), wire(20001));
    view.read_with(&window, |view, _| {
        assert_eq!(
            view.session.as_ref().unwrap().term.lock().selection_to_string().as_deref(),
            Some("bc")
        );
    });
    answer(&view, &mut window, 20001);
    assert_eq!(input(&receiver), wire(4));
    window.simulate_keystrokes("backspace");
    assert_eq!(input(&receiver), [wire(4099), wire(8198)].concat());
    view.read_with(&window, |view, _| {
        assert!(view.session.as_ref().unwrap().term.lock().nebula_prompt_active());
        assert!(view.session.as_ref().unwrap().term.lock().selection.is_none());
    });
    window.simulate_keystrokes("backspace");
    assert_eq!(input(&receiver), b"\x7f");
}

#[gpui::test]
fn shell_pointer_fast_delete_waits_for_the_verified_drag_reply(cx: &mut TestAppContext) {
    for rejected in [false, true] {
        let (view, mut window, receiver) = fixture(cx);
        gesture(&view, &mut window, 3.1, 4.9);
        assert_eq!(input(&receiver), wire(20000));
        window.simulate_keystrokes("delete");
        assert!(input(&receiver).is_empty());
        if rejected {
            view.update(&mut window, |view, cx| {
                report(view, "pebrel_pointer", &format!("{OWNER}\nunsupported:20000"), cx)
            });
            assert!(input(&receiver).is_empty());
        } else {
            answer(&view, &mut window, 20000);
            assert_eq!(input(&receiver), [wire(4), wire(4099), wire(8198)].concat());
        }
    }
}

#[gpui::test]
fn shell_pointer_bind_x_redraw_restores_only_the_verified_selection(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = fixture(cx);
    gesture(&view, &mut window, 3.1, 4.9);
    input(&receiver);
    view.update(&mut window, |view, _| {
        super::super::super::startup_tests::feed(view, b"\r\x1b[K");
        assert!(view.session.as_ref().unwrap().term.lock().selection.is_none());
    });
    answer(&view, &mut window, 20000);
    assert_eq!(input(&receiver), wire(4));
    view.update(&mut window, |view, cx| {
        super::super::super::startup_tests::feed(view, b"$ abcdef\x1b[1;6H");
        view.process_event(TermEvent::Wakeup, cx);
        assert_eq!(
            view.session.as_ref().unwrap().term.lock().selection_to_string().as_deref(),
            Some("bc")
        );
        super::super::super::startup_tests::feed(view, b"\r\x1b[K$ changed");
        view.process_event(TermEvent::Wakeup, cx);
        assert!(view.session.as_ref().unwrap().term.lock().selection.is_none());
    });
}

#[gpui::test]
fn shell_pointer_release_endpoint_matches_the_highlight_when_last_move_is_missing(
    cx: &mut TestAppContext,
) {
    let (view, mut window, receiver) = fixture(cx);
    let start = position(&view, &window, 3.1);
    let previous = position(&view, &window, 4.1);
    let release = position(&view, &window, 6.9);
    window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    window.simulate_mouse_move(previous, Some(MouseButton::Left), Modifiers::default());
    window.simulate_mouse_up(release, MouseButton::Left, Modifiers::default());
    assert_eq!(input(&receiver), wire(20000));
    answer(&view, &mut window, 20000);
    view.read_with(&window, |view, _| {
        assert_eq!(
            view.session.as_ref().unwrap().term.lock().selection_to_string().as_deref(),
            Some("bcde")
        );
    });
    assert_eq!(input(&receiver), wire(6));
    window.simulate_keystrokes("backspace");
    assert_eq!(input(&receiver), [wire(4099), wire(8200)].concat());
}

#[gpui::test]
fn shell_pointer_wide_character_half_cell_snaps_highlight_and_native_range_together(
    cx: &mut TestAppContext,
) {
    let (view, mut window, receiver) = fixture(cx);
    view.update(&mut window, |view, _| {
        super::super::super::startup_tests::feed(view, "\r\x1b[K$ abc中ef".as_bytes());
    });
    gesture(&view, &mut window, 2.1, 5.9);
    assert_eq!(input(&receiver), wire(20000));
    view.update(&mut window, |view, cx| {
        report(view, "pebrel_pointer", &format!("{OWNER}\nutf8:20000\n8\nabc中ef"), cx);
        assert_eq!(
            view.session.as_ref().unwrap().term.lock().selection_to_string().as_deref(),
            Some("abc")
        );
    });
    assert_eq!(input(&receiver), wire(4));
    window.simulate_keystrokes("backspace");
    assert_eq!(input(&receiver), [wire(4098), wire(8198)].concat());
}

#[gpui::test]
fn shell_pointer_lost_and_late_replies_never_retarget_a_newer_click(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = fixture(cx);
    // More than the former four queue slots; each click replaces the lost one.
    for nonce in 20000..20006 {
        gesture(&view, &mut window, 3.1, 3.1);
        assert_eq!(input(&receiver), wire(nonce));
    }
    answer(&view, &mut window, 20000);
    assert!(input(&receiver).is_empty());
    answer(&view, &mut window, 20005);
    assert_eq!(input(&receiver), wire(2));
    answer(&view, &mut window, 20005);
    assert!(input(&receiver).is_empty());
}

#[gpui::test]
fn shell_pointer_user_input_and_new_prompt_invalidate_in_flight_queries(cx: &mut TestAppContext) {
    for change in ["typing", "prompt", "resize", "hidden", "focus"] {
        let (view, mut window, receiver) = fixture(cx);
        gesture(&view, &mut window, 3.1, 4.9);
        input(&receiver);
        match change {
            "typing" => window.simulate_keystrokes("x"),
            "prompt" => {
                view.update(&mut window, |view, cx| report(view, "pebrel_shell", OWNER, cx))
            },
            "resize" => window.simulate_resize(gpui::size(px(900.0), px(500.0))),
            "hidden" => view.update(&mut window, |view, cx| view.set_output_visible(false, cx)),
            "focus" => window.deactivate_window(),
            _ => unreachable!(),
        }
        draw(&mut window);
        input(&receiver);
        answer(&view, &mut window, 20000);
        assert!(input(&receiver).is_empty(), "{change}");
    }
}

#[gpui::test]
fn shell_pointer_does_not_query_output_or_ai_fullscreen_input(cx: &mut TestAppContext) {
    for mode in ["output", "alt", "mouse", "agent", "running"] {
        let (view, mut window, receiver) = fixture(cx);
        view.update(&mut window, |view, _| match mode {
            "alt" => super::super::super::startup_tests::feed(view, b"\x1b[?1049h"),
            "mouse" => super::super::super::startup_tests::feed(view, b"\x1b[?1000h\x1b[?1006h"),
            "agent" => view.running_program = Some("codex".into()),
            "running" => view.command_running = true,
            _ => {},
        });
        let column = if mode == "output" { 0.1 } else { 3.1 };
        gesture(&view, &mut window, column, column);
        let bytes = input(&receiver);
        if mode == "mouse" {
            assert!(
                bytes.starts_with(b"\x1b[<0;"),
                "native application mouse reporting is preserved"
            );
            assert!(!bytes.windows(5).any(|part| part == b"\x1b[45~"));
        } else {
            assert!(bytes.is_empty(), "{mode}");
        }
    }
}

#[gpui::test]
fn shell_pointer_powershell_uses_owned_native_chords_and_mode_aware_keys(cx: &mut TestAppContext) {
    for native_console in [false, true] {
        let (view, mut window, receiver) = fixture(cx);
        let owner = "pwsh:42:pointer-fixture";
        view.update(&mut window, |view, cx| {
            report(view, "pebrel_shell", owner, cx);
            report(view, "pebrel_pointer_ready", owner, cx);
            if native_console {
                super::super::super::startup_tests::feed(view, b"\x1b[?9001h");
            }
        });
        let mode = view.read_with(&window, |view, _| view.term_mode());
        let wire = |argument: usize| {
            let prefix = super::super::super::super::keymap::encode(
                &gpui::Keystroke::parse("ctrl-shift-f10").unwrap(),
                &mode,
            )
            .unwrap();
            let query = super::super::super::super::keymap::encode(
                &gpui::Keystroke::parse("ctrl-shift-f12").unwrap(),
                &mode,
            )
            .unwrap();
            let mut bytes = Vec::new();
            for digit in argument.to_string().bytes() {
                bytes.extend_from_slice(&prefix);
                bytes.push(digit);
            }
            bytes.extend(query);
            bytes
        };
        gesture(&view, &mut window, 3.1, 4.9);
        assert_eq!(input(&receiver), wire(20000));
        view.update(&mut window, |view, cx| {
            report(view, "pebrel_pointer", &format!("{owner}\nutf8:20000\n6\nabcdef"), cx);
        });
        assert_eq!(input(&receiver), wire(4));
        window.simulate_keystrokes("backspace");
        assert_eq!(input(&receiver), [wire(4099), wire(8198)].concat());
        if native_console
            && crate::platform::Platform::current() == crate::platform::Platform::Windows
        {
            assert!(wire(1).ends_with(b"_"), "negotiated native records must not fall back to VT");
        }
    }
}

#[gpui::test]
fn shell_pointer_native_undo_preserves_only_the_current_powershell_prompt(cx: &mut TestAppContext) {
    for (owner, alternate_screen, preserves) in [
        ("pwsh:42:pointer-fixture", false, true),
        (OWNER, false, false),
        ("pwsh:42:pointer-fixture", true, false),
    ] {
        for key in ["ctrl-x", "ctrl-z"] {
            let (view, mut window, receiver) = fixture(cx);
            view.update(&mut window, |view, cx| {
                report(view, "pebrel_shell", owner, cx);
                report(view, "pebrel_pointer_ready", owner, cx);
                if alternate_screen {
                    super::super::super::startup_tests::feed(view, b"\x1b[?1049h");
                }
            });
            // The mounted terminal receives keyboard input after gaining focus.
            gesture(&view, &mut window, 3.1, 3.1);
            input(&receiver);
            window.simulate_keystrokes(key);
            assert!(!input(&receiver).is_empty(), "native key must still reach PSReadLine");
            view.read_with(&window, |view, _| {
                assert_eq!(
                    view.session.as_ref().unwrap().term.lock().nebula_prompt_active(),
                    preserves
                );
            });
        }
    }
}
