//! Protocol, context and event ordering regressions.
use super::ordering::AiHookEventGate;
use super::payload::RAW_CONTEXT_MAX_BYTES;

use super::{AiHookKind, GateVerdict, capabilities_for, parse_remote_envelope, reorder_batch};

#[test]
fn remote_envelope_uses_local_pane_identity() {
    let raw = b"nebula-hook/1 source=codex pane=999\n{\"type\":\"agent-turn-complete\",\"last-assistant-message\":\"done\"}";
    let event = parse_remote_envelope(raw, Some(7)).unwrap();
    assert_eq!(event.pane, Some(7));
    assert_eq!(event.kind, AiHookKind::TurnDone);
    assert_eq!(event.message.as_deref(), Some("done"));
}

#[test]
fn claude_events_carry_their_session_id_for_cold_resume() {
    let raw = b"nebula-hook/1 source=claude pane=3\n{\"session_id\":\"0199a213-c2a4-7cf5-8f6b-d746fbb6e86c\",\"hook_event_name\":\"Stop\"}";
    let event = parse_remote_envelope(raw, Some(3)).unwrap();
    assert_eq!(event.session_id.as_deref(), Some("0199a213-c2a4-7cf5-8f6b-d746fbb6e86c"));
}

/// 串台门的第二层。第一层（`nebula_hook::foreign_hook_runner`）是环境变量
/// 判据，会随上游改名静默失效；这条测试钉住「即使那扇门不响，载荷形状仍然
/// 拦得住」。别家 hook runner 读我们装在 ~/.claude/settings.json 里的条目、
/// 用 camelCase 发事件，若被当成 claude 上报，pane 会贴错 provider 身份，
/// 而且它的 session id 会被交给 `claude --resume` —— 一个不存在的会话。
#[test]
fn foreign_runner_payload_is_rejected_even_if_the_env_gate_fails() {
    let raw = b"nebula-hook/1 source=claude pane=3\n{\"hookEventName\":\"Stop\",\"sessionId\":\"grok-session-1\",\"cwd\":\"D:/x\"}";
    assert!(parse_remote_envelope(raw, Some(3)).is_none());
}

/// claude 只发 snake_case。即使一个载荷同时带着合法的 `hook_event_name`
/// 和别家风格的 `sessionId`，也绝不能把后者当成 claude 的会话身份。
#[test]
fn claude_session_id_never_comes_from_camel_case() {
    let raw = b"nebula-hook/1 source=claude pane=3\n{\"hook_event_name\":\"Stop\",\"sessionId\":\"grok-session-1\"}";
    let event = parse_remote_envelope(raw, Some(3)).unwrap();
    assert_eq!(event.kind, AiHookKind::TurnDone);
    assert_eq!(event.session_id, None);
}

/// 防御式解析：不认识才丢，缺失要放行。两个方向都会咬人——读不到类型就丢
/// 会让 provider 改一次字段名就永久静默 attention 徽标；能读到却不认识还
/// 放行，会把 pane 停在一个清不掉的等待图标上。
#[test]
fn attention_type_gate_drops_only_types_it_can_read_and_does_not_know() {
    let missing = b"nebula-hook/1 source=claude pane=3\n{\"hook_event_name\":\"Notification\",\"message\":\"Claude needs your permission\"}";
    assert_eq!(
        parse_remote_envelope(missing, Some(3)).map(|event| event.kind),
        Some(AiHookKind::NeedsAttention),
        "没有类型字段时必须照常上报"
    );

    let known = b"nebula-hook/1 source=claude pane=3\n{\"hook_event_name\":\"Notification\",\"notificationType\":\"permission_prompt\"}";
    assert_eq!(
        parse_remote_envelope(known, Some(3)).map(|event| event.kind),
        Some(AiHookKind::NeedsAttention)
    );

    // 保留明确的旧桥接字段写法，不以 permission/input 子串猜测其他通知。
    let renamed = b"nebula-hook/1 source=claude pane=3\n{\"hook_event_name\":\"Notification\",\"notification_type\":\"tool_permission_request\"}";
    assert_eq!(
        parse_remote_envelope(renamed, Some(3)).map(|event| event.kind),
        Some(AiHookKind::NeedsAttention)
    );

    // 这类是 agent 在说话，不是在阻塞等待：放行会让等待图标清不掉。
    let chatty = b"nebula-hook/1 source=claude pane=3\n{\"hook_event_name\":\"Notification\",\"notificationType\":\"assistant_message\"}";
    assert!(parse_remote_envelope(chatty, Some(3)).is_none());

    // PermissionRequest 本身就是权限请求，不受通知类型字段影响。
    let explicit = b"nebula-hook/1 source=claude pane=3\n{\"hook_event_name\":\"PermissionRequest\",\"notificationType\":\"assistant_message\"}";
    assert_eq!(
        parse_remote_envelope(explicit, Some(3)).map(|event| event.kind),
        Some(AiHookKind::NeedsAttention)
    );
}

