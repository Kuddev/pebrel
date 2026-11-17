use super::*;
use crate::notify::{Notification, test_delivery};
use gpui::TestAppContext;
use pebrel_terminal::event_loop::Msg;
use std::time::{Duration, Instant};

#[gpui::test]
fn runtime_notifications_deduplicate_before_bell_and_preserve_hidden_window(
    cx: &mut TestAppContext,
) {
    let filter = "gpui_shell::workspace::windowing::runtime_notify_tests::runtime_notifications_deduplicate_before_bell_and_preserve_hidden_window";
    if crate::platform::profile_test_isolation::run_exact_child(filter, None) {
        return;
    }
    let settings = pebrel_settings::settings_path();
    let raw = std::fs::read_to_string(&settings).unwrap();
    std::fs::write(&settings, pebrel_settings::apply_updates(&raw, &[("bell", "visual".into())]))
        .unwrap();
    let _capture = test_delivery::start(Instant::now());
    transfer_tests::initialize_test(cx);
    cx.update(|cx| {
        let (id, workspace) = transfer_tests::open_test_window(cx, 2);
        let views: Vec<_> =
            workspace.read(cx).tabs.iter().map(|tab| tab.focused_view().unwrap().clone()).collect();
        let panes: Vec<_> = workspace
            .read(cx)
            .tabs
            .iter()
            .map(|tab| match tab {
                WorkspaceTab::Terminal { focused, .. } => *focused,
                _ => unreachable!(),
            })
            .collect();
        let receivers: Vec<_> = views
            .iter()
            .map(|view| view.update(cx, |view, _| view.install_completion_test_session()))
            .collect();
        workspace.update(cx, |workspace, _| workspace.window_hidden = true);
        let active = workspace.read(cx).active;
        let initial = views[0].read(cx).bell_flash_generation();
        let other_initial = views[1].read(cx).bell_flash_generation();
        let send = |pane_id, message: &str, cx: &mut App| {
            let (dispatch, reply) = RuntimeDispatch::test(RuntimeCommand::Notify {
                window_id: Some(id),
                pane_id,
                title: Some("notification fixture".into()),
                message: message.into(),
            });
            dispatch_runtime(dispatch, cx);
            reply.try_recv().unwrap()
        };

        assert!(send(panes[0], "touch the key", cx).is_ok());
        test_delivery::advance(Duration::from_millis(200));
        assert!(send(panes[0], "touch the key", cx).is_ok());
        assert_eq!(views[0].read(cx).bell_flash_generation(), initial + 1);
        assert_eq!(test_delivery::messages().len(), 1);
        assert!(workspace.read(cx).tab_meta[0].has_bell);

        assert!(send(panes[1], "touch the key", cx).is_ok());
        assert_eq!(views[1].read(cx).bell_flash_generation(), other_initial + 1);
        assert_eq!(test_delivery::messages().len(), 2);

        let entry = entry_by_id(id, cx).unwrap();
        entry
            .handle
            .update(cx, |_, window, cx| {
                workspace.update(cx, |workspace, cx| {
                    workspace.deliver_pane_notification(
                        panes[0],
                        Notification::AiTurn {
                            program: "codex".into(),
                            message: Some("task finished".into()),
                            attention: false,
                        },
                        window,
                        cx,
                    );
                });
            })
            .unwrap();
        assert_eq!(test_delivery::messages().len(), 3);
        assert_eq!(test_delivery::messages()[2].1, "task finished");

        test_delivery::advance(Duration::from_secs(4));
        assert!(send(panes[0], "touch the key", cx).is_ok());
        assert_eq!(views[0].read(cx).bell_flash_generation(), initial + 2);
        assert_eq!(test_delivery::messages().len(), 4);

        std::fs::write(&settings, pebrel_settings::apply_updates(&raw, &[("bell", "none".into())]))
            .unwrap();
        test_delivery::advance(Duration::from_secs(4));
        assert!(send(panes[0], "another action", cx).is_ok());
        assert_eq!(views[0].read(cx).bell_flash_generation(), initial + 2);
        assert_eq!(test_delivery::messages().len(), 5);
        assert!(send(u64::MAX, "invalid pane", cx).is_err());
        assert_eq!(test_delivery::messages().len(), 5);

        assert!(workspace.read(cx).window_hidden);
        assert_eq!(workspace.read(cx).active, active);
        for receiver in receivers {
            assert!(receiver.try_iter().all(|message| !matches!(message, Msg::Input(_))));
        }
    });
}
