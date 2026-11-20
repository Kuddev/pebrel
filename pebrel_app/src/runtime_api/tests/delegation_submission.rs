use super::*;

pub(super) fn submitted_delegation(
    hub: &RuntimeHub,
    origin: u64,
    target: &RuntimeManagedAgent,
) -> RuntimeDelegationReceipt {
    let receipt = hub.begin_delegation(origin, target).unwrap();
    let actual = detected_agent(&target.kind, target.session_id.as_deref());
    hub.submit_delegation(
        &receipt.task_id,
        target,
        Some(&actual),
        RuntimeTaskState::Finished,
        || Ok(()),
    )
    .unwrap();
    receipt
}

#[test]
fn only_completions_received_after_submission_own_the_delegated_task() {
    let (hub, target) = delegation_hub(RuntimeTaskState::Finished);
    let receipt = hub.begin_delegation(3, &target).unwrap();
    let before_submission = codex_turn_done("old turn");
    hub.complete_delegations_for_turn(&before_submission);
    assert!(hub.take_ready_delegation_callbacks().is_empty());

    let actual = detected_agent("codex", Some(TEST_CODEX_SESSION));
    hub.submit_delegation(
        &receipt.task_id,
        &target,
        Some(&actual),
        RuntimeTaskState::Finished,
        || Ok(()),
    )
    .unwrap();
    // A timeout in the transport thread can race an accepted UI write.
    hub.cancel_unsubmitted_delegation(&receipt.task_id);
    hub.complete_delegations_for_turn(&before_submission);
    assert!(hub.take_ready_delegation_callbacks().is_empty());
    hub.complete_delegations_for_turn(&codex_turn_done("delegated answer"));
    let callbacks = hub.take_ready_delegation_callbacks();
    assert_eq!(callbacks.len(), 1);
    assert_eq!(callbacks[0].task_id, receipt.task_id);
    assert!(callbacks[0].prompt.ends_with("worker_output=\"delegated answer\""));
}

#[test]
fn delegation_checks_worker_state_at_registration_and_at_the_write() {
    let (hub, target) = delegation_hub(RuntimeTaskState::Finished);
    let actual = detected_agent("codex", Some(TEST_CODEX_SESSION));
    for state in [
        RuntimeTaskState::Running,
        RuntimeTaskState::WaitingInput,
        RuntimeTaskState::Attention,
        RuntimeTaskState::Failed,
    ] {
        hub.publish(delegation_snapshot(
            RuntimeTaskState::Finished,
            state,
            Some("claude-session-1"),
        ));
        assert_eq!(hub.begin_delegation(3, &target).unwrap_err().code, "target_busy");
    }
    hub.publish(delegation_snapshot(
        RuntimeTaskState::Finished,
        RuntimeTaskState::Finished,
        Some("claude-session-1"),
    ));
    let receipt = hub.begin_delegation(3, &target).unwrap();
    let submit = |state, agent: Option<&RuntimeAgent>| {
        hub.submit_delegation(&receipt.task_id, &target, agent, state, || {
            panic!("unexpected write")
        })
    };
    assert_eq!(submit(RuntimeTaskState::Running, Some(&actual)).unwrap_err().code, "target_busy");
    let replaced = detected_agent("codex", Some("another-session"));
    assert_eq!(
        submit(RuntimeTaskState::Finished, Some(&replaced)).unwrap_err().code,
        "agent_identity_mismatch"
    );
    assert_eq!(
        submit(RuntimeTaskState::Finished, None).unwrap_err().code,
        "agent_identity_mismatch"
    );
    hub.cancel_unsubmitted_delegation(&receipt.task_id);
    assert_eq!(
        submit(RuntimeTaskState::Finished, Some(&actual)).unwrap_err().code,
        "delegation_not_pending"
    );
}

#[test]
fn failed_delegation_input_releases_the_worker() {
    let (hub, target) = delegation_hub(RuntimeTaskState::Finished);
    let actual = detected_agent("codex", Some(TEST_CODEX_SESSION));
    let receipt = hub.begin_delegation(3, &target).unwrap();
    let error = hub
        .submit_delegation(
            &receipt.task_id,
            &target,
            Some(&actual),
            RuntimeTaskState::Finished,
            || Err(ApiError::new("input_in_progress", "pending input")),
        )
        .unwrap_err();
    assert_eq!(error.code, "input_in_progress");
    assert!(hub.begin_delegation(3, &target).is_ok());
    hub.complete_delegations_for_turn(&codex_turn_done("prior task"));
    assert!(hub.take_ready_delegation_callbacks().is_empty());
}
