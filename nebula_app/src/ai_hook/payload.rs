//! Bounded extraction and redaction of structured hook payloads.

use super::AiBackgroundTasks;
use serde_json::Value;

pub(super) const MESSAGE_MAX_CHARS: usize = 300;
const CONTEXT_STRING_MAX_CHARS: usize = 1_024;
pub(super) const RAW_CONTEXT_MAX_BYTES: usize = 16 * 1_024;

fn context_container<'a>(payload: &'a Value, key: &str) -> Option<&'a Value> {
    payload.get(key).filter(|value| value.is_object())
}

fn context_value<'a>(payload: &'a Value, names: &[&str]) -> Option<&'a Value> {
    for container in [
        Some(payload),
        context_container(payload, "context"),
        context_container(payload, "payload"),
        context_container(payload, "properties"),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(value) = names.iter().find_map(|name| container.get(*name)) {
            return Some(value);
        }
    }
    None
}

pub(super) fn context_string(payload: &Value, names: &[&str]) -> Option<String> {
    let value = context_value(payload, names)?;
    let value = value
        .as_str()
        .or_else(|| value.get("name").and_then(Value::as_str))
        .or_else(|| value.get("type").and_then(Value::as_str))?;
    (!value.trim().is_empty()).then(|| truncate(value.trim(), CONTEXT_STRING_MAX_CHARS))
}

pub(super) fn context_u64(payload: &Value, names: &[&str]) -> Option<u64> {
    let value = context_value(payload, names)?;
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

pub(super) fn first_string_in_array(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_array)
        .and_then(|values| values.iter().find_map(Value::as_str))
        .map(|value| truncate(value, CONTEXT_STRING_MAX_CHARS))
}

pub(super) fn attention_message(payload: &Value) -> Option<String> {
    context_string(payload, &["message", "title", "reason", "description"])
        .map(|message| truncate(&message, MESSAGE_MAX_CHARS))
}

/// Explicit blocking notification types and supported compatibility spellings.
/// Substrings such as "input" would also match nonblocking "input_completed".
const ACTIONABLE_ATTENTION_TYPES: &[&str] = &[
    "permission",
    "permission_prompt",
    "permission_request",
    "tool_permission",
    "tool_permission_request",
    "approval_request",
    "confirmation_request",
    "input_request",
    "awaiting_input",
    "elicitation_dialog",
];

/// Untyped legacy notifications remain parseable; the lifecycle only admits
/// them during an active turn. Typed idle/auth/result notifications never block.
/// Do not read `type`/`kind`: those carry other providers' lifecycle events.
pub(super) fn attention_is_actionable(payload: &Value) -> bool {
    let Some(kind) = context_string(payload, &["notificationType", "notification_type"]) else {
        return true;
    };
    let kind = kind.to_ascii_lowercase();
    ACTIONABLE_ATTENTION_TYPES.contains(&kind.as_str())
}

pub(super) fn background_task_summary(payload: &Value) -> Option<AiBackgroundTasks> {
    let tasks = payload.get("background_tasks")?;
    let mut active = 0u32;
    let mut total = 0u32;
    count_background_tasks(tasks, &mut active, &mut total);
    Some(AiBackgroundTasks { active, total })
}

/// 数出这批后台活儿里有多少还在跑。
///
/// 旧实现只认 `type == "subagent"`，其余类型一律跳过。而 Claude Code 给后台 bash
/// 的类型名是 `local_bash`（同族还有 `monitor` / `workflow` / `mcp_task` /
/// `in_process_teammate` …），于是「跑着一条后台命令、回合先结束」这个最常见、也
/// 最容易被误报成「任务完成」的形状，永远数出 `active = 0`：`TurnDone` 的守卫不
/// 触发，命令还在跑就先弹了完成通知。
///
/// `background_tasks` 本身就是「在飞」的集合——字段说明写着 *In-flight background
/// work … Empty array when nothing is in flight*，它的存在就是为了让 hook 区分
/// 「真的收工」和「在等后台活儿把自己叫醒」。所以这里不再按类型过滤，只看每笔的
/// 状态。
fn count_background_tasks(value: &Value, active: &mut u32, total: &mut u32) {
    match value {
        Value::Array(values) => {
            for value in values {
                count_background_tasks(value, active, total);
            }
        },
        Value::Object(task) => {
            if is_task_entry(task) {
                *total = total.saturating_add(1);
                if task_is_in_flight(task) {
                    *active = active.saturating_add(1);
                }
                return;
            }
            // 既没有类型也没有状态的中间层对象（旧 wire 形状的包装）继续往下找。
            for value in task.values() {
                count_background_tasks(value, active, total);
            }
        },
        _ => {},
    }
}

