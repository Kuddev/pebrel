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

/// 远端回合停滞对账：只在「hook 权威禁用屏幕（allows_screen false）+ 停滞 +
/// 连续两拍 idle」同时成立时收敛；任何反证都归零计数。
#[test]
fn stalled_turn_reconciliation_needs_two_idle_samples_after_the_stall() {
    let mut activity = AgentActivity::default();
    activity.apply_hook(&event("claude", "stall", "UserPromptSubmit"));
    assert_eq!(activity.status(), AgentStatus::Working);
    assert!(!activity.hook_quiet_for(std::time::Duration::from_secs(20)));
    activity.backdate_hook_for_test(std::time::Duration::from_secs(21));
    assert!(activity.hook_quiet_for(std::time::Duration::from_secs(20)));
    // 未停滞 / 非 idle 检测 / 不足两拍：全部不动。
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), false));
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Working).as_ref(), true));
    assert!(!activity.reconcile_stalled_turn(None, true));
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert_eq!(activity.status(), AgentStatus::Working);
    // 第二拍收敛为 Done（PromptSubmit 已观测回合），来源记屏幕与规则。
    assert!(activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert_eq!(activity.status(), AgentStatus::Done);
    assert_eq!(activity.source(), AgentStatusSource::Screen);
    assert_eq!(activity.rule(), Some("test.chrome"));
}

/// 无 hook 权威（allows_screen 为真）时对账让位给 observe_screen；会话仅
/// SessionStart（status 已是 Idle）时无事可做，不得伪造一次迁移。
#[test]
fn stall_reconciliation_yields_to_screen_authority_and_reads_turn_observed() {
    let mut activity = AgentActivity::default();
    // 无 hook：allows_screen() 为真 → 恒 false。
    activity.begin_command(true);
    activity.backdate_hook_for_test(std::time::Duration::from_secs(60));
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert_eq!(activity.status(), AgentStatus::Working);
    // 只有 SessionStart 的会话已经在 Idle 上，没有可完成的回合。
    let mut idle_only = AgentActivity::default();
    idle_only.apply_hook(&event("claude", "fresh", "SessionStart"));
    idle_only.backdate_hook_for_test(std::time::Duration::from_secs(60));
    assert!(!idle_only.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert!(!idle_only.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert_eq!(idle_only.status(), AgentStatus::Idle);
    assert_eq!(idle_only.source(), AgentStatusSource::Hook);
}

/// 刚提交（pending_submit）的回合即使屏幕看起来 idle 也不收边——提交屏障
/// 与 observe_screen 的 idle 分支同一语义；对点火方向同样成立（屏障期间两向
/// 计数都归零，屏障解除后证据要重新攒）。
#[test]
fn stall_reconciliation_honors_the_pending_submit_barrier() {
    let mut activity = AgentActivity::default();
    activity.apply_hook(&event("claude", "pending", "UserPromptSubmit"));
    activity.submitted();
    assert_eq!(activity.status(), AgentStatus::Working);
    // hook 时间线仍在滚动（未停滞）：屏障绝对在，屏幕证据一律不算。
    for _ in 0..4 {
        assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), false));
        assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Working).as_ref(), false));
    }
    assert_eq!(activity.status(), AgentStatus::Working);
}

/// 提交屏障的恢复边界（评审 3）：提交后 PromptSubmit 与后续 hook 在远端
/// bridge 上全部丢失时，屏障只活到停滞阈值——过期后屏幕证据恢复话语权，
/// 收敛/点火仍需连续两拍稳定形态（不会因过期而瞬时定论）。
#[test]
fn pending_submit_barrier_expires_once_hooks_stay_silent_past_the_stall() {
    let mut activity = AgentActivity::default();
    activity.apply_hook(&event("claude", "lost-submit", "UserPromptSubmit"));
    activity.backdate_hook_for_test(std::time::Duration::from_secs(21));
    activity.submitted();
    assert_eq!(activity.status(), AgentStatus::Working);
    // 第一拍停滞：屏障过期本拍不定论，两向计数归零。
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    // 交替形态依旧不积累。
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Working).as_ref(), true));
    // 连续两拍 idle 收敛：submitted 已记 turn_observed，落 Done 而非 Idle。
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert!(activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert_eq!(activity.status(), AgentStatus::Done);
    assert_eq!(activity.source(), AgentStatusSource::Screen);
}

/// 屏障过期后的反方向（评审 3 的镜像）：提交链路整体丢失、屏幕先持续转圈
/// （状态本就 Working，点火无状态沿），随后屏幕回到 idle——屏障不得挡住
/// 这条收敛路径。
#[test]
fn expired_submit_barrier_lets_a_later_idle_screen_settle() {
    let mut activity = AgentActivity::default();
    activity.apply_hook(&event("claude", "lost-submit-2", "UserPromptSubmit"));
    activity.backdate_hook_for_test(std::time::Duration::from_secs(21));
    activity.submitted();
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Working).as_ref(), true));
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Working).as_ref(), true));
    assert_eq!(activity.status(), AgentStatus::Working);
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert!(activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert_eq!(activity.status(), AgentStatus::Done);
    assert_eq!(activity.source(), AgentStatusSource::Screen);
}