/// bypass 是持续状态，awaiting 是瞬时事件——两者必须分开读，否则「用户
/// 全局跳过权限」会被显示成「正在等你批准」，徽标永远误亮。
#[test]
fn permission_mode_is_reported_separately_from_attention() {
    let bypass = b"nebula-hook/1 source=claude pane=3\n{\"hook_event_name\":\"Notification\",\"permission_mode\":\"bypassPermissions\",\"message\":\"Claude is waiting for your input\"}";
    let event = parse_remote_envelope(bypass, Some(3)).unwrap();
    // 事件本身仍然是「需要你」——bypass 会话也会等你输入下一条指令。
    assert_eq!(event.kind, AiHookKind::NeedsAttention);
    // 但它不可能是在等批准，UI 的文案要据此区分。
    assert_eq!(event.permission_mode, Some(super::AiPermissionMode::BypassPermissions));
    assert!(!event.permission_mode.unwrap().can_ask_for_permission());
    assert_eq!(
        event.attention.as_ref().and_then(|context| context.permission_mode),
        Some(super::AiPermissionMode::BypassPermissions)
    );

    let asking = b"nebula-hook/1 source=claude pane=3\n{\"hook_event_name\":\"PermissionRequest\",\"permission_mode\":\"default\",\"tool_name\":\"Bash\"}";
    let event = parse_remote_envelope(asking, Some(3)).unwrap();
    assert!(event.permission_mode.unwrap().can_ask_for_permission());
    assert_eq!(
        event.attention.as_ref().and_then(|c| c.permission_or_tool.as_deref()),
        Some("Bash")
    );

    // 读不到就是读不到：不拿 argv 猜，也不默认成 default。
    let silent = b"nebula-hook/1 source=claude pane=3\n{\"hook_event_name\":\"Stop\"}";
    assert_eq!(parse_remote_envelope(silent, Some(3)).unwrap().permission_mode, None);
}

#[test]
fn codex_thread_id_is_the_session_identity() {
    // codex notify 的 `thread-id` 就是 rollout uuid，`codex resume` 认它。
    let raw = b"nebula-hook/1 source=codex pane=3\n{\"type\":\"agent-turn-complete\",\"thread-id\":\"b5f6c1c2-1111-2222-3333-444455556666\"}";
    let event = parse_remote_envelope(raw, Some(3)).unwrap();
    assert_eq!(event.session_id.as_deref(), Some("b5f6c1c2-1111-2222-3333-444455556666"));
    // 桥接载荷没有 id 的来源读出 None，不许瞎编。
    let raw = b"nebula-hook/1 source=opencode pane=3\n{\"kind\":\"done\"}";
    assert_eq!(parse_remote_envelope(raw, Some(3)).unwrap().session_id, None);
}