/// 任务条目的终态词表。
///
/// 取值不是猜的：本机 claude 2.1.270 二进制里出现过的 `status:"…"` 字面量是
/// `failed` / `success` / `pending` / `completed` / `running` / `killed` / `idle` /
/// `stopped` / `cancelled` / `aborted` / `exited`。这里取其中的终态，再补上同族词
/// （`done` / `finished` / `error` / `canceled` …）。
///
/// 漏掉终态会让 pane 一直等待后台任务；误把运行态加入这张表则会提前通知完成。
/// 缺失或未知状态按仍在运行处理，避免 provider 扩展类型时恢复提前完成的问题。
const TERMINAL_TASK_STATUSES: &[&str] = &[
    "completed",
    "complete",
    "done",
    "success",
    "succeeded",
    "finished",
    "exited",
    "failed",
    "failure",
    "error",
    "timeout",
    "timed_out",
    "expired",
    "terminated",
    "cancelled",
    "canceled",
    "stopped",
    "killed",
    "aborted",
    // 任务自己说 idle，就是没在干活——它仍留在集合里只是注册表还没清扫。
    "idle",
];

/// 这一笔是任务条目，还是需要继续往下找的中间层包装。
///
/// 只认**字符串**字段：`{"type": null}`、`{"type": {"name": …}}` 既不是任务也没有
/// 状态，放行给下一层递归（旧实现按 `as_str()` 判断，本次改动一度退化成
/// `is_some()`，会把一个 null 凭空当成一笔在飞任务）。
fn is_task_entry(task: &serde_json::Map<String, Value>) -> bool {
    task.get("type").and_then(Value::as_str).is_some()
        || task.get("status").and_then(Value::as_str).is_some()
}

/// 这一笔后台活儿是否还在跑。
///
/// 读不到状态就按还在跑处理：这个集合本身就是「在飞」的任务。
fn task_is_in_flight(task: &serde_json::Map<String, Value>) -> bool {
    if is_idle_teammate(task) {
        return false;
    }
    match task.get("status").and_then(Value::as_str) {
        Some(status) => !TERMINAL_TASK_STATUSES.contains(&status.to_ascii_lowercase().as_str()),
        None => true,
    }
}

/// 闲置的 `in_process_teammate`：`status` 会一直挂着 `running`
/// （anthropics/claude-code#85955），只有它自己的 `isIdle` 说得准——这也正是
/// Claude Code 内部的判据（`type === "in_process_teammate" && status ===
/// "running" && !isIdle`）。
///
/// **只对 teammate 生效**：别的类型没有 `isIdle` 的约定，拿它压掉一笔在跑的
/// `local_bash` 就是谎报完成，正好是这次要修的那个毛病。
fn is_idle_teammate(task: &serde_json::Map<String, Value>) -> bool {
    task.get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("in_process_teammate"))
        && task.get("isIdle").and_then(Value::as_bool) == Some(true)
}

pub(super) fn sanitized_raw_context(payload: &Value) -> Option<String> {
    let sanitized = sanitize_json(payload, 0);
    let raw = serde_json::to_string(&sanitized).ok()?;
    if raw == "null" || raw == "{}" {
        return None;
    }
    if raw.len() <= RAW_CONTEXT_MAX_BYTES {
        return Some(raw);
    }
    let mut limited = truncate_utf8_bytes(&raw, RAW_CONTEXT_MAX_BYTES.saturating_sub(14));
    limited.push_str("...[truncated]");
    Some(limited)
}

fn sanitize_json(value: &Value, depth: usize) -> Value {
    if depth >= 6 {
        return Value::String("[depth limited]".to_owned());
    }
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(value) => Value::String(truncate(value, CONTEXT_STRING_MAX_CHARS)),
        Value::Array(values) => Value::Array(
            values.iter().take(24).map(|value| sanitize_json(value, depth + 1)).collect(),
        ),
        Value::Object(values) => {
            let mut sanitized = serde_json::Map::new();
            for (key, value) in values.iter().take(48) {
                let value = if sensitive_context_key(key) {
                    Value::String("[redacted]".to_owned())
                } else {
                    sanitize_json(value, depth + 1)
                };
                sanitized.insert(key.clone(), value);
            }
            Value::Object(sanitized)
        },
    }
}

fn sensitive_context_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    [
        "token",
        "secret",
        "password",
        "passwd",
        "authorization",
        "cookie",
        "credential",
        "apikey",
        "accesskey",
        "privatekey",
        "environment",
        "env",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn truncate_utf8_bytes(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut boundary = max_bytes.min(value.len());
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

/// Char-boundary-safe cut with an ellipsis (toast bodies are small).
pub(super) fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_owned();
    }
    let cut: String = s.chars().take(max_chars).collect();
    format!("{cut}…")
}
