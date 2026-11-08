use super::*;

#[test]
fn delegation_exit_finishes_a_submitted_task_once_and_releases_pending() {
    let (hub, target) = delegation_hub(RuntimeTaskState::Finished);
    let receipt = submitted_delegation(&hub, 3, &target);
    hub.record_pane_exited(target.window_id, target.pane_id);
    let callbacks = hub.take_ready_delegation_callbacks();
    assert_eq!(callbacks.len(), 1, "worker exit must notify the original caller");
    assert_eq!(callbacks[0].task_id, receipt.task_id);
    assert!(callbacks[0].prompt.contains("status=failed"));
    assert!(callbacks[0].prompt.contains("reason=pane_exited"));
    assert!(callbacks[0].prompt.contains("output_missing=true"));
    assert!(callbacks[0].prompt.ends_with("worker_output=null"));
    assert!(hub.lock().pending_delegations.is_empty());
    hub.record_pane_closed(target.window_id, target.pane_id);
    hub.complete_delegations_for_turn(&codex_turn_done("late DONE"));
    assert!(hub.take_ready_delegation_callbacks().is_empty());
}

fn call_agent_wait(
    hub: &RuntimeHub,
    target: &RuntimeManagedAgent,
    after_seq: Option<u64>,
) -> Value {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    let request = ApiRequest::new(
        "token".into(),
        "agent.wait",
        json!({
            "agent": target.agent_id, "generation": target.generation,
            "state": "settled", "timeout_ms": 1, "after_seq": after_seq,
        }),
    );
    crate::runtime_api::agent_api::agent_wait_connection(&mut server, request, hub).unwrap();
    let mut line = String::new();
    BufReader::new(&mut client).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

#[test]
fn delegation_wait_exposes_the_matched_state_and_sequence_without_losing_the_snapshot() {
    let (hub, target) = delegation_hub(RuntimeTaskState::Finished);
    let response = call_agent_wait(&hub, &target, None);
    assert_eq!(response["ok"], true);
    assert_eq!(response["result"]["task_state"], "finished");
    let current = hub.current().unwrap();
    let pane = current.pane(Some(target.window_id), target.pane_id).unwrap();
    assert_eq!(response["result"]["state_change_seq"], pane.state_change_seq);
    assert_eq!(response["result"]["agent"]["agent_id"], target.agent_id);
    assert_eq!(response["result"]["snapshot"], serde_json::to_value(&current).unwrap());
    let unchanged = call_agent_wait(&hub, &target, Some(pane.state_change_seq));
    assert_eq!(unchanged["ok"], false, "the convenience fields cannot bypass after_seq");
    assert_eq!(unchanged["error"]["code"], "timeout");
}

#[test]
fn delegation_exit_reconciles_pane_removal_process_exit_and_replacement_snapshots() {
    for reason in ["pane_closed", "agent_exited", "agent_replaced"] {
        let (hub, target) = delegation_hub(RuntimeTaskState::Finished);
        submitted_delegation(&hub, 3, &target);
        let mut next = delegation_snapshot(
            RuntimeTaskState::Finished,
            RuntimeTaskState::Running,
            Some("claude-session-1"),
        );
        match reason {
            "pane_closed" => {
                next.windows[0].tabs.pop();
            },
            "agent_exited" => {
                let pane = &mut next.windows[0].tabs[1].panes[0];
                pane.agent = None;
                pane.running_program = None;
            },
            "agent_replaced" => {
                next.windows[0].tabs[1].panes[0].agent =
                    Some(detected_agent("codex", Some("replacement")));
            },
            _ => unreachable!(),
        }
        hub.publish(next.clone());
        hub.publish(next);
        let callbacks = hub.take_ready_delegation_callbacks();
        assert_eq!(callbacks.len(), 1, "{reason}");
        assert!(callbacks[0].prompt.contains(&format!("reason={reason}")));
        assert!(hub.lock().pending_delegations.is_empty());
        assert_eq!(
            hub.active_agent(&target.agent_id, Some(target.generation)).unwrap_err().code,
            reason
        );
    }
}

#[test]
fn delegation_exit_does_not_notify_an_unsubmitted_or_cancelled_registration() {
    for cancelled in [false, true] {
        let (hub, target) = delegation_hub(RuntimeTaskState::Finished);
        let receipt = hub.begin_delegation(3, &target).unwrap();
        if cancelled {
            hub.cancel_delegation(&receipt.task_id);
        }
        hub.record_pane_closed(target.window_id, target.pane_id);
        assert!(hub.take_ready_delegation_callbacks().is_empty());
        assert!(hub.lock().pending_delegations.is_empty());
        assert_eq!(
            hub.commit_delegation(&receipt.task_id, &target).unwrap_err().code,
            "delegation_not_pending"
        );
    }
}

#[test]
fn delegation_exit_waits_for_the_original_caller_and_retries_the_same_result() {
    let (hub, target) = delegation_hub(RuntimeTaskState::Running);
    let receipt = submitted_delegation(&hub, 3, &target);
    hub.close_agent(&target.agent_id, "agent_exited");
    assert!(hub.lock().pending_delegations.is_empty());
    assert!(hub.has_pending_delegation_callbacks());
    assert!(hub.take_ready_delegation_callbacks().is_empty());
    hub.publish(delegation_snapshot(
        RuntimeTaskState::Finished,
        RuntimeTaskState::Running,
        Some("claude-session-1"),
    ));
    let callbacks = hub.take_ready_delegation_callbacks();
    assert_eq!(callbacks.len(), 1);
    let callback = callbacks[0].clone();
    assert_eq!(callback.task_id, receipt.task_id);
    assert!(callback.prompt.contains("reason=agent_exited"));
    hub.requeue_delegation_callback(callback.clone());
    hub.requeue_delegation_callback(callback.clone());
    hub.record_pane_closed(target.window_id, target.pane_id);
    let retry = hub.take_ready_delegation_callbacks();
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].prompt, callback.prompt, "duration and result are immutable on retry");
}