#[test]
fn pi_extension_events_use_the_normalized_lifecycle_shape() {
    let start = parse_remote_envelope(
        b"nebula-hook/1 source=pi pane=3\n{\"kind\":\"session-start\",\"sessionId\":\"pi-42\",\"bridge_sequence\":\"1700000000000001\"}",
        Some(3),
    )
    .unwrap();
    assert_eq!(start.kind, AiHookKind::SessionStart);
    assert_eq!(start.session_id.as_deref(), Some("pi-42"));
    assert_eq!(start.bridge_sequence, Some(1_700_000_000_000_001));

    let prompt =
        parse_remote_envelope(b"nebula-hook/1 source=pi pane=3\n{\"kind\":\"prompt\"}", Some(3))
            .unwrap();
    assert_eq!(prompt.kind, AiHookKind::PromptSubmit);

    let done =
        parse_remote_envelope(b"nebula-hook/1 source=pi pane=3\n{\"kind\":\"done\"}", Some(3))
            .unwrap();
    assert_eq!(done.kind, AiHookKind::TurnDone);

    for (kind, expected) in
        [("tool-complete", AiHookKind::ToolComplete), ("session-end", AiHookKind::SessionEnd)]
    {
        let raw = format!("nebula-hook/1 source=pi pane=3\n{{\"kind\":\"{kind}\"}}");
        assert_eq!(parse_remote_envelope(raw.as_bytes(), Some(3)).unwrap().kind, expected);
    }
}

#[test]
fn permission_context_is_structured_bounded_and_redacted() {
    let oversized = "x".repeat(RAW_CONTEXT_MAX_BYTES * 2);
    let payload = serde_json::json!({
        "session_id": "claude-session",
        "hook_event_name": "PermissionRequest",
        "cwd": "D:/work/nebula",
        "tool_name": "Write",
        "message": "Allow writing ai_hook.rs?",
        "selection": "selected source",
        "api_key": "must-not-leak",
        "tool_input": { "content": oversized }
    });
    let raw = format!("nebula-hook/1 source=claude pane=9\n{payload}");
    let event = parse_remote_envelope(raw.as_bytes(), Some(9)).unwrap();
    let context = event.attention.as_ref().unwrap();

    assert_eq!(event.kind, AiHookKind::NeedsAttention);
    assert_eq!(context.pane_id, Some(9));
    assert_eq!(context.cwd.as_deref(), Some("D:/work/nebula"));
    assert_eq!(context.permission_or_tool.as_deref(), Some("Write"));
    assert_eq!(context.selection.as_deref(), Some("selected source"));
    let sanitized = context.raw_context.as_deref().unwrap();
    assert!(sanitized.contains("[redacted]"));
    assert!(!sanitized.contains("must-not-leak"));
    assert!(sanitized.len() <= RAW_CONTEXT_MAX_BYTES);
    let summary = context.summary_for_pane(9);
    assert!(summary.contains("Pane 9"));
    assert!(summary.contains("Write"));
    assert!(!summary.contains("selected source"));
}

/// `background_tasks` 里的**每一笔**在飞的活儿都算数，不只是 subagent。
///
/// 2026-09-14：旧实现只认 `type == "subagent"`，而 Claude Code 给后台 bash 的
/// 类型名是 `local_bash`——「跑着后台命令、回合先结束」于是永远数出 active=0，
/// `TurnDone` 的守卫不触发，用户在命令还在跑时就收到「回合完成」。
/// 类型名取自本机 claude 2.1.270 二进制里的集合：`local_bash` / `subagent` /
/// `monitor` / `workflow` / `mcp_task` / `in_process_teammate` / `local_agent` /
/// `remote_agent` / `dream` / `auto_mode_scan` / `cloud_session`。
#[test]
fn claude_stop_counts_every_in_flight_background_task() {
    let raw = br#"nebula-hook/1 source=claude pane=3
{"session_id":"s","hook_event_name":"Stop","background_tasks":[{"type":"local_bash","status":"running"},{"type":"subagent","status":"completed"},{"type":"monitor","status":"running"},{"type":"mcp_task","status":"cancelled"}]}"#;
    let event = parse_remote_envelope(raw, Some(3)).unwrap();
    assert_eq!(event.kind, AiHookKind::TurnDone);
    assert_eq!(event.active_background_tasks(), 2, "local_bash 与 monitor 都还在跑");
    assert_eq!(event.background_tasks.unwrap().total, 4);
    assert!(capabilities_for("claude").background_tasks);
    assert!(!capabilities_for("pi").attention_context);
}

/// 空数组 = 真的收工：守卫必须放行，否则完成通知永远不弹。
#[test]
fn empty_background_tasks_leave_the_turn_done() {
    let raw = br#"nebula-hook/1 source=claude pane=3
{"session_id":"s","hook_event_name":"Stop","background_tasks":[]}"#;
    let event = parse_remote_envelope(raw, Some(3)).unwrap();
    assert_eq!(event.active_background_tasks(), 0);
    assert_eq!(event.background_tasks.unwrap().total, 0);
}

