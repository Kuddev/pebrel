use super::*;

#[test]
fn non_successful_turns_do_not_emit_a_completion_notification() {
    for reason in ["error", "aborted", "length", "toolUse"] {
        let payload = serde_json::json!({"kind":"done", "stop_reason":reason});
        let event = parse_remote_envelope(
            format!("nebula-hook/1 source=pi\n{payload}").as_bytes(),
            Some(1),
        )
        .unwrap();
        let notification = crate::notify::Notification::from_ai_hook(&event, None, false);
        if reason == "aborted" {
            assert!(notification.is_none());
        } else {
            let notification = notification.expect("failed turns report an issue");
            assert!(notification.is_failure());
            assert!(!notification.is_attention());
            assert!(!matches!(notification, crate::notify::Notification::AiTurn { .. }));
        }
    }
    let event = parse_remote_envelope(
        b"nebula-hook/1 source=pi\n{\"kind\":\"done\",\"stop_reason\":\"stop\"}",
        Some(1),
    )
    .unwrap();
    assert!(crate::notify::Notification::from_ai_hook(&event, None, false,).is_some());
}
#[test]
fn pi_result_metadata_distinguishes_legacy_unknown_and_background_work() {
    use crate::ai_agents::AgentStatus;
    use crate::notify::Notification;
    for (metadata, outcome, status, notifies) in [
        ("", AiTurnOutcome::Unspecified, AgentStatus::Done, true),
        (",\"stop_reason\":\"stop\"", AiTurnOutcome::Succeeded, AgentStatus::Done, true),
        (",\"stop_reason\":\"error\"", AiTurnOutcome::Failed, AgentStatus::Idle, true),
        (",\"stop_reason\":\"aborted\"", AiTurnOutcome::Cancelled, AgentStatus::Idle, false),
        (",\"stop_reason\":\"length\"", AiTurnOutcome::Incomplete, AgentStatus::Idle, true),
        (",\"stop_reason\":\"toolUse\"", AiTurnOutcome::Incomplete, AgentStatus::Idle, true),
        (",\"stop_reason\":\"future\"", AiTurnOutcome::Unknown, AgentStatus::Idle, true),
        (",\"stop_reason\":null", AiTurnOutcome::Unknown, AgentStatus::Idle, true),
        (",\"stop_reason\":42", AiTurnOutcome::Unknown, AgentStatus::Idle, true),
    ] {
        let wire = format!("nebula-hook/1 source=pi\n{{\"kind\":\"done\"{metadata}}}");
        let mut event = parse_remote_envelope(wire.as_bytes(), Some(1)).unwrap();
        assert_eq!(event.turn_outcome, outcome);
        let mut activity = lifecycle::AgentActivity::default();
        assert!(activity.apply_hook(&event));
        assert_eq!(activity.status(), status);
        assert_eq!(Notification::from_ai_hook(&event, None, false).is_some(), notifies);
        event.background_tasks = Some(AiBackgroundTasks { active: 1, total: 1 });
        assert!(activity.apply_hook(&event));
        assert_eq!(activity.status(), AgentStatus::Working);
        assert!(Notification::from_ai_hook(&event, None, false).is_none());
    }
}

use crate::ai_agents::{AgentStatus, AgentStatusSource};
use lifecycle::AgentActivity;
use serde_json::{Value, json};

fn codex(name: &str, session: &str, turn: Option<&str>) -> AiHookEvent {
    native("full", None, json!({"hook_event_name":name,"session_id":session,"turn_id":turn}))
}

fn native(mode: &str, process: Option<&str>, payload: Value) -> AiHookEvent {
    let process = process.map(|key| format!(" process={key}")).unwrap_or_default();
    let wire = format!("nebula-hook/1 source=codex codex_hooks={mode}{process}\n{payload}");
    parse_remote_envelope(wire.as_bytes(), Some(90)).unwrap()
}

#[test]
fn native_codex_owns_start_permission_recovery_interrupt_and_end() {
    let mut activity = AgentActivity::default();
    for (event, expected) in [
        ("SessionStart", AgentStatus::Idle),
        ("UserPromptSubmit", AgentStatus::Working),
        ("PermissionRequest", AgentStatus::Blocked),
        ("PostToolUse", AgentStatus::Working),
        ("Interrupt", AgentStatus::Idle),
    ] {
        let hook = codex(event, "native", Some("turn1"));
        assert!(hook.capabilities().lifecycle && hook.capabilities().attention_events);
        assert!(activity.apply_hook(&hook));
        assert_eq!(activity.status(), expected);
        assert_eq!(activity.source(), AgentStatusSource::Hook);
        assert!(!activity.allows_screen());
        if event == "Interrupt" {
            assert_eq!(hook.turn_outcome, AiTurnOutcome::Cancelled);
        }
    }
    assert!(activity.apply_hook(&codex("SessionEnd", "native", None)));
    assert!(!activity.hook_seen());
    assert!(!activity.apply_hook(&codex("PostToolUse", "native", Some("turn1"))));
}

