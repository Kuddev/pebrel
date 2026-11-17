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
    native_kind: Option<AttentionKind>,
) -> AttentionKind {
    if let Some(kind) = native_kind {
        // Native admission has already selected the category and its field
        // precedence. Unrelated hook fields cannot reinterpret that event.
        return if kind == AttentionKind::Permission {
            permission_kind(permission_mode)
        } else {
            kind
        };
    }
    let event = payload.get("hook_event_name").and_then(Value::as_str);
    let tool = payload.get("tool_name").and_then(Value::as_str);
    let question = event == Some("PreToolUse")
        && (claude_style(source) && tool == Some("AskUserQuestion")
            || source == "codex" && tool == Some("request_user_input"));
    if question {
        return question_kind(payload);
    }
    if claude_style(source) && event == Some("PermissionRequest") && tool == Some("ExitPlanMode") {
        return AttentionKind::Confirmation;
    }
    if event == Some("PermissionRequest") || source == "opencode" {
        // The installed OpenCode bridge emits attention only from permission requests.
        return permission_kind(permission_mode);
    }
    let kind = context_string(payload, &["notificationType", "notification_type"])
        .and_then(|kind| notification_kind(&kind.to_ascii_lowercase()))
        .unwrap_or_default();
    if kind == AttentionKind::Permission { permission_kind(permission_mode) } else { kind }
}

fn question_kind(payload: &Value) -> AttentionKind {
    let Some(questions) = payload
        .get("tool_input")
        .and_then(|input| input.get("questions"))
        .and_then(Value::as_array)
        .filter(|questions| !questions.is_empty())
    else {
        return AttentionKind::Unknown;
    };
    let mut kind = AttentionKind::Input;
    for question in questions {
        if !question
            .get("question")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
        {
            return AttentionKind::Unknown;
        }
        let Some(options) = question.get("options").and_then(Value::as_array) else {
            return AttentionKind::Unknown;
        };
        if options.is_empty() {
            continue; // An explicit empty options array establishes free input.
        }
        if !options.iter().all(|option| {
            option
                .get("label")
                .and_then(Value::as_str)
                .is_some_and(|label| !label.trim().is_empty())
        }) {
            return AttentionKind::Unknown;
        }
        kind = AttentionKind::Selection;
    }
    kind
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
        let untyped = b"nebula-hook/1 source=claude\n{\"hook_event_name\":\"Notification\",\"message\":\"Allow permission? Choose a plan\",\"selection\":\"selected document\"}";
        assert!(parse_remote_envelope(untyped, Some(1)).is_none());
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

    #[test]
    fn question_subtype_requires_usable_structured_options() {
        for (source, tool) in [("claude", "AskUserQuestion"), ("codex", "request_user_input")] {
            for options in [
                None,
                Some(Value::Null),
                Some(serde_json::json!({})),
                Some(serde_json::json!([null])),
                Some(serde_json::json!([{"label": ""}])),
                Some(serde_json::json!([{"label": "Valid"}, null])),
                Some(serde_json::json!(["unstructured choice"])),
            ] {
                let mut question = serde_json::json!({"question": "Choose a scope"});
                if let Some(options) = options {
                    question["options"] = options;
                }
                let payload = serde_json::json!({
                    "hook_event_name": "PreToolUse", "tool_name": tool,
                    "tool_input": {"questions": [question]}
                });
                assert_eq!(
                    kind(source, payload.clone()),
                    AttentionKind::Unknown,
                    "{source}: {payload}"
                );
            }
        }
        for questions in [Value::Null, serde_json::json!([]), serde_json::json!([null])] {
            assert_eq!(
                kind(
                    "claude",
                    serde_json::json!({
                        "hook_event_name": "PreToolUse", "tool_name": "AskUserQuestion",
                        "tool_input": {"questions": questions}
                    })
                ),
                AttentionKind::Unknown
            );
        }
    }

    #[test]
    fn native_attention_classification_follows_the_admitted_category() {
        use crate::ai_hook::lifecycle::AgentActivity;
        for source in ["cursor", "copilot", "grok", "antigravity"] {
            for (payload, expected) in [
                (
                    serde_json::json!({"notification_type": "permission_prompt", "notificationType": "elicitation_dialog"}),
                    AttentionKind::Permission,
                ),
                (
                    serde_json::json!({"notification_type": "elicitation_dialog", "hook_event_name": "PermissionRequest", "tool_name": "Bash"}),
                    AttentionKind::Input,
                ),
                (
                    serde_json::json!({"notification_type": "elicitation_dialog", "notificationType": "permission_prompt", "type": "permission_prompt"}),
                    AttentionKind::Input,
                ),
                (serde_json::json!({"type": "permission_prompt"}), AttentionKind::Permission),
            ] {
                let wire = format!("nebula-hook/1 source={source} event=notification\n{payload}");
                let event = parse_remote_envelope(wire.as_bytes(), Some(1)).unwrap();
                assert_eq!(event.attention.as_ref().unwrap().kind, expected, "{source}: {payload}");
                let mut activity = AgentActivity::default();
                assert!(activity.apply_hook(&event));
                assert_eq!(activity.attention_kind(), Some(expected));
            }
        }
    }
}