#[test]
fn pending_and_unknown_background_work_prevent_premature_completion() {
    let payload = serde_json::json!({
        "hook_event_name": "Stop",
        "background_tasks": { "entries": [
            { "type": "local_bash", "status": "pending" },
            { "type": "future_task", "status": "waiting_for_resource" },
            { "type": "monitor" },
            { "type": "subagent", "status": "COMPLETED" }
        ] }
    });
    let raw = format!("nebula-hook/1 source=claude pane=3\n{payload}");
    let event = parse_remote_envelope(raw.as_bytes(), Some(3)).unwrap();
    assert_eq!(event.kind, AiHookKind::TurnDone);
    assert_eq!(event.active_background_tasks(), 3);
    assert_eq!(event.background_tasks.unwrap().total, 4);
}

/// 闲置的 `in_process_teammate` 会一直挂 `status: "running"`
/// （anthropics/claude-code#85955），只有它自己的 `isIdle` 能说明它没在干活。
/// 少了这道判据，pane 会永远停在「还在跑」，完成通知再也弹不出来。
#[test]
fn idle_in_process_teammate_does_not_hold_the_turn_open() {
    let raw = br#"nebula-hook/1 source=claude pane=3
{"session_id":"s","hook_event_name":"Stop","background_tasks":[{"type":"in_process_teammate","status":"running","isIdle":true},{"type":"in_process_teammate","status":"running","isIdle":false}]}"#;
    let event = parse_remote_envelope(raw, Some(3)).unwrap();
    assert_eq!(event.active_background_tasks(), 1, "只有没闲置的那个算在跑");
}

/// 终态词表必须覆盖真实取值：`success` / `exited` 是从本机 claude 2.1.270 里
/// 读到的字面量，漏掉它们会让 pane 永远停在「工作中」——完成通知不再弹，
/// `agent.delegate` 的完成回调也不会触发，比早弹一条更难发现。
#[test]
fn finished_background_tasks_do_not_hold_the_turn_open() {
    for status in ["completed", "success", "exited", "failed", "killed", "cancelled", "idle"] {
        let payload = serde_json::json!({
            "session_id": "s",
            "hook_event_name": "Stop",
            "background_tasks": [{ "type": "local_bash", "status": status }],
        });
        let raw = format!("nebula-hook/1 source=claude pane=3\n{payload}");
        let event = parse_remote_envelope(raw.as_bytes(), Some(3)).unwrap();
        assert_eq!(event.active_background_tasks(), 0, "status={status} 是终态");
        assert_eq!(event.background_tasks.unwrap().total, 1, "它仍然算一笔任务");
    }
}

/// 两个形状陷阱：`{"type": null}` 不是任务（不能凭空造出一笔在飞的活儿），
/// `isIdle` 只对 teammate 有约定（别拿它压掉一笔在跑的 `local_bash`）。
#[test]
fn malformed_task_shapes_do_not_manufacture_in_flight_work() {
    let raw = br#"nebula-hook/1 source=claude pane=3
{"session_id":"s","hook_event_name":"Stop","background_tasks":[{"type":null},{"type":"local_bash","status":"running","isIdle":true}]}"#;
    let event = parse_remote_envelope(raw, Some(3)).unwrap();
    assert_eq!(event.active_background_tasks(), 1, "isIdle 不适用于 local_bash");
    assert_eq!(event.background_tasks.unwrap().total, 1, "null 型别不算任务");
}

#[test]
fn distinct_original_answers_are_not_swallowed_by_preview_deduplication() {
    for provider in ["claude", "codex"] {
        let mut gate = AiHookEventGate::default();
        for ending in ["first", "second"] {
            let answer = format!("{}{ending}", "shared preview ".repeat(500));
            let payload = if provider == "claude" {
                serde_json::json!({"hook_event_name": "Stop", "session_id": "session", "last_assistant_message": answer})
            } else {
                serde_json::json!({"type": "agent-turn-complete", "thread-id": "session", "last-assistant-message": answer})
            };
            let raw = format!("nebula-hook/1 source={provider} pane=3\n{payload}");
            let event = parse_remote_envelope(raw.as_bytes(), Some(3)).unwrap();
            assert!(gate.accept(&event, 3));
        }
    }
}