#[test]
fn delegation_exit_does_not_replace_an_already_accepted_final_result() {
    let (hub, target) = delegation_hub(RuntimeTaskState::Finished);
    submitted_delegation(&hub, 3, &target);
    hub.complete_delegations_for_turn(&codex_turn_done("DONE"));
    hub.record_pane_exited(target.window_id, target.pane_id);
    let callbacks = hub.take_ready_delegation_callbacks();
    assert_eq!(callbacks.len(), 1);
    assert!(callbacks[0].prompt.contains("status=completed"));
    assert!(callbacks[0].prompt.ends_with("worker_output=\"DONE\""));
    assert!(!callbacks[0].prompt.contains("reason=pane_exited"));
}

#[test]
fn delegation_exit_releases_ownership_if_the_original_caller_disappears() {
    let (hub, target) = delegation_hub(RuntimeTaskState::Finished);
    submitted_delegation(&hub, 3, &target);
    hub.publish(delegation_snapshot(
        RuntimeTaskState::Finished,
        RuntimeTaskState::Running,
        Some("replacement-origin"),
    ));
    assert!(hub.lock().pending_delegations.is_empty());
    hub.record_pane_closed(target.window_id, target.pane_id);
    assert!(hub.take_ready_delegation_callbacks().is_empty());
}

#[test]
fn delegation_exit_does_not_deliver_to_a_registry_closed_caller_in_a_stale_snapshot() {
    for completed in [false, true] {
        let (hub, target) = delegation_hub(RuntimeTaskState::Finished);
        let caller = hub
            .register_agent(
                "caller".into(),
                crate::ai_agents::AgentKind::Claude,
                7,
                3,
                Some("claude-session-1".into()),
                None,
            )
            .unwrap();
        hub.publish(delegation_snapshot(
            RuntimeTaskState::Finished,
            RuntimeTaskState::Finished,
            Some("claude-session-1"),
        ));
        submitted_delegation(&hub, 3, &target);
        if completed {
            hub.complete_delegations_for_turn(&codex_turn_done("DONE"));
        }
        hub.close_agent(&caller.agent_id, "agent_exited");
        hub.record_pane_exited(target.window_id, target.pane_id);
        assert!(hub.lock().pending_delegations.is_empty());
        assert!(hub.take_ready_delegation_callbacks().is_empty());
    }
}

#[test]
fn delegation_exit_does_not_confuse_a_live_window_transfer_with_worker_closure() {
    let (hub, target) = delegation_hub(RuntimeTaskState::Finished);
    submitted_delegation(&hub, 3, &target);
    let mut next = delegation_snapshot(
        RuntimeTaskState::Finished,
        RuntimeTaskState::Running,
        Some("claude-session-1"),
    );
    let worker_tab = next.windows[0].tabs.pop().unwrap();
    let mut new_window = next.windows[0].clone();
    new_window.id = 9;
    new_window.tabs = vec![worker_tab];
    new_window.focused_pane_id = Some(target.pane_id);
    next.windows.push(new_window);
    hub.move_panes_to_window(7, 9, &[target.pane_id]);
    hub.publish(next);
    assert!(hub.take_ready_delegation_callbacks().is_empty());
    assert_eq!(hub.lock().pending_delegations.len(), 1);
    hub.record_pane_exited(9, target.pane_id);
    let callbacks = hub.take_ready_delegation_callbacks();
    assert_eq!(callbacks.len(), 1);
    assert!(callbacks[0].prompt.contains("reason=pane_exited"));
}
