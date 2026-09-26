use super::*;
use crate::ai_agents::AgentKind;
use crate::ai_hook::parse_remote_envelope;

fn event(source: &str, session: &str, name: &str) -> AiHookEvent {
    let payload = if source == "codex" && name == "notify" {
        serde_json::json!({"type":"agent-turn-complete", "thread-id":session})
    } else if matches!(source, "opencode" | "pi") {
        let kind = match name {
            "SessionStart" => "session-start",
            "UserPromptSubmit" => "prompt",
            "PermissionRequest" => "attention",
            "PostToolUse" => "tool-complete",
            "Stop" => "done",
            "SessionEnd" => "session-end",
            _ => panic!("unknown fixture event {name}"),
        };
        serde_json::json!({"kind":kind, "session_id":session})
    } else {
        serde_json::json!({"hook_event_name":name, "session_id":session})
    };
    let wire = format!("nebula-hook/1 source={source}\n{payload}");
    parse_remote_envelope(wire.as_bytes(), Some(1)).unwrap()
}

fn screen(status: AgentStatus) -> Option<Detection> {
    Some(Detection { agent: AgentKind::Codex, status, rule_id: "test.chrome".into() })
}

#[test]
fn complete_hooks_own_every_state_even_when_screen_disagrees() {
    for provider in ["claude", "opencode"] {
        let mut activity = AgentActivity::default();
        for (name, status) in [
            ("SessionStart", AgentStatus::Idle),
            ("UserPromptSubmit", AgentStatus::Working),
            ("PermissionRequest", AgentStatus::Blocked),
            ("PostToolUse", AgentStatus::Working),
            ("Stop", AgentStatus::Done),
        ] {
            assert!(activity.apply_hook(&event(provider, "primary", name)));
            for candidate in [AgentStatus::Idle, AgentStatus::Blocked, AgentStatus::Working] {
                for _ in 0..6 {
                    assert!(!activity.observe_screen(screen(candidate)));
                }
                assert_eq!(activity.status(), status, "{provider}/{name}/{candidate:?}");
                assert_eq!(activity.source(), AgentStatusSource::Hook);
            }
        }
    }
}

#[test]
fn turn_hooks_allow_only_missing_attention_evidence_during_a_turn() {
    let mut activity = AgentActivity::default();
    activity.apply_hook(&event("pi", "session", "SessionStart"));
    assert!(!activity.observe_screen(screen(AgentStatus::Working)));
    assert!(!activity.observe_screen(screen(AgentStatus::Blocked)));
    activity.apply_hook(&event("pi", "session", "UserPromptSubmit"));
    for _ in 0..6 {
        assert!(!activity.observe_screen(screen(AgentStatus::Idle)));
    }
    assert_eq!(activity.status(), AgentStatus::Working);
    assert!(activity.observe_screen(screen(AgentStatus::Blocked)));
    activity.apply_hook(&event("pi", "session", "PostToolUse"));
    assert_eq!(activity.status(), AgentStatus::Working);
    activity.apply_hook(&event("pi", "session", "Stop"));
    activity.input_sent();
    for _ in 0..6 {
        activity.observe_screen(screen(AgentStatus::Idle));
    }
    for status in [AgentStatus::Working, AgentStatus::Blocked] {
        assert!(!activity.observe_screen(screen(status)));
        assert_eq!(activity.status(), AgentStatus::Done);
    }
}

#[test]
fn completion_only_hooks_latch_old_frames_but_can_observe_a_new_turn() {
    let mut activity = AgentActivity::default();
    activity.apply_hook(&event("codex", "thread", "notify"));
    for status in [AgentStatus::Working, AgentStatus::Blocked] {
        assert!(!activity.observe_screen(screen(status)));
        assert_eq!(activity.status(), AgentStatus::Done);
    }
    // A single idle redraw does not reopen the previous turn.
    activity.observe_screen(screen(AgentStatus::Idle));
    activity.observe_screen(screen(AgentStatus::Working));
    assert_eq!(activity.status(), AgentStatus::Done);
    for _ in 0..2 {
        activity.observe_screen(screen(AgentStatus::Idle));
    }
    assert!(activity.observe_screen(screen(AgentStatus::Working)));
    for _ in 0..6 {
        activity.observe_screen(screen(AgentStatus::Idle));
    }
    assert_eq!(activity.status(), AgentStatus::Working, "completion still belongs to notify");
    activity.apply_hook(&event("codex", "thread", "notify"));
    activity.input_sent();
    assert!(activity.observe_screen(screen(AgentStatus::Blocked)));
    assert_eq!(activity.status(), AgentStatus::Blocked);
}

