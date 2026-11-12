use super::*;
use gpui::TestAppContext;
use nebula_terminal::event_loop::Msg;

#[gpui::test]
fn upload_completion_is_paste_only_and_rejects_stale_owners(cx: &mut TestAppContext) {
    for change in [
        "none",
        "typing",
        "submit",
        "focus",
        "session",
        "disconnect",
        "program",
        "failure",
        "cancel",
    ] {
        let (view, window, _) = super::super::super::startup_tests::open(cx);
        let (send, receive) = tokio::sync::oneshot::channel();
        let (wake, signal) = tokio::sync::mpsc::channel(1);
        let mut receiver = None;
        window.update(|window, cx| {
            view.update(cx, |view, cx| {
                receiver = Some(view.install_remote_files_test_session("fixture", cx));
                view.await_uploaded_paths(FileIntake::test_pending(), receive, signal, window, cx);
                match change {
                    "typing" => view.write_input(b"describe ".to_vec(), cx),
                    "submit" => view.write_input(b"\r".to_vec(), cx),
                    "focus" => {
                        window.blur();
                    },
                    "session" => {
                        view.install_remote_files_test_session("fixture", cx);
                    },
                    "disconnect" => view.apply_ssh_stage(
                        crate::ssh_session::SshStage::Failed("offline".into()),
                        cx,
                    ),
                    "program" => view.running_program = Some("another-program".into()),
                    "cancel" => view.path_drop.invalidate(),
                    _ => {},
                }
            })
        });
        window.run_until_parked();
        let receiver = receiver.unwrap();
        receiver.try_iter().for_each(drop);
        let paths = vec!["/home/test/a '中.txt".into(), "/home/test/b.txt".into()];
        let expected = drop_text_for_paths(&paths, PathQuote::Posix).unwrap();
        send.send(if change == "failure" {
            Err("permission denied".into())
        } else {
            Ok(UploadedPaths::test_paths(paths))
        })
        .ok()
        .unwrap();
        drop(wake);
        window.run_until_parked();
        let bytes: Vec<u8> = receiver
            .try_iter()
            .filter_map(|message| match message {
                Msg::Input(bytes) => Some(bytes.into_owned()),
                _ => None,
            })
            .flatten()
            .collect();
        if matches!(change, "none" | "typing") {
            assert_eq!(bytes, expected.as_bytes(), "{change}");
            assert!(!bytes.contains(&b'\r') && !bytes.contains(&b'\n'));
        } else {
            assert!(bytes.is_empty(), "{change}");
        }
        assert!(!view.read_with(window, |view, _| view.path_drop.pending));
    }
}

#[gpui::test]
fn visible_cancel_stops_path_insertion_and_fits_narrow_terminals(cx: &mut TestAppContext) {
    use gpui::{AppContext as _, Modifiers, MouseButton, size};
    use gpui_component::Root;

    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::terminal::init(cx);
        crate::gpui_shell::math_view::register(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord));
    });
    let (send, receive) = tokio::sync::oneshot::channel();
    let (_wake, signal) = tokio::sync::mpsc::channel(1);
    let mut result = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            TerminalView::new(
                42,
                (80, 24),
                super::super::super::TerminalLaunch::Local {
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
        let receiver = view.update(cx, |view, cx| {
            let receiver = view.install_remote_files_test_session("fixture", cx);
            view.await_uploaded_paths(FileIntake::test_pending(), receive, signal, window, cx);
            receiver
        });
        result = Some((view.clone(), receiver));
        Root::new(view, window, cx)
    });
    let (view, receiver) = result.unwrap();
    window.simulate_resize(size(px(360.0), px(500.0)));
    window.run_until_parked();
    window.update(|window, cx| window.draw(cx).clear(cx));
    let progress = window.debug_bounds("file-upload-progress").unwrap();
    let cancel = window.debug_bounds("cancel-file-upload").unwrap();
    assert!(progress.origin.x >= px(0.0) && progress.right() <= px(360.0));
    assert!(cancel.size.width > px(0.0) && cancel.size.height > px(0.0));
    assert!(progress.contains(&cancel.center()));
    receiver.try_iter().for_each(drop);
    window.simulate_mouse_down(cancel.center(), MouseButton::Left, Modifiers::default());
    window.simulate_mouse_up(cancel.center(), MouseButton::Left, Modifiers::default());
    window.run_until_parked();
    assert!(view.read_with(window, |view, _| view.path_drop.upload.as_ref().unwrap().cancelled()));
    send.send(Ok(UploadedPaths::test_paths(vec!["/home/test/uploaded.txt".into()]))).ok().unwrap();
    window.run_until_parked();
    window.update(|window, cx| window.draw(cx).clear(cx));
    assert!(window.debug_bounds("file-upload-progress").is_none());
    assert!(!receiver.try_iter().any(|message| matches!(message, Msg::Input(_))));
}
