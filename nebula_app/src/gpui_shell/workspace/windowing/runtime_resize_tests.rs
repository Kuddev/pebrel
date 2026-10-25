use super::*;
use crate::gpui_shell::terminal::view::TerminalLaunch;
use gpui::{TestAppContext, point, size};
use nebula_terminal::event_loop::Msg;

#[gpui::test]
fn resize_reply_commits_both_pty_sizes_before_the_next_input(cx: &mut TestAppContext) {
    transfer_tests::initialize_test(cx);
    cx.update(|cx| {
        let (id, workspace) = transfer_tests::open_test_window(cx, 0);
        let entry = entry_by_id(id, cx).unwrap();
        let (ids, receivers) = entry
            .handle
            .update(cx, |_, window, cx| {
                workspace.update(cx, |workspace, cx| {
                    let mut panes = Vec::new();
                    let mut receivers = Vec::new();
                    for _ in 0..2 {
                        let pane = workspace.new_pane(
                            (80, 24),
                            TerminalLaunch::Local {
                                cwd: None,
                                shell: Some(nebula_terminal::tty::Shell::new(
                                    "pebrel-test-missing-shell-executable".into(),
                                    vec![],
                                )),
                                shell_name: None,
                            },
                            None,
                            window,
                            cx,
                        );
                        receivers.push(pane.view.update(cx, |view, cx| {
                            let receiver = view.install_completion_test_session();
                            // Match the spawn grid, then let the real layout own later sizes.
                            view.set_layout(
                                point(px(0.0), px(0.0)),
                                px(10.0),
                                px(20.0),
                                size(px(800.0), px(480.0)),
                                1.0,
                                cx,
                            );
                            view.mark_structural_resize();
                            receiver
                        }));
                        panes.push(pane);
                    }
                    let ids = [panes[0].id, panes[1].id];
                    let mut tree = SplitTree::leaf(ids[0]);
                    assert!(tree.split_leaf(
                        ids[0],
                        ids[1],
                        nebula_split::SplitDirection::LeftRight,
                        0.5
                    ));
                    workspace.insert_tab_at(
                        0,
                        WorkspaceTab::Terminal {
                            tree,
                            panes,
                            focused: ids[0],
                            zoomed: false,
                            broadcast: false,
                        },
                        TabMeta::default(),
                    );
                    (ids, receivers)
                })
            })
            .unwrap();
        entry
            .handle
            .update(cx, |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            })
            .unwrap();
        let before: Vec<_> = receivers
            .iter()
            .map(|receiver| {
                receiver
                    .try_iter()
                    .filter_map(|message| match message {
                        Msg::Resize(size) => Some(size.num_cols),
                        _ => None,
                    })
                    .last()
                    .expect("initial layout must resize the fixture PTY")
            })
            .collect();
        let (dispatch, reply) = RuntimeDispatch::test(RuntimeCommand::ResizePane {
            window_id: Some(id),
            pane_id: ids[0],
            ratio: 0.7,
        });
        dispatch_runtime(dispatch, cx);
        assert!(reply.try_recv().unwrap().is_ok());
        // No frame pump or timer is allowed between the resize reply and input.
        for pane_id in ids {
            let (dispatch, reply) = RuntimeDispatch::test(RuntimeCommand::Prompt {
                window_id: Some(id),
                pane_id,
                text: "x".into(),
                submit: false,
            });
            dispatch_runtime(dispatch, cx);
            assert!(reply.try_recv().unwrap().is_ok());
        }
        let after: Vec<_> = receivers
            .iter()
            .map(|receiver| {
                let messages: Vec<_> = receiver.try_iter().collect();
                let resize = messages
                    .iter()
                    .position(|message| matches!(message, Msg::Resize(_)))
                    .expect("resize acknowledgement must follow the PTY size message");
                let input = messages
                    .iter()
                    .position(|message| matches!(message, Msg::Input(_)))
                    .expect("next request must still deliver input");
                assert!(resize < input, "input must not overtake structural geometry");
                let Msg::Resize(size) = &messages[resize] else { unreachable!() };
                size.num_cols
            })
            .collect();
        assert!(after[0] > before[0]);
        assert!(after[1] < before[1]);
        let (dispatch, reply) = RuntimeDispatch::test(RuntimeCommand::ResizePane {
            window_id: Some(id),
            pane_id: u64::MAX,
            ratio: 0.7,
        });
        dispatch_runtime(dispatch, cx);
        assert!(reply.try_recv().unwrap().is_err());
        assert!(receivers.iter().all(|receiver| receiver.try_recv().is_err()));
    });
}