/// 推断完成的补正（评审 2）：停滞对账收敛 Done 后 turn_id 仍在，同 turn
/// 的迟到真 TurnDone 不被去重护栏拦截——成功的真实结果与失败通知都能落地；
/// 权威 TurnDone 之后的重放才被拦。
#[test]
fn late_authoritative_turn_done_supersedes_an_inferred_done() {
    let mut activity = AgentActivity::default();
    activity.apply_hook(&event_with_turn("claude", "t", "UserPromptSubmit", "t-9"));
    activity.backdate_hook_for_test(std::time::Duration::from_secs(21));
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert!(activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert_eq!(activity.status(), AgentStatus::Done, "inferred by the screen");
    // 迟到的成功 TurnDone：被采纳，来源翻正为 hook。
    assert!(activity.apply_hook(&event_with_turn("claude", "t", "Stop", "t-9")));
    assert_eq!(activity.status(), AgentStatus::Done);
    assert_eq!(activity.source(), AgentStatusSource::Hook);
    // 权威完成后的同 turn 重放照旧被去重护栏拦下。
    assert!(!activity.apply_hook(&event_with_turn("claude", "t", "Stop", "t-9")));
}

/// 推断完成的失败补正（评审 2 失败半边）：迟到的失败 TurnDone 把状态收敛
/// 为 Idle 语义（失败不是「完成待命」），事件本身被采纳。
#[test]
fn late_failed_turn_done_supersedes_an_inferred_done() {
    let mut activity = AgentActivity::default();
    activity.apply_hook(&event_with_turn("claude", "f", "UserPromptSubmit", "f-1"));
    activity.backdate_hook_for_test(std::time::Duration::from_secs(21));
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert!(activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert_eq!(activity.status(), AgentStatus::Done);
    let mut failed = event_with_turn("claude", "f", "Stop", "f-1");
    failed.turn_outcome = crate::ai_hook::AiTurnOutcome::Failed;
    assert!(activity.apply_hook(&failed));
    assert_eq!(activity.status(), AgentStatus::Idle, "a failed turn is not a ready turn");
    assert_eq!(activity.source(), AgentStatusSource::Hook);
}

/// 对账的反方向：UserPromptSubmit 在远端 bridge 丢失（通道锁 busy/超时/超限）
/// 时，上一回合的 Stop 把回合钉在 Done；hook 静默 + 连续两拍转圈重新点火
/// Working，并补记 turn_observed（迟到的 Stop 能收敛为 Done 而非 Idle）。
#[test]
fn stalled_turn_reconciliation_reignites_a_lost_prompt_submit() {
    let mut activity = AgentActivity::default();
    activity.apply_hook(&event("claude", "lost", "UserPromptSubmit"));
    activity.apply_hook(&event("claude", "lost", "Stop"));
    assert_eq!(activity.status(), AgentStatus::Done);
    activity.backdate_hook_for_test(std::time::Duration::from_secs(21));
    // 一拍转圈不点火；中途一拍 idle 归零 working 计数，证据重新攒。
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Working).as_ref(), true));
    assert_eq!(activity.status(), AgentStatus::Done);
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Working).as_ref(), true));
    assert!(activity.reconcile_stalled_turn(screen(AgentStatus::Working).as_ref(), true));
    assert_eq!(activity.status(), AgentStatus::Working);
    assert_eq!(activity.source(), AgentStatusSource::Screen);
    assert_eq!(activity.rule(), Some("test.chrome"));
    // 点火补记了回合：之后两拍 idle 收敛回 Done（双向闭环）。
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert!(activity.reconcile_stalled_turn(screen(AgentStatus::Idle).as_ref(), true));
    assert_eq!(activity.status(), AgentStatus::Done);
}

/// 权限答复（PostToolUse/PermissionResult）在远端丢失时回合钉在 Blocked；
/// 用户已答复、屏幕回到转圈，两拍 working 同样重新点火。权限屏本身
/// （Blocked 检测）不参与对账——attention 语义归 hook 与 observe_screen 管。
#[test]
fn stalled_turn_reconciliation_reignites_a_blocked_turn_after_the_answer() {
    let mut activity = AgentActivity::default();
    activity.apply_hook(&event("claude", "blocked", "UserPromptSubmit"));
    activity.apply_hook(&event("claude", "blocked", "PermissionRequest"));
    assert_eq!(activity.status(), AgentStatus::Blocked);
    activity.backdate_hook_for_test(std::time::Duration::from_secs(21));
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Blocked).as_ref(), true));
    assert!(!activity.reconcile_stalled_turn(screen(AgentStatus::Working).as_ref(), true));
    assert!(activity.reconcile_stalled_turn(screen(AgentStatus::Working).as_ref(), true));
    assert_eq!(activity.status(), AgentStatus::Working);
    assert_eq!(activity.source(), AgentStatusSource::Screen);
}
