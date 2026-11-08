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

fn event_with_turn(source: &str, session: &str, name: &str, turn: &str) -> AiHookEvent {
    let mut event = event(source, session, name);
    event.turn_id = Some(turn.to_owned());
    event
}

fn screen(status: AgentStatus) -> Option<Detection> {
    Some(Detection { agent: AgentKind::Codex, status, rule_id: format!("test.{status:?}") })
}

#[test]
fn delegation_runtime_turn_stays_pending_across_a_late_session_start() {
    let mut activity = AgentActivity::default();
    activity.submitted_text("claude", "initial prompt");
    assert!(activity.runtime_turn_pending());
    assert!(activity.apply_hook(&event("claude", "main", "SessionStart")));
    assert_eq!(activity.status(), AgentStatus::Idle);
    assert!(activity.runtime_turn_pending(), "SessionStart is not a task completion");
    assert!(activity.apply_hook(&event("claude", "main", "UserPromptSubmit")));
    assert!(activity.runtime_turn_pending());
    assert!(activity.apply_hook(&event("claude", "main", "Stop")));
    assert!(!activity.runtime_turn_pending());
}

#[test]
fn delegation_runtime_turn_is_released_on_a_verified_conversation_replacement() {
    let mut activity = AgentActivity::default();
    let mut start = event("claude", "first", "SessionStart");
    start.remote_process = Some("99:1".into());
    assert!(activity.apply_hook(&start));
    activity.submitted();
    let mut replacement = event("claude", "second", "SessionStart");
    replacement.remote_process = start.remote_process.clone();
    assert!(activity.apply_hook(&replacement));
    assert!(!activity.runtime_turn_pending());
}

