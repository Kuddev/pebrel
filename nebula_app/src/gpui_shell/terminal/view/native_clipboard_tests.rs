//! Clipboard intake must retain native editor ownership and avoid implicit Enter.

use super::*;
use gpui::TestAppContext;
use startup_tests::{feed, open};

mod acceptance;

const OWNER: &str = "pwsh:42:clipboard-fixture";
const TEXT: &str = "Write-Output \"alpha\"\r\nWrite-Output \"beta\"\r\n";

fn report(view: &mut TerminalView, name: &str, value: &str, cx: &mut Context<TerminalView>) {
    view.process_event(TermEvent::UserVar { name: name.into(), value: value.into() }, cx);
}

fn ready(view: &mut TerminalView, cx: &mut Context<TerminalView>) {
    view.suggest.suggest_env = crate::display::SuggestEnv::Local;
    report(view, "pebrel_shell", OWNER, cx);
    report(view, "pebrel_clipboard_ready", OWNER, cx);
    // OSC 133 belongs to StreamProcessor, not the test helper's VT-only parser.
    view.session.as_ref().unwrap().term.lock().nebula_add_prompt_mark();
    feed(view, b"\x1b[?9001hPS> ");
    view.session.as_ref().unwrap().term.lock().nebula_mark_prompt_input();
    cx.write_to_clipboard(ClipboardItem::new_string(TEXT.into()));
}

fn input(receiver: &std::sync::mpsc::Receiver<Msg>) -> Vec<u8> {
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
fn native_clipboard_broadcast_keeps_multiline_input_unsubmitted(cx: &mut TestAppContext) {
    let supported = crate::platform::CAPABILITIES.native_shell_clipboard;
    for native_input in [true, false] {
        let (view, window, receiver) = open(cx);
        view.update(window, |view, cx| {
            ready(view, cx);
            if !native_input {
                feed(view, b"\x1b[?9001l");
            }
            for _ in 0..2 {
                assert_eq!(
                    view.native_clipboard_ready(),
                    supported,
                    "consecutive pastes retain editor admission"
                );
                assert_eq!(
                    cx.read_from_clipboard().and_then(|clip| clip.text()).as_deref(),
                    Some(TEXT)
                );
                view.apply_broadcast_text(TEXT, true, cx);
                let expected = if supported {
                    assert!(
                        !view.command_running,
                        "a native clipboard paste has not accepted the input"
                    );
                    assert!(view.suggest.last_committed.is_empty());
                    keymap::encode(
                        &gpui::Keystroke::parse("ctrl-shift-f11").unwrap(),
                        &view.term_mode(),
                    )
                    .unwrap()
                } else {
                    TEXT.replace("\r\n", "\r").into_bytes()
                };
                assert_eq!(input(&receiver), expected);
            }
        });
    }
}

#[gpui::test]
fn native_clipboard_does_not_retarget_stale_owner_or_remote_input(cx: &mut TestAppContext) {
    for remote in [false, true] {
        let (view, window, receiver) = open(cx);
        view.update(window, |view, cx| {
            ready(view, cx);
            if remote {
                view.suggest.suggest_env =
                    crate::display::SuggestEnv::Ssh { destination: "fixture.invalid".into() };
            } else {
                report(view, "pebrel_shell", "pwsh:99:replacement", cx);
            }
            view.apply_broadcast_text(TEXT, true, cx);
            assert_eq!(input(&receiver), TEXT.replace("\r\n", "\r").as_bytes());
        });
    }
}

#[gpui::test]
fn native_clipboard_action_is_admitted_only_on_supported_hosts(cx: &mut TestAppContext) {
    let (view, window, receiver) = open(cx);
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            ready(view, cx);
            if crate::platform::CAPABILITIES.native_shell_clipboard {
                view.paste(window, cx);
                assert_eq!(
                    input(&receiver),
                    keymap::encode(
                        &gpui::Keystroke::parse("ctrl-shift-f11").unwrap(),
                        &view.term_mode()
                    )
                    .unwrap()
                );
            } else {
                assert!(!view.try_native_clipboard_paste(TEXT, true, cx));
                assert!(
                    input(&receiver).is_empty(),
                    "unsupported hosts never dispatch the native chord"
                );
            }
            assert!(!view.command_running);
        });
    });
}

#[gpui::test]
fn native_clipboard_revocation_and_changed_clipboard_keep_the_original_text(
    cx: &mut TestAppContext,
) {
    for case in ["revoked", "new-prompt", "command", "clipboard"] {
        let (view, window, receiver) = open(cx);
        view.update(window, |view, cx| {
            ready(view, cx);
            match case {
                "revoked" => report(view, "pebrel_clipboard_ready", "", cx),
                "new-prompt" => report(view, "pebrel_shell", OWNER, cx),
                "command" => view.process_event(TermEvent::CommandStart, cx),
                "clipboard" => cx.write_to_clipboard(ClipboardItem::new_string("new clip".into())),
                _ => unreachable!(),
            }
            view.apply_broadcast_text(TEXT, true, cx);
            assert_eq!(input(&receiver), TEXT.replace("\r\n", "\r").as_bytes(), "{case}");
        });
    }
}

#[gpui::test]
fn native_clipboard_yields_to_negotiated_paste_and_foreground_modes(cx: &mut TestAppContext) {
    for sequence in ["\x1b[?2004h", "\x1b[?1049h", "\x1b[>1u"] {
        let (view, window, receiver) = open(cx);
        view.update(window, |view, cx| {
            ready(view, cx);
            view.session.as_ref().unwrap().term.lock().set_options(nebula_terminal::term::Config {
                kitty_keyboard: true,
                ..Default::default()
            });
            feed(view, sequence.as_bytes());
            view.apply_broadcast_text(TEXT, true, cx);
            let normalized = TEXT.replace("\r\n", "\r");
            let expected = if view.term_mode().contains(TermMode::BRACKETED_PASTE) {
                format!("\x1b[200~{normalized}\x1b[201~")
            } else {
                normalized
            };
            assert_eq!(input(&receiver), expected.as_bytes(), "{sequence:?}");
        });
    }
}
