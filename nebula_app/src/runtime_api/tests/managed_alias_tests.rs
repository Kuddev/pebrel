use super::*;
use crate::ai_agents::AgentKind;
use crate::session::AgentSession;

fn saved(name: &str) -> AgentSession {
    AgentSession {
        source: "codex".into(),
        session_id: Some(TEST_CODEX_SESSION.into()),
        session_file: None,
        managed_name: Some(name.into()),
    }
}

fn live(id: Option<&str>) -> RuntimeSnapshot {
    let mut current = snapshot(RuntimeTaskState::Idle);
    current.windows[0].tabs[0].panes[0].agent = Some(detected_agent("codex", id));
    current
}

#[test]
fn managed_alias_schema_remains_optional_and_does_not_enter_resume_commands() {
    let old: AgentSession = serde_json::from_value(json!({
        "source":"codex", "session_id":TEST_CODEX_SESSION
    }))
    .unwrap();
    assert_eq!(old.managed_name, None);
    assert!(serde_json::to_value(&old).unwrap().get("managed_name").is_none());
    let saved = saved("worker");
    let decoded: AgentSession =
        serde_json::from_value(serde_json::to_value(&saved).unwrap()).unwrap();
    assert_eq!(decoded, saved);
    assert_eq!(old.resume_command(), saved.resume_command());
}

#[test]
fn managed_alias_restores_only_after_exact_live_native_identity() {
    let hub = RuntimeHub::new();
    let saved = saved("worker");
    hub.restore_managed_alias(7, 3, &saved);
    assert_eq!(hub.saved_managed_name(7, 3, &saved).as_deref(), Some("worker"));
    hub.publish(snapshot(RuntimeTaskState::Idle));
    hub.publish(live(None));
    assert_eq!(hub.active_agent("worker", None).unwrap_err().code, "agent_not_found");
    let published = hub.publish(live(Some(TEST_CODEX_SESSION)));
    let registered = hub.active_agent("worker", None).unwrap();
    assert_eq!(
        published.windows[0].tabs[0].panes[0].agent.as_ref().unwrap().name.as_deref(),
        Some("worker")
    );
    assert_eq!(registered.session_id, saved.session_id);
    assert!(registered.worktree.is_none());
    hub.publish(live(Some(TEST_CODEX_SESSION)));
    assert_eq!(hub.active_agent("worker", None).unwrap().agent_id, registered.agent_id);
    assert!(hub.lock().pending_alias_restores.is_empty());
}

#[test]
fn managed_alias_rejects_mismatch_close_and_missing_native_id() {
    for mode in ["different_id", "different_provider", "closed", "invalid_name", "no_saved_id"] {
        let hub = RuntimeHub::new();
        let mut target = saved("worker");
        if mode == "invalid_name" {
            target.managed_name = Some("bad name\n".into());
        }
        if mode == "no_saved_id" {
            target.session_id = None;
        }
        hub.restore_managed_alias(7, 3, &target);
        let mut current = live(Some(TEST_CODEX_SESSION));
        if mode == "different_id" {
            current.windows[0].tabs[0].panes[0].agent.as_mut().unwrap().session_id =
                Some("replacement".into());
        }
        if mode == "different_provider" {
            current.windows[0].tabs[0].panes[0].agent =
                Some(detected_agent("claude", Some(TEST_CODEX_SESSION)));
        }
        if mode == "closed" {
            hub.record_pane_closed(7, 3);
        }
        hub.publish(current);
        assert!(hub.lock().managed_agents.is_empty(), "{mode}");
        assert!(hub.lock().pending_alias_restores.is_empty(), "{mode}");
    }
}

#[test]
fn managed_alias_reservation_survives_other_window_startup_and_moves_with_its_pane() {
    let hub = RuntimeHub::new();
    let saved = saved("worker");
    hub.restore_managed_alias(8, 3, &saved);
    hub.publish(snapshot(RuntimeTaskState::Idle));
    assert_eq!(hub.saved_managed_name(8, 3, &saved).as_deref(), Some("worker"));
    assert_eq!(hub.saved_managed_name(7, 3, &saved), None);
    hub.move_panes_to_window(8, 9, &[3]);
    let mut current = live(Some(TEST_CODEX_SESSION));
    current.windows[0].id = 9;
    hub.publish(current);
    assert_eq!(hub.active_agent("worker", None).unwrap().window_id, 9);
}

#[test]
fn managed_alias_cannot_steal_an_active_name() {
    let hub = RuntimeHub::new();
    let owner = hub
        .register_agent("worker".into(), AgentKind::Codex, 7, 4, Some("other".into()), None)
        .unwrap();
    hub.restore_managed_alias(7, 3, &saved("worker"));
    let mut current = live(Some(TEST_CODEX_SESSION));
    let mut other = current.windows[0].tabs[0].panes[0].clone();
    other.id = 4;
    other.agent = Some(detected_agent("codex", Some("other")));
    current.windows[0].tabs[0].panes.push(other);
    hub.publish(current);
    assert_eq!(hub.active_agent("worker", None).unwrap().agent_id, owner.agent_id);
    assert!(hub.lock().pending_alias_restores.is_empty());
    assert_eq!(hub.saved_managed_name(7, 3, &saved("worker")), None);
}