#[test]
fn event_gate_rejects_duplicates_and_out_of_order_provider_events() {
    let mut gate = AiHookEventGate::default();
    let done = parse_remote_envelope(
        b"nebula-hook/1 source=pi pane=3\n{\"kind\":\"done\",\"session_id\":\"s\",\"bridge_sequence\":3,\"event_id\":\"s:3\"}",
        Some(3),
    )
    .unwrap();
    assert!(gate.accept(&done, 3));
    assert!(!gate.accept(&done, 3), "same event id must be idempotent");

    let next_done = parse_remote_envelope(
        b"nebula-hook/1 source=pi pane=3\n{\"kind\":\"done\",\"session_id\":\"s\",\"bridge_sequence\":4,\"event_id\":\"s:4\"}",
        Some(3),
    )
    .unwrap();
    assert!(
        gate.accept(&next_done, 3),
        "a newer sequence must not be swallowed by the short duplicate window"
    );

    let late_prompt = parse_remote_envelope(
        b"nebula-hook/1 source=pi pane=3\n{\"kind\":\"prompt\",\"session_id\":\"s\",\"bridge_sequence\":2,\"event_id\":\"s:2\"}",
        Some(3),
    )
    .unwrap();
    assert!(!gate.accept(&late_prompt, 3));
}

#[test]
fn event_gate_keeps_sessions_isolated_and_session_end_terminal() {
    let mut gate = AiHookEventGate::default();
    let ended = parse_remote_envelope(
        b"nebula-hook/1 source=pi pane=7\n{\"kind\":\"session-end\",\"session_id\":\"old\",\"bridge_sequence\":5}",
        Some(7),
    )
    .unwrap();
    assert!(gate.accept(&ended, 7));

    let revive = parse_remote_envelope(
        b"nebula-hook/1 source=pi pane=7\n{\"kind\":\"prompt\",\"session_id\":\"old\",\"bridge_sequence\":6}",
        Some(7),
    )
    .unwrap();
    assert!(!gate.accept(&revive, 7));

    let new_session = parse_remote_envelope(
        b"nebula-hook/1 source=pi pane=7\n{\"kind\":\"prompt\",\"session_id\":\"new\",\"bridge_sequence\":1}",
        Some(7),
    )
    .unwrap();
    assert!(gate.accept(&new_session, 7));
}

#[test]
fn blocked_can_resume_but_unordered_done_cannot_regress() {
    let mut gate = AiHookEventGate::default();
    let attention = parse_remote_envelope(
        b"nebula-hook/1 source=claude pane=4\n{\"hook_event_name\":\"PermissionRequest\",\"session_id\":\"s\",\"tool_name\":\"Bash\"}",
        Some(4),
    )
    .unwrap();
    let tool = parse_remote_envelope(
        b"nebula-hook/1 source=claude pane=4\n{\"hook_event_name\":\"PostToolUse\",\"session_id\":\"s\"}",
        Some(4),
    )
    .unwrap();
    let done = parse_remote_envelope(
        b"nebula-hook/1 source=claude pane=4\n{\"hook_event_name\":\"Stop\",\"session_id\":\"s\"}",
        Some(4),
    )
    .unwrap();

    assert!(gate.accept(&attention, 4));
    assert!(gate.accept(&tool, 4), "permission continuation must resume working");
    assert!(gate.accept(&done, 4));
    assert!(!gate.accept(&tool, 4), "late unordered tool event must not overwrite Done");
}