#[test]
fn codex_capabilities_follow_the_installed_notify_bridge() {
    let notify = event("codex", "thread", "notify");
    assert!(!notify.capabilities().lifecycle);
    assert!(!notify.capabilities().attention_context);
    // Merely recognizing a provider's event name would not prove that our
    // installer delivers its starts, approvals and completions as a contract.
    for name in ["SessionStart", "UserPromptSubmit", "PermissionRequest", "Stop"] {
        let wire = format!(
            "nebula-hook/1 source=codex\n{{\"hook_event_name\":\"{name}\",\"session_id\":\"s\"}}"
        );
        assert!(parse_remote_envelope(wire.as_bytes(), Some(1)).is_none());
    }
}

#[test]
fn fallback_requires_consecutive_idle_and_work_before_a_completion() {
    let mut activity = AgentActivity::default();
    activity.begin_command(true);
    for _ in 0..2 {
        activity.observe_screen(screen(AgentStatus::Idle));
    }
    assert_eq!(activity.status(), AgentStatus::Idle, "opening a CLI did not finish a turn");
    activity.observe_screen(screen(AgentStatus::Working));
    activity.observe_screen(screen(AgentStatus::Idle));
    activity.observe_screen(None);
    activity.observe_screen(screen(AgentStatus::Idle));
    assert_eq!(activity.status(), AgentStatus::Working);
    assert!(activity.observe_screen(screen(AgentStatus::Idle)));
    assert_eq!(activity.status(), AgentStatus::Done);
    assert_eq!(screen_notification(AgentStatus::Working, activity.status(), false), Some(false));
    assert_eq!(screen_notification(activity.status(), activity.status(), false), None);
}

#[test]
fn runtime_submission_does_not_finish_on_the_previous_idle_frame() {
    let mut activity = AgentActivity::default();
    activity.submitted();
    for _ in 0..8 {
        activity.observe_screen(screen(AgentStatus::Idle));
    }
    assert_eq!(activity.status(), AgentStatus::Working);
    activity.observe_screen(screen(AgentStatus::Working));
    for _ in 0..2 {
        activity.observe_screen(screen(AgentStatus::Idle));
    }
    assert_eq!(activity.status(), AgentStatus::Done);
}

#[test]
fn nested_local_or_remote_sessions_cannot_replace_primary_ownership() {
    for pid in [None, Some(10)] {
        let mut activity = AgentActivity::default();
        let mut primary = event("claude", "primary", "UserPromptSubmit");
        primary.agent_pid = pid;
        activity.apply_hook(&primary);
        for name in ["SessionStart", "PermissionRequest", "Stop", "SessionEnd"] {
            let mut child = event("claude", "child", name);
            child.agent_pid = pid.map(|pid| pid + 1);
            assert!(!activity.apply_hook(&child));
            assert_eq!(activity.status(), AgentStatus::Working);
        }
        assert_eq!(activity.primary_pid(), pid);
        assert!(!activity.apply_hook(&event("codex", "foreign", "notify")));
        activity.reset();
        assert!(activity.apply_hook(&event("codex", "next-command", "notify")));
    }
}

