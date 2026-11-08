use super::*;

#[test]
fn delegation_dispatch_identity_rejects_a_replaced_or_missing_session() {
    let (_hub, target) = delegation_hub(RuntimeTaskState::Finished);
    let expected = detected_agent("codex", Some(TEST_CODEX_SESSION));
    assert!(target.check_delegation_identity(Some(&expected)).is_ok());
    for actual in [
        None,
        Some(detected_agent("claude", Some(TEST_CODEX_SESSION))),
        Some(detected_agent("codex", Some("replacement"))),
        Some(detected_agent("codex", None)),
    ] {
        assert_eq!(
            target.check_delegation_identity(actual.as_ref()).unwrap_err().code,
            "agent_identity_mismatch"
        );
    }
}

#[test]
fn delegation_rejects_a_worker_still_running_its_initial_prompt() {
    let (hub, target) = delegation_hub(RuntimeTaskState::Finished);
    hub.publish(delegation_snapshot(
        RuntimeTaskState::Finished,
        RuntimeTaskState::Running,
        Some("claude-session-1"),
    ));
    let error = hub.begin_delegation(3, &target).unwrap_err();
    assert_eq!(error.code, "target_busy");
    hub.complete_delegations_for_turn(&codex_turn_done("READY"));
    assert!(hub.take_ready_delegation_callbacks().is_empty());
    hub.publish(delegation_snapshot(
        RuntimeTaskState::Finished,
        RuntimeTaskState::Finished,
        Some("claude-session-1"),
    ));
    assert!(hub.begin_delegation(3, &target).is_ok(), "busy rejection left no pending task");
}

#[test]
fn delegation_registered_but_not_submitted_cannot_consume_an_earlier_completion() {
    let (hub, target) = delegation_hub(RuntimeTaskState::Finished);
    hub.begin_delegation(3, &target).unwrap();
    // A queued previous Stop can arrive while the UI has not processed the
    // delegated prompt yet. Registration alone cannot establish task ownership.
    hub.complete_delegations_for_turn(&codex_turn_done("READY"));
    assert!(hub.take_ready_delegation_callbacks().is_empty());
    assert_eq!(hub.begin_delegation(3, &target).unwrap_err().code, "delegation_in_progress");
}

#[test]
fn delegation_committed_task_returns_done_once_after_ignoring_queued_ready() {
    let (hub, target) = delegation_hub(RuntimeTaskState::Finished);
    let receipt = hub.begin_delegation(3, &target).unwrap();
    hub.complete_delegations_for_turn(&codex_turn_done("READY"));
    assert!(hub.take_ready_delegation_callbacks().is_empty());
    hub.commit_delegation(&receipt.task_id, &target).unwrap();
    hub.complete_delegations_for_turn(&codex_turn_done("DONE"));
    let callbacks = hub.take_ready_delegation_callbacks();
    assert_eq!(callbacks.len(), 1);
    assert!(callbacks[0].prompt.ends_with("worker_output=\"DONE\""));
    hub.complete_delegations_for_turn(&codex_turn_done("DONE"));
    assert!(hub.take_ready_delegation_callbacks().is_empty());
}

#[test]
fn delegation_cancelled_registration_cannot_be_committed_later() {
    let (hub, target) = delegation_hub(RuntimeTaskState::Finished);
    let receipt = hub.begin_delegation(3, &target).unwrap();
    hub.cancel_delegation(&receipt.task_id);
    assert_eq!(
        hub.commit_delegation(&receipt.task_id, &target).unwrap_err().code,
        "delegation_not_pending"
    );
    hub.complete_delegations_for_turn(&codex_turn_done("READY"));
    assert!(hub.take_ready_delegation_callbacks().is_empty());
    assert!(hub.begin_delegation(3, &target).is_ok());
}

#[test]
fn delegation_blocked_or_failed_targets_are_not_queued() {
    for state in
        [RuntimeTaskState::Attention, RuntimeTaskState::WaitingInput, RuntimeTaskState::Failed]
    {
        let (hub, target) = delegation_hub(RuntimeTaskState::Finished);
        hub.publish(delegation_snapshot(
            RuntimeTaskState::Finished,
            state,
            Some("claude-session-1"),
        ));
        assert_eq!(hub.begin_delegation(3, &target).unwrap_err().code, "target_busy");
    }
}

#[test]
fn delegation_launch_receipt_reports_post_submission_state_and_wait_baseline() {
    let hub = RuntimeHub::new();
    hub.publish(snapshot(RuntimeTaskState::Idle));
    let sink_hub = hub.clone();
    let sink = EventSink::Callback(Arc::new(move |callback| {
        let RuntimeCallback::Control(dispatch) = callback else { return };
        match &dispatch.command {
            RuntimeCommand::AgentStart { pane_id: Some(pane_id), name, kind, .. } => {
                let agent = sink_hub
                    .register_agent(
                        name.clone(),
                        *kind,
                        7,
                        *pane_id,
                        Some(TEST_CODEX_SESSION.into()),
                        None,
                    )
                    .unwrap();
                let mut current = snapshot(RuntimeTaskState::Idle);
                current.windows[0].tabs[0].panes[0].agent =
                    Some(detected_agent("codex", Some(TEST_CODEX_SESSION)));
                let current = sink_hub.publish(current);
                dispatch.respond(Ok(json!({"action": {"agent": agent, "window_id": 7, "pane_id": pane_id}, "snapshot": current})));
            },
            RuntimeCommand::AgentPrompt { .. } => {
                let mut current = snapshot(RuntimeTaskState::Running);
                current.windows[0].tabs[0].panes[0].agent =
                    Some(detected_agent("codex", Some(TEST_CODEX_SESSION)));
                let current = sink_hub.publish(current);
                dispatch.respond(Ok(json!({"action": {}, "snapshot": current})));
            },
            RuntimeCommand::ReadPane { .. } => {
                dispatch.respond(Ok(json!({"action": {"text": "", "returned_lines": 0}})));
            },
            other => panic!("unexpected command {other:?}"),
        }
    }));
    let receipt = super::orchestrate::execute_for_test(&json!({
        "steps": [
            {"id": "worker", "op": "agent_launch", "target": {"window_id": 7, "pane_id": 3},
             "name": "worker", "kind": "codex", "initial_prompt": "initial task"},
            {"id": "wait_first_turn", "op": "wait", "target": {"step": "worker", "field": "pane_id"},
             "state": "settled", "timeout_ms": 1}
        ]
    }), &sink, &hub).unwrap();
    let action = &receipt["steps"][0]["action"];
    assert_eq!(action["ready_state"], "idle");
    assert_eq!(action["ready_state_scope"], "before_initial_prompt");
    assert_eq!(action["state_after_submission"], "running");
    assert!(action["state_change_seq"].as_u64().unwrap() > 1);
    assert_eq!(receipt["steps"][1]["error"]["code"], "timeout");
}