/// 每一种拦截都要说得出是哪条规则——「通知没出现」事后唯一的线索就是这个
/// 原因，一个光秃秃的 bool 等于没有线索。
#[test]
fn gate_reports_which_rule_dropped_the_event() {
    let event = |json: &str| {
        let raw = format!("nebula-hook/1 source=pi pane=3\n{json}");
        parse_remote_envelope(raw.as_bytes(), Some(3)).unwrap()
    };
    let mut gate = AiHookEventGate::default();

    let done = event(
        "{\"kind\":\"done\",\"session_id\":\"s\",\"bridge_sequence\":3,\"event_id\":\"s:3\"}",
    );
    assert_eq!(gate.verdict(&done, 3), GateVerdict::Accepted);
    assert_eq!(gate.verdict(&done, 3), GateVerdict::DuplicateEventId);

    let older = event(
        "{\"kind\":\"prompt\",\"session_id\":\"s\",\"bridge_sequence\":2,\"event_id\":\"s:2\"}",
    );
    assert_eq!(gate.verdict(&older, 3), GateVerdict::StaleSequence);

    // 无序号但带 provider 时间戳：走时间判据。
    let mut timed = AiHookEventGate::default();
    let newer = event("{\"kind\":\"done\",\"session_id\":\"t\",\"occurred_at_ms\":2000}");
    let earlier = event("{\"kind\":\"prompt\",\"session_id\":\"t\",\"occurred_at_ms\":1000}");
    assert_eq!(timed.verdict(&newer, 3), GateVerdict::Accepted);
    assert_eq!(timed.verdict(&earlier, 3), GateVerdict::StaleTime);

    // SessionEnd 之后只有 SessionStart 能复活。
    let mut ended = AiHookEventGate::default();
    assert_eq!(
        ended.verdict(&event("{\"kind\":\"session-end\",\"session_id\":\"u\"}"), 3),
        GateVerdict::Accepted
    );
    assert_eq!(
        ended.verdict(&event("{\"kind\":\"prompt\",\"session_id\":\"u\"}"), 3),
        GateVerdict::AfterSessionEnd
    );

    // Done 之后无证据的 ToolComplete 不能把完成态倒退回运行中。
    let mut finished = AiHookEventGate::default();
    assert_eq!(
        finished.verdict(&event("{\"kind\":\"done\",\"session_id\":\"v\"}"), 3),
        GateVerdict::Accepted
    );
    assert_eq!(
        finished.verdict(&event("{\"kind\":\"tool-complete\",\"session_id\":\"v\"}"), 3),
        GateVerdict::UnorderedAfterDone
    );
}