#[test]
fn verified_process_can_change_session_and_background_tasks_keep_working() {
    let mut activity = AgentActivity::default();
    let mut start = event("claude", "first", "SessionStart");
    start.agent_pid = Some(10);
    activity.apply_hook(&start);
    start.session_id = Some("cleared".into());
    assert!(activity.apply_hook(&start));
    let mut late = event("claude", "first", "Stop");
    late.agent_pid = Some(10);
    assert!(!activity.apply_hook(&late), "the old session cannot undo a verified /clear");
    start.kind = AiHookKind::TurnDone;
    start.background_tasks = Some(super::super::AiBackgroundTasks { active: 1, total: 2 });
    activity.apply_hook(&start);
    assert_eq!(activity.status(), AgentStatus::Working);
    start.background_tasks.as_mut().unwrap().active = 0;
    activity.apply_hook(&start);
    assert_eq!(activity.status(), AgentStatus::Done);
    start.kind = AiHookKind::SessionEnd;
    activity.apply_hook(&start);
    assert_eq!(activity.status(), AgentStatus::Unknown);
    assert!(!activity.hook_seen());
    assert_eq!(activity.primary_pid(), None);
}

#[test]
fn sessions_inside_one_provider_process_do_not_share_turn_authority() {
    let mut activity = AgentActivity::default();
    let mut primary = event("opencode", "main", "UserPromptSubmit");
    primary.agent_pid = Some(20);
    activity.apply_hook(&primary);
    for name in ["UserPromptSubmit", "PermissionRequest", "Stop", "SessionEnd"] {
        let mut nested = event("opencode", "nested", name);
        nested.agent_pid = Some(20);
        assert!(!activity.apply_hook(&nested));
        assert_eq!(activity.status(), AgentStatus::Working);
    }
}

#[test]
fn ordinary_idle_reminders_are_not_blocking_requests() {
    for kind in [
        "idle",
        "idle_prompt",
        "auth_success",
        "assistant_message",
        "input_completed",
        "permission_granted",
        "approval_complete",
    ] {
        let wire = format!(
            "nebula-hook/1 source=claude\n{{\"hook_event_name\":\"Notification\",\"notification_type\":\"{kind}\"}}"
        );
        assert!(parse_remote_envelope(wire.as_bytes(), Some(1)).is_none());
    }
    assert_eq!(event("claude", "s", "PermissionRequest").kind, AiHookKind::NeedsAttention);
    let question = b"nebula-hook/1 source=claude\n{\"hook_event_name\":\"Notification\",\"notification_type\":\"elicitation_dialog\"}";
    assert_eq!(parse_remote_envelope(question, Some(1)).unwrap().kind, AiHookKind::NeedsAttention);
}

#[test]
fn untyped_legacy_notifications_cannot_reopen_an_idle_or_finished_turn() {
    let wire = b"nebula-hook/1 source=claude\n{\"hook_event_name\":\"Notification\",\"session_id\":\"s\",\"message\":\"waiting for input\"}";
    let notification = parse_remote_envelope(wire, Some(1)).unwrap();
    let mut activity = AgentActivity::default();
    activity.apply_hook(&event("claude", "s", "SessionStart"));
    assert!(!activity.apply_hook(&notification));
    assert_eq!(activity.status(), AgentStatus::Idle);
    activity.apply_hook(&event("claude", "s", "UserPromptSubmit"));
    assert!(activity.apply_hook(&notification));
    assert_eq!(activity.status(), AgentStatus::Blocked);
    activity.apply_hook(&event("claude", "s", "Stop"));
    assert!(!activity.apply_hook(&notification));
    assert_eq!(activity.status(), AgentStatus::Done);
    // An explicit permission request is actionable even without a prior turn.
    assert!(activity.apply_hook(&event("claude", "s", "PermissionRequest")));
    assert_eq!(activity.status(), AgentStatus::Blocked);
}

#[test]
fn shell_command_end_rejects_late_hooks_until_a_new_command_starts() {
    let mut activity = AgentActivity::default();
    activity.apply_hook(&event("claude", "old", "UserPromptSubmit"));
    activity.command_finished();
    for status in [AgentStatus::Working, AgentStatus::Blocked, AgentStatus::Idle] {
        assert!(!activity.observe_screen(screen(status)));
    }
    for name in ["PostToolUse", "PermissionRequest", "Stop"] {
        assert!(!activity.apply_hook(&event("claude", "old", name)));
        assert_eq!(activity.status(), AgentStatus::Unknown);
    }
    activity.begin_command(true);
    assert!(activity.apply_hook(&event("codex", "new", "notify")));
}
