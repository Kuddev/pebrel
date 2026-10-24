use super::super::{Msg, Settings, TerminalLaunch, TerminalViewEvent, startup_tests};
use super::*;
use gpui::{AppContext as _, Entity, Modifiers, TestAppContext, VisualTestContext, px};
use gpui_component::Root;
use std::sync::mpsc::Receiver;

fn open(cx: &mut TestAppContext) -> (Entity<TerminalView>, VisualTestContext, Receiver<Msg>) {
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
        let receiver = view.update(cx, |view, cx| {
            let receiver = view.install_remote_files_test_session("fixture@example.invalid", cx);
            view.spawn_at -= TerminalView::STARTUP_GRID_GRACE;
            view.copy_on_select = false;
            view.ghost_enabled = false;
            receiver
        });
        view.read(cx).focus_handle.clone().focus(window, cx);
        result = Some((view.clone(), receiver));
        Root::new(view, window, cx)
    });
    let mut window = window.clone();
    window.simulate_resize(gpui::size(px(700.0), px(500.0)));
    window.update(|window, _| window.activate_window());
    draw(&mut window);
    let (view, receiver) = result.unwrap();
    receiver.try_iter().for_each(drop);
    (view, window, receiver)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
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

fn uploaded() -> UploadedFiles {
    let root = tempfile::tempdir().unwrap();
    let paths: Vec<_> = ["report's.pdf", "第二份.txt"]
        .into_iter()
        .map(|name| {
            let path = root.path().join(name);
            std::fs::write(&path, b"content").unwrap();
            path
        })
        .collect();
    crate::ssh_session::runtime().unwrap().block_on(async {
        let (_, session) = crate::ssh_sftp::test_peer::connect(b"untouched").await;
        clipboard_files::upload_for_test(session, paths).await.unwrap()
    })
}

fn start(
    view: &Entity<TerminalView>,
    window: &mut VisualTestContext,
) -> (Operation, futures::channel::oneshot::Sender<Result<UploadedFiles, String>>) {
    let operation = Operation::default();
    let (sender, receiver) = futures::channel::oneshot::channel();
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.start_file_paste(
                2,
                operation.clone(),
                None,
                async { receiver.await.unwrap() },
                window,
                cx,
            )
        });
    });
    draw(window);
    (operation, sender)
}

#[gpui::test]
fn successful_batch_pastes_quoted_paths_without_submit_or_broadcast(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = open(cx);
    let broadcasts = std::rc::Rc::new(std::cell::Cell::new(0));
    let count = broadcasts.clone();
    let _subscription = window.update(|_, cx| {
        cx.subscribe(&view, move |_, event, _| {
            if matches!(event, TerminalViewEvent::UserInput(_)) {
                count.set(count.get() + 1);
            }
        })
    });
    view.update(&mut window, |view, _| startup_tests::feed(view, b"\x1b[?2004h"));
    let (_, sender) = start(&view, &mut window);
    assert!(window.debug_bounds("ssh-file-paste").is_some());
    assert!(input(&receiver).is_empty());
    assert!(sender.send(Ok(uploaded())).is_ok());
    draw(&mut window);
    let written = String::from_utf8(input(&receiver)).unwrap();
    assert_eq!(written, "\x1b[200~'/project/report'\\''s.pdf' /project/第二份.txt \x1b[201~");
    assert!(!written.contains(['\r', '\n']));
    assert_eq!(broadcasts.get(), 0);
    assert!(view.read_with(&window, |view, _| view.file_paste.operation.is_none()));
    assert!(window.debug_bounds("ssh-file-paste").is_none());
}

#[gpui::test]
fn escape_and_the_real_cancel_button_discard_late_success(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = open(cx);
    for click in [false, true] {
        let (operation, sender) = start(&view, &mut window);
        if click {
            window.simulate_resize(gpui::size(px(360.0), px(500.0)));
            draw(&mut window);
            let bounds = window.debug_bounds("ssh-file-paste-cancel").unwrap();
            let banner = window.debug_bounds("ssh-file-paste").unwrap();
            assert!(bounds.size.height >= px(32.0));
            assert!(banner.contains(&bounds.center()));
            window.simulate_click(bounds.center(), Modifiers::default());
        } else {
            window.simulate_keystrokes("escape");
        }
        assert!(operation.is_cancelled());
        assert!(sender.send(Ok(uploaded())).is_ok());
        draw(&mut window);
        assert!(input(&receiver).is_empty());
        window.update(|window, cx| view.read(cx).focus_handle.clone().focus(window, cx));
    }
}

#[gpui::test]
fn switching_focus_away_and_back_does_not_revive_a_pending_paste(cx: &mut TestAppContext) {
    let (view, mut window, receiver) = open(cx);
    let (operation, sender) = start(&view, &mut window);
    window.update(|window, cx| {
        let other = cx.focus_handle();
        other.focus(window, cx);
    });
    window.run_until_parked();
    window.update(|window, cx| view.read(cx).focus_handle.clone().focus(window, cx));
    assert!(operation.is_cancelled());
    assert!(sender.send(Ok(uploaded())).is_ok());
    draw(&mut window);
    assert!(input(&receiver).is_empty());
}

#[gpui::test]
fn input_hidden_panes_closed_sessions_and_failures_never_insert_late_paths(
    cx: &mut TestAppContext,
) {
    let (view, mut window, mut receiver) = open(cx);
    for action in ["input", "hidden", "shutdown", "replacement", "failure"] {
        let (operation, sender) = start(&view, &mut window);
        view.update(&mut window, |view, cx| match action {
            "input" => view.write_input(b"x".to_vec(), cx),
            "hidden" => view.set_output_visible(false, cx),
            "shutdown" => view.shutdown(),
            "replacement" => {
                receiver = view.install_remote_files_test_session("fixture@example.invalid", cx);
            },
            _ => {},
        });
        input(&receiver);
        if action == "failure" {
            assert!(sender.send(Err("upload rejected".into())).is_ok());
        } else {
            assert!(sender.send(Ok(uploaded())).is_ok());
            assert!(operation.is_cancelled() || action == "replacement");
        }
        draw(&mut window);
        assert!(input(&receiver).is_empty());
        assert!(view.read_with(&window, |view, _| view.file_paste.operation.is_none()));
        view.update(&mut window, |view, cx| view.set_output_visible(true, cx));
    }
}

#[gpui::test]
fn ordinary_path_text_remains_text_and_repeated_file_requests_do_not_restart(
    cx: &mut TestAppContext,
) {
    let (view, mut window, receiver) = open(cx);
    window.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.paste_clipboard_item(
                Some(gpui::ClipboardItem::new_string(r"C:\report.pdf".into())),
                window,
                cx,
            );
            assert!(view.file_paste.operation.is_none());
        })
    });
    assert_eq!(input(&receiver), br"C:\report.pdf");
    let (operation, sender) = start(&view, &mut window);
    window.update(|window, cx| view.update(cx, |view, cx| view.paste_files(vec![], window, cx)));
    assert!(!operation.is_cancelled());
    assert!(sender.send(Err("fixture stopped".into())).is_ok());
    draw(&mut window);
    assert!(input(&receiver).is_empty());
}

#[test]
fn dropping_the_paste_owner_requests_cancellation() {
    let operation = Operation::default();
    let state = FilePasteState { operation: Some(operation.clone()), task: None, count: 0 };
    drop(state);
    assert!(operation.is_cancelled());
}
