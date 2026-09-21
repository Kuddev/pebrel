//! Local/SSH wire envelope parsing and provider normalization.

use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::payload::*;
use super::{
    AiHookEvent, AiHookKind, AiPermissionMode, AiTurnOutcome, AttentionContext, CodexHookMode,
};

const ID_MAX_CHARS: usize = 512;
const TURN_RESULT_MAX_CHARS: usize = 4_000;
static RECEIVE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Parse one pipe message: a `nebula-hook/1 source=<s> pane=<n>` header line,
/// then the hook's raw JSON payload verbatim (the helper never re-encodes;
/// all JSON work happens here, off the turn's hot path).
pub(super) fn parse_envelope(bytes: &[u8]) -> Option<AiHookEvent> {
    let received_at_ms = unix_time_ms();
    let received_sequence = RECEIVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nl = bytes.iter().position(|&b| b == b'\n')?;
    let header = std::str::from_utf8(&bytes[..nl]).ok()?.trim();
    let raw = &bytes[nl + 1..];

    let mut fields = header.split_whitespace();
    if fields.next() != Some("nebula-hook/1") {
        return None;
    }
    let (mut source, mut pane, mut codex_mode) = (None, None, None);
    for field in fields {
        match field.split_once('=') {
            Some(("source", v)) => source = Some(v.to_owned()),
            Some(("pane", v)) => pane = v.parse().ok(),
            Some(("codex_hooks", "full")) => codex_mode = Some(CodexHookMode::Full),
            Some(("codex_hooks", "turns")) => codex_mode = Some(CodexHookMode::Turns),
            _ => (),
        }
    }
    let source = source?;

    let payload: Value = serde_json::from_slice(raw).unwrap_or(Value::Null);
    let native_codex = source == "codex" && payload.get("hook_event_name").is_some();
    // An event name alone cannot prove that a native lifecycle set is installed.
    let codex_hooks = if native_codex { Some(codex_mode?) } else { None };
    if let Some(mode) = codex_hooks
        && !mode.events().contains(&payload.get("hook_event_name")?.as_str()?)
    {
        return None;
    }
    if native_codex
        && payload.get("agent_id").and_then(Value::as_str).is_some_and(|id| !id.is_empty())
    {
        // Regular tool hooks can also run inside a thread-spawned subagent.
        // Codex labels that context explicitly, even in the same OS process.
        return None;
    }
    // 会话身份的候选字段名按 source 收紧。claude 每个 hook 载荷都带
    // `session_id`（snake_case）；codex notify 带 `thread-id`（kebab-case，即
    // rollout uuid）。**claude 分支绝不读 camelCase**：那是别家 hook runner 的
    // 写法，把它的 session id 当成 claude 的，就会把一个不存在的会话交给
    // `claude --resume`（见 nebula_hook 的 FOREIGN_HOOK_RUNNERS 注释）。
    let session_id_keys: &[&str] = match source.as_str() {
        "claude" | "kimi" => &["session_id"],
        "codex" if native_codex => &["session_id"],
        "codex" => &["thread-id"],
        // opencode/pi 由我们自己的 bridge 规范化成 snake_case；camelCase 是
        // provider SDK 原样透传时的兼容路径。
        _ => &["session_id", "sessionID", "sessionId"],
    };
    let session_id = session_id_keys
        .iter()
        .find_map(|key| payload.get(*key))
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(|id| truncate(id, ID_MAX_CHARS));
    let mut event_id =
        context_string(&payload, &["event_id", "eventId"]).map(|id| truncate(&id, ID_MAX_CHARS));
    let turn_id =
        context_string(&payload, &["turn_id", "turn-id"]).map(|id| truncate(&id, ID_MAX_CHARS));
    // 旧字段名保留：用户机器上可能还装着上一版 bridge，它写的是
    // `provider_sequence`。字段来源始终是 bridge，不是 provider 原生顺序。
    let bridge_sequence = context_u64(
        &payload,
        &["bridge_sequence", "bridgeSequence", "provider_sequence", "providerSequence"],
    );
    let occurred_at_ms =
        context_u64(&payload, &["occurred_at_ms", "occurredAtMs", "occurred_at", "timestamp"]);
    let background_tasks = background_task_summary(&payload);
    let permission_mode = context_string(&payload, &["permission_mode", "permissionMode"])
        .as_deref()
        .and_then(AiPermissionMode::parse);
    let (kind, message) = match source.as_str() {
        // 第二道串台门。第一道在 nebula_hook 里靠环境变量判断调用方是不是别家
        // 的 hook runner；那种门会被上游改名静默失效，所以这里独立再拦一次：
        // 别家 runner 的载荷用 camelCase 字段名，claude 从不这样发。
        "claude" if payload.get("hookEventName").is_some() => return None,
        "claude" => match payload.get("hook_event_name").and_then(Value::as_str) {
            Some("SessionStart") => (AiHookKind::SessionStart, None),
            Some("UserPromptSubmit") => (AiHookKind::PromptSubmit, None),
            Some("PreToolUse")
                if payload.get("tool_name").and_then(Value::as_str) == Some("AskUserQuestion") =>
            {
                (AiHookKind::NeedsAttention, attention_message(&payload))
            },
            Some("PreToolUse" | "PostToolUse" | "PostToolUseFailure") => {
                (AiHookKind::ToolComplete, None)
            },
            Some("Stop") => (AiHookKind::TurnDone, None),
            Some("StopFailure") => (AiHookKind::TurnDone, context_string(&payload, &["error"])),
            Some("SessionEnd") => (AiHookKind::SessionEnd, None),
            // `Notification` 覆盖「权限询问」和「idle 提醒」两类，将来也可能
            // 用来传别的东西。类型不可操作时丢掉，读不到类型时照常上报。
            Some("Notification") if !attention_is_actionable(&payload) => return None,
            Some("Notification") | Some("PermissionRequest") => {
                (AiHookKind::NeedsAttention, attention_message(&payload))
            },
            // SubagentStop and friends would only produce noise.
            _ => return None,
        },
        "codex" if native_codex => match payload.get("hook_event_name").and_then(Value::as_str) {
            Some("SessionStart") => (AiHookKind::SessionStart, None),
            Some("UserPromptSubmit") => (AiHookKind::PromptSubmit, None),
            Some("PermissionRequest") => (AiHookKind::NeedsAttention, attention_message(&payload)),
            Some("PreToolUse")
                if payload.get("tool_name").and_then(Value::as_str)
                    == Some("request_user_input") =>
            {
                let question = payload
                    .get("tool_input")?
                    .get("questions")?
                    .as_array()?
                    .iter()
                    .find_map(|question| question.get("question").and_then(Value::as_str))?
                    .trim();
                if question.is_empty() {
                    return None;
                }
                (AiHookKind::NeedsAttention, Some(truncate(question, MESSAGE_MAX_CHARS)))
            },
            Some("PostToolUse") => (AiHookKind::ToolComplete, None),
            Some("Stop") => {
                (AiHookKind::TurnDone, context_string(&payload, &["last_assistant_message"]))
            },
            Some("Interrupt") => (AiHookKind::TurnDone, None),
            Some("SessionEnd") => (AiHookKind::SessionEnd, None),
            // Subagent events share the parent session_id. They cannot finish
            // or replace the main turn, even when the provider uses one PID.
            _ => return None,
        },
        "codex" => match payload.get("type").and_then(Value::as_str) {
            Some("agent-turn-complete") => (
                AiHookKind::TurnDone,
                payload
                    .get("last-assistant-message")
                    .and_then(Value::as_str)
                    .map(|m| truncate(m, TURN_RESULT_MAX_CHARS)),
            ),
            _ => return None,
        },
        // Kimi's explicit waiting and result events share the pane lifecycle.
        // Heartbeats and background notifications never request user input.
        "kimi" => match payload.get("hook_event_name").and_then(Value::as_str) {
            Some("SessionStart") => (AiHookKind::SessionStart, None),
            Some("UserPromptSubmit") => (AiHookKind::PromptSubmit, None),
            Some("Stop") | Some("Interrupt") => (AiHookKind::TurnDone, None),
            Some("StopFailure") => (AiHookKind::TurnDone, context_string(&payload, &["error"])),
            Some("PermissionRequest") => (AiHookKind::NeedsAttention, attention_message(&payload)),
            Some("PermissionResult") => (AiHookKind::ToolComplete, None),
            Some("SessionEnd") => (AiHookKind::SessionEnd, None),
            _ => return None,
        },
        // opencode's Bun plugin normalizes its event bus into a tiny
        // `{"kind":"prompt|done|attention","message":?}` payload (see the
        // embedded plugin in `ensure_opencode_plugin`), so this side stays
        // decoupled from opencode's evolving SDK event schema.
        "opencode" | "pi" => match payload.get("kind").and_then(Value::as_str) {
            Some("session-start") => (AiHookKind::SessionStart, None),
            Some("prompt") => (AiHookKind::PromptSubmit, None),
            Some("tool-complete") => (AiHookKind::ToolComplete, None),
            Some("done") => (AiHookKind::TurnDone, context_string(&payload, &["message"])),
            Some("session-end") => (AiHookKind::SessionEnd, None),
            Some("attention") => (AiHookKind::NeedsAttention, attention_message(&payload)),
            _ => return None,
        },
        _ => return None,
    };
    if source == "codex" && kind == AiHookKind::TurnDone && event_id.is_none() {
        event_id = turn_id.as_ref().map(|id| format!("codex:turn:{id}:done"));
    }
    let turn_outcome = if matches!(source.as_str(), "claude" | "kimi")
        && kind == AiHookKind::TurnDone
    {
        match payload.get("hook_event_name").and_then(Value::as_str) {
            Some("Stop") => AiTurnOutcome::Succeeded,
            Some("Interrupt") => AiTurnOutcome::Cancelled,
            Some("StopFailure") => AiTurnOutcome::Failed,
            _ => AiTurnOutcome::Unknown,
        }
    } else if native_codex
        && payload.get("hook_event_name").and_then(Value::as_str) == Some("Interrupt")
    {
        AiTurnOutcome::Cancelled
    } else if source == "pi" && kind == AiHookKind::TurnDone {
        match payload.get("stop_reason").and_then(Value::as_str) {
            Some("stop") => AiTurnOutcome::Succeeded,
            Some("error") => AiTurnOutcome::Failed,
            Some("aborted") => AiTurnOutcome::Cancelled,
            Some("length" | "toolUse") => AiTurnOutcome::Incomplete,
            None if !payload.as_object()?.contains_key("stop_reason") => AiTurnOutcome::Unspecified,
            _ => AiTurnOutcome::Unknown,
        }
    } else {
        AiTurnOutcome::Unknown
    };
    let attention = (kind == AiHookKind::NeedsAttention).then(|| AttentionContext {
        source: source.clone(),
        pane_id: pane,
        session_id: session_id.clone(),
        event_kind: kind,
        event_id: event_id.clone(),
        bridge_sequence,
        occurred_at_ms,
        received_at_ms,
        cwd: context_string(&payload, &["cwd", "directory"]),
        project: context_string(
            &payload,
            &["project", "project_path", "projectPath", "workspace_root", "workspaceRoot"],
        )
        .or_else(|| first_string_in_array(&payload, "workspace_roots")),
        git_branch: context_string(&payload, &["git_branch", "gitBranch", "branch"]),
        permission_or_tool: context_string(
            &payload,
            &[
                "permission_or_tool",
                "permissionOrTool",
                "permission_type",
                "permissionType",
                "tool_name",
                "toolName",
                "tool",
            ],
        )
        .or_else(|| {
            payload
                .get("hook_event_name")
                .and_then(Value::as_str)
                .filter(|name| *name == "PermissionRequest")
                .map(str::to_owned)
        }),
        permission_mode,
        message: message.clone(),
        selection: context_string(&payload, &["selection", "selected_text", "selectedText"]),
        raw_context: sanitized_raw_context(&payload),
    });
    let answer = crate::assistant_answer::AssistantAnswer::from_hook(&source, &payload);
    let answer_cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|path| path.len() <= 4096 && !path.chars().any(char::is_control))
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute());
    Some(AiHookEvent {
        codex_hooks,
        remote_process: None,
        turn_id,
        session_compacted: kind == AiHookKind::SessionStart
            && payload.get("source").and_then(Value::as_str) == Some("compact"),
        legacy_attention: source == "claude"
            && payload.get("hook_event_name").and_then(Value::as_str) == Some("Notification")
            && context_string(&payload, &["notificationType", "notification_type"]).is_none(),
        answer,
        answer_cwd,
        pane,
        source,
        kind,
        turn_outcome,
        message,
        session_id,
        session_file: payload
            .get("session_file")
            .and_then(Value::as_str)
            .filter(|path| crate::session::valid_native_session_file(path))
            .map(str::to_owned),
        bridge_instance: payload
            .get("bridge_instance")
            .and_then(Value::as_str)
            .filter(|id| {
                id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
            .map(str::to_owned),
        event_id,
        bridge_sequence,
        occurred_at_ms,
        received_at_ms,
        received_sequence,
        // 只有命名管道的服务端能从内核问出来；解析阶段一律留空。
        client_pid: None,
        agent_pid: None,
        permission_mode,
        background_tasks,
        attention,
    })
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
}

/// 远端会话只能提交事件语义，Pane 身份始终由本地 PTY 通道覆盖，
/// 防止远端载荷把通知路由到同一窗口中的其他标签页。
pub(crate) fn parse_remote_envelope(bytes: &[u8], pane: Option<u64>) -> Option<AiHookEvent> {
    let mut event = parse_envelope(bytes)?;
    // Only this entrypoint is reached after SSH/WSL token verification. Ignore
    // claimed process metadata on the local named-pipe path.
    let header = std::str::from_utf8(bytes.split(|byte| *byte == b'\n').next()?).ok()?;
    event.remote_process = header.split_whitespace().find_map(|field| {
        let key = field.strip_prefix("process=")?;
        (key.len() <= 128
            && !key.is_empty()
            && key.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b':')))
        .then(|| key.to_owned())
    });
    event.pane = pane;
    if let Some(attention) = event.attention.as_mut() {
        attention.pane_id = pane;
    }
    Some(event)
}