#[test]
fn complete_hooks_own_every_state_even_when_screen_disagrees() {
    // An opencode stop without a classified outcome proves nothing about the answer.
    for (provider, stopped) in [("claude", AgentStatus::Done), ("opencode", AgentStatus::Idle)] {
        let mut activity = AgentActivity::default();
        for (name, status) in [
            ("SessionStart", AgentStatus::Idle),
            ("UserPromptSubmit", AgentStatus::Working),
            ("PermissionRequest", AgentStatus::Blocked),
            ("PostToolUse", AgentStatus::Working),
            ("Stop", stopped),
        ] {
            assert!(activity.apply_hook(&event(provider, "primary", name)));
            for candidate in [AgentStatus::Idle, AgentStatus::Blocked, AgentStatus::Working] {
                for _ in 0..6 {
                    assert!(!activity.observe_screen(screen(candidate)));
                }
                assert_eq!(activity.status(), status, "{provider}/{name}/{candidate:?}");
                assert_eq!(activity.source(), AgentStatusSource::Hook);
            }
            for _ in 0..6 {
                assert!(!activity.observe_screen(None));
                assert_eq!(activity.status(), status, "{provider}/{name}");
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
fn claude_form_disappearing_into_an_unmatched_running_layout_clears_attention() {
    let mut activity = AgentActivity::default();
    activity.submitted();
    let form = "Do you want to proceed?\n❯ 1. Yes\n2. No\nEsc to cancel · Tab to amend";
    assert!(activity.observe_screen(crate::ai_agents::detect("claude", form)));
    assert_eq!(activity.status(), AgentStatus::Blocked);
    let running = "* Ideating… (2m · 123 tokens)\n────────\n❯\n────────\n版本 2.1.301\n上下文 25%\n状态 运行中\n/project";
    assert!(crate::ai_agents::detect("claude", running).is_none());
    assert!(!activity.observe_screen(crate::ai_agents::detect("claude", running)));
    assert_eq!(activity.status(), AgentStatus::Blocked, "one redraw is inconclusive");
    assert!(activity.observe_screen(crate::ai_agents::detect("claude", running)));
    assert_eq!(activity.status(), AgentStatus::Working);
    assert_eq!(activity.source(), AgentStatusSource::Process);
    assert_eq!(activity.rule(), None);
    assert_eq!(screen_notification(AgentStatus::Blocked, activity.status(), false), None);
    for _ in 0..8 {
        assert!(!activity.observe_screen(None));
        assert_eq!(activity.status(), AgentStatus::Working, "silence cannot finish a turn");
    }
}

#[test]
fn expired_screen_attention_restores_work_evidence_including_partial_hooks() {
    for source in [AgentStatusSource::Process, AgentStatusSource::Screen, AgentStatusSource::Hook] {
        let mut activity = AgentActivity::default();
        match source {
            AgentStatusSource::Process => activity.submitted(),
            AgentStatusSource::Screen => {
                activity.observe_screen(screen(AgentStatus::Working));
            },
            AgentStatusSource::Hook => {
                activity.apply_hook(&event("pi", "partial", "UserPromptSubmit"));
            },
            _ => unreachable!(),
        }
        let rule = activity.rule().map(str::to_owned);
        activity.observe_screen(screen(AgentStatus::Blocked));
        assert!(!activity.observe_screen(None));
        assert!(activity.observe_screen(None));
        assert_eq!(activity.status(), AgentStatus::Working);
        assert_eq!(activity.source(), source);
        assert_eq!(activity.rule(), rule.as_deref());
        for _ in 0..2 {
            activity.observe_screen(screen(AgentStatus::Idle));
        }
        assert_eq!(
            activity.status(),
            if source == AgentStatusSource::Hook {
                AgentStatus::Working
            } else {
                AgentStatus::Done
            },
            "real idle evidence still respects completion ownership"
        );
    }
}

#[test]
fn expired_screen_attention_without_work_evidence_becomes_unknown() {
    let mut activity = AgentActivity::default();
    activity.observe_screen(screen(AgentStatus::Blocked));
    assert!(!activity.observe_screen(None));
    assert!(activity.observe_screen(None));
    assert_eq!(activity.status(), AgentStatus::Unknown);
    assert_eq!(activity.source(), AgentStatusSource::Unknown);
    assert_eq!(activity.rule(), None);
    assert!(!activity.turn_observed, "an expired form is not a submitted turn");
    assert_eq!(screen_notification(AgentStatus::Blocked, activity.status(), false), None);
}

#[test]
fn live_forms_and_other_screen_evidence_interrupt_unmatched_samples() {
    let mut activity = AgentActivity::default();
    activity.submitted();
    activity.observe_screen(screen(AgentStatus::Blocked));
    for _ in 0..6 {
        assert!(!activity.observe_screen(None));
        assert!(!activity.observe_screen(screen(AgentStatus::Blocked)));
        assert_eq!(activity.status(), AgentStatus::Blocked);
    }
    assert!(!activity.observe_screen(None));
    assert!(!activity.observe_screen(screen(AgentStatus::Idle)));
    assert!(!activity.observe_screen(None), "the idle sample interrupted absence evidence");
    assert_eq!(activity.status(), AgentStatus::Blocked);
    assert!(activity.observe_screen(None));
    assert_eq!(activity.status(), AgentStatus::Working);
}

#[test]
fn input_and_unidentified_snapshots_interrupt_attention_expiry() {
    for input in [false, true] {
        let mut activity = AgentActivity::default();
        activity.submitted();
        activity.observe_screen(screen(AgentStatus::Blocked));
        assert!(!activity.observe_screen(None));
        if input {
            activity.input_sent();
        } else {
            activity.reset_screen_samples();
        }
        assert!(!activity.observe_screen(None));
        assert_eq!(activity.status(), AgentStatus::Blocked);
        assert!(activity.observe_screen(None));
        assert_eq!(activity.status(), AgentStatus::Working);
    }
}

#[test]
fn new_work_and_command_boundaries_discard_previous_attention_evidence() {
    for boundary in ["submit", "hook", "command"] {
        let mut activity = AgentActivity::default();
        activity.observe_screen(screen(AgentStatus::Working));
        activity.observe_screen(screen(AgentStatus::Blocked));
        activity.observe_screen(None);
        match boundary {
            "submit" => activity.submitted(),
            "hook" => {
                assert!(activity.apply_hook(&event("pi", "new-work", "UserPromptSubmit")));
            },
            "command" => {
                activity.command_finished();
                assert!(!activity.observe_screen(None));
                activity.begin_command(true);
            },
            _ => unreachable!(),
        }
        activity.observe_screen(screen(AgentStatus::Blocked));
        assert!(!activity.observe_screen(None));
        assert!(activity.observe_screen(None));
        assert_eq!(activity.status(), AgentStatus::Working);
        assert_eq!(
            activity.source(),
            if boundary == "hook" { AgentStatusSource::Hook } else { AgentStatusSource::Process }
        );
        assert_eq!(activity.rule(), None, "old screen rule cannot survive new work");
        if boundary == "command" {
            for _ in 0..2 {
                activity.observe_screen(screen(AgentStatus::Idle));
            }
            assert_eq!(activity.status(), AgentStatus::Idle, "launching is not a submitted turn");
        }
    }
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
fn local_mode_command_keeps_activity_and_real_prompts_keep_the_submission_barrier() {
    for native_hook in [false, true] {
        let mut activity = AgentActivity::default();
        if native_hook {
            let wire = b"nebula-hook/1 source=codex codex_hooks=full\n{\"hook_event_name\":\"SessionStart\",\"session_id\":\"s\"}";
            let event = parse_remote_envelope(wire, Some(1)).unwrap();
            activity.apply_hook(&event);
        } else {
            activity.begin_command(true);
            for _ in 0..2 {
                activity.observe_screen(screen(AgentStatus::Idle));
            }
        }
        let source = activity.source();
        activity.submitted_text("codex", "/plan");
        assert_eq!(activity.status(), AgentStatus::Idle);
        assert_eq!(activity.source(), source);
        assert!(!activity.pending_submit);
        activity.submitted_text("codex", "请问我一个选择题");
        for _ in 0..8 {
            activity.observe_screen(screen(AgentStatus::Idle));
        }
        assert_eq!(activity.status(), AgentStatus::Working);
        assert!(activity.pending_submit);
        activity.submitted_text("codex", "/plan");
        assert_eq!(activity.status(), AgentStatus::Working, "a mode switch cannot finish work");
    }
    for (program, text) in [("claude", "/plan"), ("codex", "/review"), ("codex", "/plan\nmy task")]
    {
        let mut activity = AgentActivity::default();
        activity.submitted_text(program, text);
        assert!(activity.pending_submit, "{program}: {text}");
    }
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
fn untyped_legacy_notifications_cannot_override_any_explicit_state() {
    let wire = b"nebula-hook/1 source=claude\n{\"hook_event_name\":\"Notification\",\"session_id\":\"s\",\"message\":\"waiting for input\"}";
    assert!(parse_remote_envelope(wire, Some(1)).is_none());
    let mut notification = event("claude", "s", "PermissionRequest");
    notification.legacy_attention = true;
    let mut activity = AgentActivity::default();
    activity.apply_hook(&event("claude", "s", "SessionStart"));
    assert!(!activity.apply_hook(&notification));
    assert_eq!(activity.status(), AgentStatus::Idle);
    activity.apply_hook(&event("claude", "s", "UserPromptSubmit"));
    assert!(!activity.apply_hook(&notification));
    assert_eq!(activity.status(), AgentStatus::Working);
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

/// Age and familiar screen forms cannot replace a complete provider lifecycle.
#[test]
fn full_hook_states_remain_authoritative_after_long_silence() {
    for (name, expected) in [
        ("UserPromptSubmit", AgentStatus::Working),
        ("PermissionRequest", AgentStatus::Blocked),
        ("Stop", AgentStatus::Done),
    ] {
        let mut activity = AgentActivity::default();
        activity.apply_hook(&event("claude", "s", "UserPromptSubmit"));
        activity.apply_hook(&event("claude", "s", name));
        activity.backdate_activity_for_test(std::time::Duration::from_secs(3600));
        for _ in 0..4 {
            for status in [AgentStatus::Idle, AgentStatus::Working, AgentStatus::Blocked] {
                assert!(!activity.observe_screen(screen(status)));
                assert_eq!(activity.status(), expected);
                assert_eq!(activity.source(), AgentStatusSource::Hook);
            }
        }
    }
}

#[test]
fn a_silent_submission_waits_for_an_explicit_result() {
    let mut activity = AgentActivity::default();
    activity.apply_hook(&event("claude", "s", "Stop"));
    activity.submitted();
    activity.backdate_activity_for_test(std::time::Duration::from_secs(3600));
    for _ in 0..4 {
        assert!(!activity.observe_screen(screen(AgentStatus::Idle)));
        assert_eq!(activity.status(), AgentStatus::Working);
    }
    assert!(activity.apply_hook(&event("claude", "s", "Stop")));
    assert_eq!(activity.status(), AgentStatus::Done);
}

#[test]
fn a_delayed_failure_is_not_preceded_by_screen_success() {
    let mut activity = AgentActivity::default();
    activity.apply_hook(&event_with_turn("claude", "f", "UserPromptSubmit", "f-1"));
    activity.backdate_activity_for_test(std::time::Duration::from_secs(3600));
    for _ in 0..4 {
        assert!(!activity.observe_screen(screen(AgentStatus::Idle)));
        assert_eq!(activity.status(), AgentStatus::Working);
    }
    let mut failed = event_with_turn("claude", "f", "Stop", "f-1");
    failed.turn_outcome = crate::ai_hook::AiTurnOutcome::Failed;
    assert!(activity.apply_hook(&failed));
    assert_eq!(activity.status(), AgentStatus::Idle);
    assert_eq!(activity.source(), AgentStatusSource::Hook);
}

#[test]
fn answering_attention_requires_a_client_signal_to_resume() {
    let mut activity = AgentActivity::default();
    activity.apply_hook(&event("claude", "s", "PermissionRequest"));
    activity.backdate_activity_for_test(std::time::Duration::from_secs(3600));
    for _ in 0..4 {
        assert!(!activity.observe_screen(screen(AgentStatus::Working)));
        assert_eq!(activity.status(), AgentStatus::Blocked);
    }
    assert!(activity.apply_hook(&event("claude", "s", "PostToolUse")));
    assert_eq!(activity.status(), AgentStatus::Working);
}