#[test]
fn managed_alias_restart_gets_fresh_identity_and_no_old_tasks() {
    let old = RuntimeHub::with_generation_namespace_for_test(1000);
    old.restore_managed_alias(7, 3, &saved("worker"));
    old.publish(live(Some(TEST_CODEX_SESSION)));
    let previous = old.active_agent("worker", None).unwrap();
    let restored = RuntimeHub::with_generation_namespace_for_test(2000);
    restored.restore_managed_alias(7, 3, &saved("worker"));
    restored.publish(live(Some(TEST_CODEX_SESSION)));
    let current = restored.active_agent("worker", None).unwrap();
    assert_ne!(previous.agent_id, current.agent_id);
    assert_ne!(previous.generation, current.generation);
    assert_eq!(
        restored.active_agent("worker", Some(previous.generation)).unwrap_err().code,
        "agent_identity_mismatch"
    );
    assert_eq!(
        restored.active_agent(&previous.agent_id, Some(previous.generation)).unwrap_err().code,
        "agent_not_found"
    );
    assert!(restored.lock().pending_delegations.is_empty());
    assert!(restored.take_ready_delegation_callbacks().is_empty());
}

#[test]
fn managed_alias_attach_is_exact_idempotent_and_refuses_conflicts() {
    let hub = RuntimeHub::new();
    let actual = detected_agent("codex", Some(TEST_CODEX_SESSION));
    for (kind, id) in [(AgentKind::Claude, TEST_CODEX_SESSION), (AgentKind::Codex, "stale")] {
        assert_eq!(
            hub.attach_agent("worker".into(), kind, 7, 3, id, &actual).unwrap_err().code,
            "agent_identity_mismatch"
        );
    }
    assert!(hub.lock().managed_agents.is_empty());
    let attached = hub
        .attach_agent("worker".into(), AgentKind::Codex, 7, 3, TEST_CODEX_SESSION, &actual)
        .unwrap();
    assert_eq!(
        hub.attach_agent("worker".into(), AgentKind::Codex, 7, 3, TEST_CODEX_SESSION, &actual)
            .unwrap()
            .agent_id,
        attached.agent_id
    );
    assert_eq!(
        hub.attach_agent("other".into(), AgentKind::Codex, 7, 3, TEST_CODEX_SESSION, &actual)
            .unwrap_err()
            .code,
        "agent_pane_conflict"
    );
    assert_eq!(
        hub.attach_agent("worker".into(), AgentKind::Codex, 7, 4, TEST_CODEX_SESSION, &actual)
            .unwrap_err()
            .code,
        "agent_name_conflict"
    );
    assert_eq!(hub.lock().managed_agents.len(), 1);
}

#[test]
fn managed_alias_explicit_attach_after_new_invalidates_old_delegation_once() {
    let (hub, old) = delegation_hub(RuntimeTaskState::Finished);
    submitted_delegation(&hub, 3, &old);
    let actual = detected_agent("codex", Some("new-conversation"));
    let attached = hub
        .attach_agent("worker".into(), AgentKind::Codex, 7, 4, "new-conversation", &actual)
        .unwrap();
    assert_eq!(attached.generation, old.generation + 1);
    assert_eq!(
        hub.active_agent(&old.agent_id, Some(old.generation)).unwrap_err().code,
        "agent_replaced"
    );
    let mut current = delegation_snapshot(
        RuntimeTaskState::Finished,
        RuntimeTaskState::Idle,
        Some("claude-session-1"),
    );
    current.windows[0]
        .tabs
        .iter_mut()
        .flat_map(|tab| &mut tab.panes)
        .find(|pane| pane.id == 4)
        .unwrap()
        .agent = Some(actual);
    hub.publish(current);
    let callbacks = hub.take_ready_delegation_callbacks();
    assert_eq!(callbacks.len(), 1);
    assert!(callbacks[0].prompt.contains("reason=agent_replaced"));
    hub.complete_delegations_for_turn(&codex_turn_done("OLD DONE"));
    assert!(hub.take_ready_delegation_callbacks().is_empty());
}

#[test]
fn managed_alias_attach_parser_requires_explicit_expected_identity() {
    let request = |extra: Value| {
        let mut params = json!({"pane_id":3,"name":"worker","kind":"codex"});
        params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        ApiRequest::new("token".into(), "agent.attach", params)
    };
    assert!(RuntimeCommand::from_request(&request(json!({}))).is_err());
    assert!(RuntimeCommand::from_request(&request(json!({"expected_session_id":""}))).is_err());
    assert!(RuntimeCommand::from_request(&request(json!({"expected_session_id":"a\nb"}))).is_err());
    assert!(
        RuntimeCommand::from_request(&request(
            json!({"expected_session_id":TEST_CODEX_SESSION, "cwd":"/wrong"})
        ))
        .is_err()
    );
    assert!(matches!(
        RuntimeCommand::from_request(&request(json!({"expected_session_id":TEST_CODEX_SESSION})))
            .unwrap(),
        RuntimeCommand::AgentAttach { pane_id: 3, .. }
    ));
}