#[test]
fn native_and_notify_coexist_without_duplicate_completion_or_capability_downgrade() {
    let notify = parse_remote_envelope(b"nebula-hook/1 source=codex\n{\"type\":\"agent-turn-complete\",\"thread-id\":\"native\",\"turn-id\":\"turn1\"}", Some(90)).unwrap();
    let done = codex("Stop", "native", Some("turn1"));
    assert_eq!(notify.event_id, done.event_id);
    let mut activity = AgentActivity::default();
    activity.apply_hook(&codex("UserPromptSubmit", "native", Some("turn1")));
    assert!(!activity.apply_hook(&notify));
    assert!(activity.apply_hook(&done));
    assert!(!activity.apply_hook(&notify));
    activity.apply_hook(&codex("UserPromptSubmit", "native", Some("turn2")));
    assert!(!activity.apply_hook(&done), "old turn cannot finish new work");
    assert_eq!(activity.status(), AgentStatus::Working);
    let mut reversed = AgentActivity::default();
    assert!(reversed.apply_hook(&notify));
    assert!(
        !reversed.apply_hook(&done),
        "same turn completed only once even if notify won the race"
    );
}

#[test]
fn older_native_contract_and_notify_retain_their_actual_capabilities() {
    let hook = native("turns", None, json!({"hook_event_name":"SessionStart","session_id":"old"}));
    assert!(hook.capabilities().lifecycle);
    assert!(!hook.capabilities().attention_events);
    assert!(!capabilities_for("codex").lifecycle);
    for name in ["SubagentStart", "SubagentStop", "invented"] {
        let wire = format!(
            "nebula-hook/1 source=codex codex_hooks=full\n{{\"hook_event_name\":\"{name}\",\"session_id\":\"main\"}}"
        );
        assert!(parse_remote_envelope(wire.as_bytes(), Some(90)).is_none());
    }
}

#[test]
fn authenticated_remote_process_can_clear_its_session_without_accepting_a_child_or_late_start() {
    let start = |session: &str, process: &str, sequence: u64| {
        native(
            "full",
            Some(process),
            json!({
                "hook_event_name":"SessionStart", "session_id":session, "source":"clear", "bridge_sequence":sequence,
            }),
        )
    };
    let mut activity = AgentActivity::default();
    assert!(activity.apply_hook(&start("before", "100:55", 1)));
    assert!(!activity.apply_hook(&start("child", "200:66", 2)));
    assert!(activity.apply_hook(&start("after", "100:55", 3)));
    assert!(!activity.apply_hook(&start("before", "100:55", 1)));
    let stopped = native(
        "full",
        Some("100:55"),
        json!({"hook_event_name":"Stop", "session_id":"before", "bridge_sequence":4}),
    );
    assert!(!activity.apply_hook(&stopped));
    assert_eq!(activity.primary_pid(), None, "remote process must never become a host PID");
    let local = protocol::parse_envelope(b"nebula-hook/1 source=codex codex_hooks=full process=100:55\n{\"hook_event_name\":\"SessionStart\",\"session_id\":\"local\"}").unwrap();
    assert!(local.remote_process.is_none());
}

#[test]
fn normal_tool_hooks_inside_a_codex_subagent_do_not_change_the_primary_turn() {
    for name in ["PermissionRequest", "PostToolUse", "Stop", "UserPromptSubmit"] {
        let payload = json!({"hook_event_name":name,"session_id":"main","turn_id":"main-turn","agent_id":"child","agent_type":"worker"});
        let wire = format!("nebula-hook/1 source=codex codex_hooks=full\n{payload}");
        assert!(parse_remote_envelope(wire.as_bytes(), Some(1)).is_none());
    }
}

#[test]
fn codex_user_questions_are_structured_attention_and_prose_in_tools_is_not() {
    let groups = installation::codex_groups("helper", None, CodexHookMode::Full);
    assert_eq!(groups["PreToolUse"][0]["matcher"], "^request_user_input$");
    assert!(
        installation::codex_groups("helper", None, CodexHookMode::Turns)
            .get("PreToolUse")
            .is_none()
    );
    let payload = json!({"hook_event_name":"PreToolUse", "session_id":"main", "turn_id":"turn", "tool_name":"request_user_input", "tool_input":{"questions":[{"id":"scope","question":"Which scope should be used?","options":[{"label":"Current project"},{"label":"All projects"}]}]}});
    let event = native("full", None, payload.clone());
    let mut activity = AgentActivity::default();
    activity.apply_hook(&codex("UserPromptSubmit", "main", Some("turn")));
    assert!(activity.apply_hook(&event));
    assert_eq!(activity.status(), AgentStatus::Blocked);
    assert_eq!(event.message.as_deref(), Some("Which scope should be used?"));
    assert!(!activity.allows_screen());
    assert!(activity.apply_hook(&codex("PostToolUse", "main", Some("turn"))));
    assert_eq!(activity.status(), AgentStatus::Working);
    for (mode, body) in [
        ("turns", payload),
        (
            "full",
            json!({"hook_event_name":"PreToolUse", "tool_name":"Bash", "tool_input":{"command":"echo '[y/n]'"}}),
        ),
        (
            "full",
            json!({"hook_event_name":"PreToolUse", "tool_name":"request_user_input", "tool_input":{"questions":[]}}),
        ),
    ] {
        let wire = format!("nebula-hook/1 source=codex codex_hooks={mode}\n{body}");
        assert!(parse_remote_envelope(wire.as_bytes(), Some(1)).is_none());
    }
}

