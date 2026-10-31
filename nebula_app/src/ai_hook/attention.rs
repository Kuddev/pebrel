//! Interaction details from structured provider facts, never from message prose.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::AiPermissionMode;
use super::payload::context_string;
use super::protocol::claude_style;

/// Refines attention without changing the pane's lifecycle state or authorizing input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionKind {
    Permission,
    Selection,
    Confirmation,
    Input,
    #[default]
    Unknown,
}

/// The same exact notification vocabulary controls admission and classification.
pub(super) fn notification_kind(kind: &str) -> Option<AttentionKind> {
    match kind {
        "permission"
        | "permission_prompt"
        | "permission_request"
        | "tool_permission"
        | "tool_permission_request"
        | "approval_request" => Some(AttentionKind::Permission),
        "confirmation_request" => Some(AttentionKind::Confirmation),
        "input_request" | "awaiting_input" | "elicitation_dialog" => Some(AttentionKind::Input),
        _ => None,
    }
}

pub(super) fn from_payload(
    source: &str,
    payload: &Value,
    permission_mode: Option<AiPermissionMode>,
) -> AttentionKind {
    let event = payload.get("hook_event_name").and_then(Value::as_str);
    let tool = payload.get("tool_name").and_then(Value::as_str);
    let question = event == Some("PreToolUse")
        && (claude_style(source) && tool == Some("AskUserQuestion")
            || source == "codex" && tool == Some("request_user_input"));
    if question {
        let has_choices = payload
            .get("tool_input")
            .and_then(|input| input.get("questions"))
            .and_then(Value::as_array)
            .is_some_and(|questions| {
                questions.iter().any(|question| {
                    question.get("options").and_then(Value::as_array).is_some_and(|v| !v.is_empty())
                })
            });
        return if has_choices { AttentionKind::Selection } else { AttentionKind::Input };
    }
    if claude_style(source) && event == Some("PermissionRequest") && tool == Some("ExitPlanMode") {
        return AttentionKind::Confirmation;
    }
    if event == Some("PermissionRequest") || source == "opencode" {
        // The installed OpenCode bridge emits attention only from permission requests.
        return permission_kind(permission_mode);
    }
    let names = if matches!(source, "cursor" | "copilot" | "grok" | "antigravity") {
        &["notificationType", "notification_type", "type"][..]
    } else {
        &["notificationType", "notification_type"][..]
    };
    let kind = context_string(payload, names)
        .and_then(|kind| notification_kind(&kind.to_ascii_lowercase()))
        .unwrap_or_default();
    if kind == AttentionKind::Permission { permission_kind(permission_mode) } else { kind }
}

fn permission_kind(mode: Option<AiPermissionMode>) -> AttentionKind {
    // Bypass sessions can still ask questions, but cannot be waiting for approval.
    if mode == Some(AiPermissionMode::BypassPermissions) {
        AttentionKind::Unknown
    } else {
        AttentionKind::Permission
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_hook::parse_remote_envelope;

    fn kind(source: &str, payload: Value) -> AttentionKind {
        let wire = format!("nebula-hook/1 source={source} codex_hooks=full\n{payload}");
        parse_remote_envelope(wire.as_bytes(), Some(1)).unwrap().attention.unwrap().kind
    }

    #[test]
    fn structured_requests_distinguish_approval_plans_choices_and_free_input() {
        for (source, tool) in [("claude", "AskUserQuestion"), ("codex", "request_user_input")] {
            let mut question = serde_json::json!({
                "hook_event_name": "PreToolUse", "tool_name": tool,
                "tool_input": {"questions": [{"question": "Choose a scope", "options": [{"label": "Local"}, {"label": "All"}]}]}
            });
            assert_eq!(kind(source, question.clone()), AttentionKind::Selection);
            question["tool_input"]["questions"][0]["options"] = serde_json::json!([]);
            assert_eq!(kind(source, question), AttentionKind::Input);
            assert_eq!(
                kind(
                    source,
                    serde_json::json!({"hook_event_name":"PermissionRequest", "tool_name":"Bash"})
                ),
                AttentionKind::Permission
            );
        }
        assert_eq!(
            kind(
                "claude",
                serde_json::json!({"hook_event_name":"PermissionRequest", "tool_name":"ExitPlanMode"})
            ),
            AttentionKind::Confirmation
        );
        assert_eq!(
            kind("opencode", serde_json::json!({"kind":"attention"})),
            AttentionKind::Permission
        );
    }

    #[test]
    fn notification_details_do_not_guess_from_text_or_selected_text() {
        for (notification, expected) in [
            ("permission_prompt", AttentionKind::Permission),
            ("confirmation_request", AttentionKind::Confirmation),
            ("elicitation_dialog", AttentionKind::Input),
        ] {
            assert_eq!(
                kind(
                    "claude",
                    serde_json::json!({"hook_event_name":"Notification", "notification_type":notification})
                ),
                expected
            );
        }
        assert_eq!(
            kind(
                "claude",
                serde_json::json!({"hook_event_name":"Notification", "message":"Allow permission? Choose a plan", "selection":"selected document"})
            ),
            AttentionKind::Unknown
        );
        assert_eq!(
            kind(
                "claude",
                serde_json::json!({"hook_event_name":"PermissionRequest", "permission_mode":"bypassPermissions"})
            ),
            AttentionKind::Unknown
        );
        assert_eq!(notification_kind("input_completed"), None);
        assert_eq!(
            kind(
                "claude",
                serde_json::json!({"hook_event_name":"Notification", "notification_type":"elicitation_dialog", "permission_mode":"bypassPermissions"})
            ),
            AttentionKind::Input
        );
    }
}
