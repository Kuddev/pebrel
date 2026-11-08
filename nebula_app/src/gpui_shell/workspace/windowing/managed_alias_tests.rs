use super::*;
use crate::ai_agents::AgentKind;
use gpui::TestAppContext;
use nebula_terminal::event::Event as TermEvent;
use nebula_terminal::event_loop::Msg;

#[gpui::test]
fn managed_alias_attach_persists_native_identity_and_local_commands_do_not_hold_a_turn(
    cx: &mut TestAppContext,
) {
    transfer_tests::initialize_test(cx);
    cx.update(|cx| {
        let (window_id, workspace) = transfer_tests::open_test_window(cx, 1);
        let entry = entry_by_id(window_id, cx).unwrap();
        entry.handle.update(cx, |_, window, cx| {
            workspace.update(cx, |workspace, cx| {
                let view = workspace.tabs[0].focused_view().unwrap().clone();
                let pane_id = view.read(cx).pane_id;
                let receiver = view.update(cx, |view, cx| {
                    let receiver = view.install_completion_test_session();
                    let wire = format!(
                        "nebula-hook/1 source=codex process={pane_id}:1 codex_hooks=full\n{}",
                        json!({"hook_event_name":"SessionStart", "session_id":"alias-native-one", "bridge_sequence":1}),
                    );
                    view.remote_files_test_event(TermEvent::AiHookEnvelope(wire.into_bytes()), cx);
                    receiver
                });
                let attach = |id: &str| RuntimeCommand::AgentAttach {
                    window_id: Some(window_id), pane_id, name: "reuse-worker".into(),
                    kind: AgentKind::Codex, expected_session_id: id.into(),
                };
                assert_eq!(workspace.execute_runtime_command(&attach("stale"), window, cx)
                    .unwrap_err().code, "agent_identity_mismatch");
                let attached = workspace.execute_runtime_command(&attach("alias-native-one"), window, cx).unwrap();
                assert_eq!(attached["action"]["attached"], true);
                let old_id = attached["action"]["agent"]["agent_id"].as_str().unwrap().to_owned();
                let old_generation = attached["action"]["agent"]["generation"].as_u64().unwrap();
                assert!(receiver.try_iter().all(|message| !matches!(message, Msg::Input(_))));
                let saved = workspace.snapshot_session(cx);
                let crate::session::LayoutSession::Pane { agent: Some(saved), .. } =
                    saved.tabs[0].layout.as_ref().unwrap() else { panic!("native Agent not saved") };
                assert_eq!(saved.managed_name.as_deref(), Some("reuse-worker"));
                assert_eq!(saved.session_id.as_deref(), Some("alias-native-one"));
                let command = RuntimeCommand::AgentPrompt {
                    agent: old_id.clone(), generation: Some(old_generation),
                    text: "/model".into(), submit: true,
                };
                let result = workspace.execute_runtime_command(&command, window, cx).unwrap();
                assert_eq!(result["action"]["local_command"], true);
                assert_ne!(view.read(cx).runtime_task_state(), crate::runtime_api::RuntimeTaskState::Running);
                let bytes: Vec<_> = receiver.try_iter().filter_map(|message| match message {
                    Msg::Input(bytes) => Some(bytes.into_owned()), _ => None,
                }).flatten().collect();
                assert!(String::from_utf8(bytes).unwrap().contains("/model"));
                view.update(cx, |view, cx| {
                    let wire = format!(
                        "nebula-hook/1 source=codex process={pane_id}:1 codex_hooks=full\n{}",
                        json!({"hook_event_name":"SessionStart", "session_id":"alias-native-two", "bridge_sequence":2}),
                    );
                    view.remote_files_test_event(TermEvent::AiHookEnvelope(wire.into_bytes()), cx);
                });
                assert_eq!(workspace.execute_runtime_command(&command, window, cx).unwrap_err().code,
                    "agent_identity_mismatch");
                assert!(receiver.try_iter().all(|message| !matches!(message, Msg::Input(_))));
                workspace.execute_runtime_command(&RuntimeCommand::Snapshot, window, cx).unwrap();
                assert_eq!(workspace.execute_runtime_command(&command, window, cx).unwrap_err().code,
                    "agent_replaced");
                assert!(receiver.try_iter().all(|message| !matches!(message, Msg::Input(_))));
                let attached = workspace.execute_runtime_command(&attach("alias-native-two"), window, cx).unwrap();
                assert_ne!(attached["action"]["agent"]["agent_id"], old_id);
                assert!(attached["action"]["agent"]["generation"].as_u64().unwrap() > old_generation);
                assert!(receiver.try_iter().all(|message| !matches!(message, Msg::Input(_))));
            });
        }).unwrap();
    });
}