#[test]
fn compaction_does_not_mark_an_active_turn_idle_and_native_stop_supplies_the_answer() {
    let mut activity = AgentActivity::default();
    activity.apply_hook(&codex("UserPromptSubmit", "main", Some("turn")));
    let compacted = native(
        "full",
        None,
        json!({"hook_event_name":"SessionStart","session_id":"main","source":"compact"}),
    );
    activity.apply_hook(&compacted);
    assert_eq!(activity.status(), AgentStatus::Working);
    let done = native(
        "full",
        None,
        json!({"hook_event_name":"Stop","session_id":"main","turn_id":"turn","last_assistant_message":"complete answer"}),
    );
    assert_eq!(done.answer.unwrap().source().unwrap().as_ref(), "complete answer");
}

#[test]
fn kimi_failure_and_interrupt_do_not_announce_success() {
    for (event_name, expected) in [
        ("Stop", AiTurnOutcome::Succeeded),
        ("StopFailure", AiTurnOutcome::Failed),
        ("Interrupt", AiTurnOutcome::Cancelled),
    ] {
        let raw = format!(
            "nebula-hook/1 source=kimi pane=3\n{}",
            json!({"hook_event_name":event_name, "session_id":"kimi-1", "error":"rate_limit"})
        );
        let event = parse_remote_envelope(raw.as_bytes(), Some(3)).unwrap();
        assert_eq!(event.turn_outcome, expected);
        let notification =
            crate::notify::Notification::from_ai_hook(&event, event.message.clone(), false);
        match expected {
            AiTurnOutcome::Succeeded => assert!(!notification.unwrap().is_failure()),
            AiTurnOutcome::Failed => {
                assert!(notification.unwrap().is_failure());
                assert_eq!(event.message.as_deref(), Some("rate_limit"));
            },
            AiTurnOutcome::Cancelled => assert!(notification.is_none()),
            _ => unreachable!(),
        }
    }
}

#[test]
fn queued_input_and_compaction_keep_native_turn_running() {
    let screens = [
        "• Working (12s · esc to interrupt)\n• Messages to be submitted after next tool call (press esc to interrupt and send immediately)\n  ↳ Continue the task\n› Ask Codex to do anything\ngpt-6 max · /project",
        "• Compacting context (1m 41s · esc to interrupt)\n  └ Making room to continue.\n› Ask Codex to do anything\ngpt-6 max · /project",
        "› Ask Codex to do anything\ngpt-6 max · /project",
    ];
    let mut activity = AgentActivity::default();
    activity.apply_hook(&codex("UserPromptSubmit", "main", Some("turn")));
    activity.input_sent();
    for screen in screens {
        for _ in 0..8 {
            assert!(!activity.observe_screen(crate::ai_agents::detect("codex", screen)));
            assert_eq!(activity.status(), AgentStatus::Working);
        }
    }
    let compact = native(
        "full",
        None,
        json!({
            "hook_event_name":"SessionStart", "session_id":"main", "source":"compact"
        }),
    );
    activity.apply_hook(&compact);
    assert_eq!(activity.status(), AgentStatus::Working);
    activity.apply_hook(&codex("Stop", "main", Some("turn")));
    assert_eq!(activity.status(), AgentStatus::Done);
}

#[test]
fn claude_question_tool_failure_and_terminal_failure_have_distinct_effects() {
    let mut activity = AgentActivity::default();
    let parse = |payload: Value| {
        parse_remote_envelope(format!("nebula-hook/1 source=claude\n{payload}").as_bytes(), Some(1))
            .unwrap()
    };
    let question = parse(json!({"hook_event_name":"PreToolUse", "tool_name":"AskUserQuestion",
        "tool_input":{"questions":[{"question":"Choose a scope","options":[{"label":"Current"}]}]}}));
    assert_eq!(question.kind, AiHookKind::NeedsAttention);
    activity.apply_hook(&question);
    assert_eq!(activity.status(), AgentStatus::Blocked);
    let tool_failure = parse(
        json!({"hook_event_name":"PostToolUseFailure", "tool_name":"Bash", "error":"exit 1"}),
    );
    activity.apply_hook(&tool_failure);
    assert_eq!(
        activity.status(),
        AgentStatus::Working,
        "the agent can recover from a tool failure"
    );
    let failed = parse(json!({"hook_event_name":"StopFailure", "error":"rate_limit"}));
    activity.apply_hook(&failed);
    assert_eq!(activity.status(), AgentStatus::Idle);
    assert_eq!(failed.turn_outcome, AiTurnOutcome::Failed);
    let notification =
        crate::notify::Notification::from_ai_hook(&failed, failed.message.clone(), false).unwrap();
    assert!(notification.is_failure());
    let success = parse(json!({"hook_event_name":"Stop"}));
    activity.apply_hook(&success);
    assert_eq!(activity.status(), AgentStatus::Done);
}