#[test]
fn sequenced_batch_reorders_each_stream_without_mixing_streams() {
    fn event(session: &str, sequence: u64) -> super::AiHookEvent {
        let raw = format!(
            "nebula-hook/1 source=pi pane=3\n{{\"kind\":\"prompt\",\"session_id\":\"{session}\",\"bridge_sequence\":{sequence},\"event_id\":\"{session}:{sequence}\"}}"
        );
        parse_remote_envelope(raw.as_bytes(), Some(3)).unwrap()
    }

    let ordered = reorder_batch(vec![event("a", 3), event("b", 2), event("a", 1), event("b", 1)]);
    let sequence = ordered
        .iter()
        .map(|event| (event.session_id.as_deref().unwrap(), event.bridge_sequence.unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(sequence, vec![("a", 1), ("b", 1), ("a", 3), ("b", 2)]);
}

#[test]
fn late_pi_event_cannot_restore_the_previous_native_session_after_a_switch() {
    let event = |id: &str, sequence: u64| {
        let body = serde_json::json!({"kind":"session-start", "session_id":id,
            "bridge_instance":"same-process", "bridge_sequence":sequence});
        parse_remote_envelope(format!("nebula-hook/1 source=pi pane=7\n{body}").as_bytes(), Some(7))
            .unwrap()
    };
    let mut gate = AiHookEventGate::default();
    assert!(gate.accept(&event("first", 1), 7));
    assert!(gate.accept(&event("second", 3), 7));
    assert_eq!(gate.verdict(&event("first", 2), 7), GateVerdict::StaleSequence);
    let mut next_process = event("third", 1);
    next_process.bridge_instance = Some("next-process".into());
    assert!(gate.accept(&next_process, 7));
}

#[test]
fn kimi_events_map_to_the_shared_lifecycle() {
    // Stop 是回合终态，且必须带上 kimi 的 session_id 供冷恢复使用。
    let raw = b"nebula-hook/1 source=kimi pane=3\n{\"hook_event_name\":\"Stop\",\"session_id\":\"kimi-session-1\",\"session_title\":\"fix bug\",\"client_type\":\"kimi_code_cli\",\"cwd\":\"D:/work\"}";
    let event = parse_remote_envelope(raw, Some(3)).unwrap();
    assert_eq!(event.kind, AiHookKind::TurnDone);
    assert_eq!(event.session_id.as_deref(), Some("kimi-session-1"));

    // Interrupt（用户按 Esc 代替 Stop 触发）与 StopFailure（出错结束）同样
    // 是回合终态：漏掉任何一个，pane 都会永远卡在 Working。
    for name in ["Interrupt", "StopFailure"] {
        let raw = format!("nebula-hook/1 source=kimi pane=3\n{{\"hook_event_name\":\"{name}\"}}");
        let event = parse_remote_envelope(raw.as_bytes(), Some(3)).unwrap();
        assert_eq!(event.kind, AiHookKind::TurnDone, "{name} must end the turn");
    }

    for (name, expected) in [
        ("SessionStart", AiHookKind::SessionStart),
        ("UserPromptSubmit", AiHookKind::PromptSubmit),
        ("PermissionResult", AiHookKind::ToolComplete),
        ("SessionEnd", AiHookKind::SessionEnd),
    ] {
        let raw = format!("nebula-hook/1 source=kimi pane=3\n{{\"hook_event_name\":\"{name}\"}}");
        let event = parse_remote_envelope(raw.as_bytes(), Some(3)).unwrap();
        assert_eq!(event.kind, expected, "{name}");
    }

    // PermissionRequest 是「现在需要用户」：工具名经通用字段表提取，cwd
    // 进入 attention 上下文供通知摘要定位。
    let raw = b"nebula-hook/1 source=kimi pane=9\n{\"hook_event_name\":\"PermissionRequest\",\"session_id\":\"s\",\"tool_name\":\"Bash\",\"cwd\":\"D:/work/kimi\"}";
    let event = parse_remote_envelope(raw, Some(9)).unwrap();
    assert_eq!(event.kind, AiHookKind::NeedsAttention);
    let context = event.attention.as_ref().unwrap();
    assert_eq!(context.permission_or_tool.as_deref(), Some("Bash"));
    assert_eq!(context.cwd.as_deref(), Some("D:/work/kimi"));
    assert_eq!(context.pane_id, Some(9));
}

/// 心跳与后台任务通知都必须丢弃：SessionHeartbeat 约 60s 一次，放行会让
/// pane 状态被无意义事件反复触碰；Notification 是后台任务状态变更，不是
/// 「等你输入」，映射成 NeedsAttention 会点亮一个清不掉的等待徽标。
#[test]
fn kimi_heartbeat_and_notification_are_dropped() {
    for name in ["SessionHeartbeat", "Notification", "TurnStarted", "UserPromptQueued"] {
        let raw = format!("nebula-hook/1 source=kimi pane=3\n{{\"hook_event_name\":\"{name}\"}}");
        assert!(parse_remote_envelope(raw.as_bytes(), Some(3)).is_none(), "{name} must be dropped");
    }
}

/// PermissionResult 与 claude PostToolUse 同角色：权限答复后把 Blocked
/// 拉回 Working，而不是等整个回合结束。
#[test]
fn kimi_permission_result_resumes_a_blocked_stream() {
    let mut gate = AiHookEventGate::default();
    let event = |name: &str| {
        let raw = format!(
            "nebula-hook/1 source=kimi pane=4\n{{\"hook_event_name\":\"{name}\",\"session_id\":\"s\"}}"
        );
        parse_remote_envelope(raw.as_bytes(), Some(4)).unwrap()
    };
    assert!(gate.accept(&event("PermissionRequest"), 4));
    assert!(gate.accept(&event("PermissionResult"), 4), "Blocked must resume working");
    assert!(gate.accept(&event("Stop"), 4));
}

#[test]
fn kimi_capabilities_are_declared() {
    let capabilities = capabilities_for("kimi");
    assert!(capabilities.attention_context);
    assert!(!capabilities.background_tasks);
    assert!(!capabilities.bridge_sequence);
    assert!(!capabilities.serialized_delivery);
}
