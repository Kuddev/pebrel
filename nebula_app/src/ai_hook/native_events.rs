//! Native lifecycle hooks that carry their event in the helper argv.
use serde_json::Value;

use super::payload::{attention_message, context_string};
use super::{AiHookKind, AiTurnOutcome};

pub(super) fn parse(
    source: &str,
    event: &str,
    payload: &Value,
) -> Option<(AiHookKind, Option<String>)> {
    let kind = match event {
        "session-start" => AiHookKind::SessionStart,
        "prompt" => AiHookKind::PromptSubmit,
        "tool-complete" => AiHookKind::ToolComplete,
        "done" => AiHookKind::TurnDone,
        "failed" if source == "grok" => AiHookKind::TurnDone,
        "error"
            if source == "copilot" && payload.get("recoverable") == Some(&Value::Bool(false)) =>
        {
            AiHookKind::TurnDone
        },
        "session-end" => AiHookKind::SessionEnd,
        "notification" => {
            let category =
                context_string(payload, &["notification_type", "notificationType", "type"]);
            let actionable = category
                .as_deref()
                .is_some_and(|value| value == "elicitation_dialog" || value == "permission_prompt");
            if !actionable {
                return None;
            }
            return Some((AiHookKind::NeedsAttention, attention_message(payload)));
        },
        _ => return None,
    };
    Some((kind, None))
}

pub(super) fn outcome(source: &str, event: &str, payload: &Value) -> AiTurnOutcome {
    if matches!(event, "failed" | "error") {
        return AiTurnOutcome::Failed;
    }
    match source {
        "cursor" => match payload.get("status").and_then(Value::as_str) {
            Some("completed") => AiTurnOutcome::Succeeded,
            Some("aborted") => AiTurnOutcome::Cancelled,
            Some("error") => AiTurnOutcome::Failed,
            _ => AiTurnOutcome::Unknown,
        },
        "copilot" => match payload.get("stopReason").and_then(Value::as_str) {
            Some("end_turn") => AiTurnOutcome::Succeeded,
            _ => AiTurnOutcome::Unknown,
        },
        "grok" if event == "done" => AiTurnOutcome::Succeeded,
        _ => AiTurnOutcome::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_hook::parse_envelope;

    #[test]
    fn native_events_preserve_provider_and_resume_identity() {
        for source in ["copilot", "grok"] {
            for (event, kind) in [
                ("session-start", AiHookKind::SessionStart),
                ("prompt", AiHookKind::PromptSubmit),
                ("done", AiHookKind::TurnDone),
                ("tool-complete", AiHookKind::ToolComplete),
                ("session-end", AiHookKind::SessionEnd),
            ] {
                let raw = format!(
                    "nebula-hook/1 source={source} pane=7 event={event}\n{{\"sessionId\":\"native-session\"}}"
                );
                let parsed = parse_envelope(raw.as_bytes()).unwrap();
                assert_eq!(parsed.source, source);
                assert_eq!(parsed.session_id.as_deref(), Some("native-session"));
                assert_eq!(parsed.kind, kind);
                assert_eq!(parsed.pane, Some(7));
            }
            let idle = format!(
                "nebula-hook/1 source={source} event=notification\n{{\"notification_type\":\"idle_prompt\"}}"
            );
            assert!(parse_envelope(idle.as_bytes()).is_none());
            let attention = format!(
                "nebula-hook/1 source={source} event=notification\n{{\"notificationType\":\"elicitation_dialog\",\"message\":\"Approve?\"}}"
            );
            assert_eq!(
                parse_envelope(attention.as_bytes()).unwrap().kind,
                AiHookKind::NeedsAttention
            );
        }
    }

    #[test]
    fn cursor_resume_ids_are_source_specific() {
        for (source, field) in [("cursor", "conversation_id"), ("cursor", "session_id")] {
            for event in ["session-start", "prompt", "done", "tool-complete", "session-end"] {
                let raw = format!(
                    "nebula-hook/1 source={source} pane=4 event={event}\n{{\"{field}\":\"native-id\",\"sessionId\":\"foreign-id\"}}"
                );
                let parsed = parse_envelope(raw.as_bytes()).unwrap();
                assert_eq!(parsed.source, source);
                assert_eq!(parsed.session_id.as_deref(), Some("native-id"));
            }
            let raw = format!(
                "nebula-hook/1 source={source} event=done\n{{\"sessionId\":\"foreign-id\"}}"
            );
            assert!(parse_envelope(raw.as_bytes()).unwrap().session_id.is_none());
        }
        for (status, outcome) in [
            ("completed", AiTurnOutcome::Succeeded),
            ("aborted", AiTurnOutcome::Cancelled),
            ("error", AiTurnOutcome::Failed),
        ] {
            let raw = format!(
                "nebula-hook/1 source=cursor event=done\n{{\"status\":\"{status}\",\"conversation_id\":\"id\"}}"
            );
            let parsed = parse_envelope(raw.as_bytes()).unwrap();
            assert_eq!(parsed.kind, AiHookKind::TurnDone);
            assert_eq!(parsed.turn_outcome, outcome);
        }
    }

    #[test]
    fn omp_results_do_not_become_pi_sessions() {
        let parsed = parse_envelope(b"nebula-hook/1 source=omp pane=3\n{\"kind\":\"done\",\"session_id\":\"omp-session\",\"stop_reason\":\"error\"}").unwrap();
        assert_eq!(parsed.source, "omp");
        assert_eq!(parsed.session_id.as_deref(), Some("omp-session"));
        assert_eq!(parsed.kind, AiHookKind::TurnDone);
        assert_eq!(parsed.turn_outcome, AiTurnOutcome::Failed);
    }

    #[test]
    fn native_failures_and_foreign_cursor_payloads_never_report_success() {
        let grok = parse_envelope(b"nebula-hook/1 source=grok event=failed\n{\"hookEventName\":\"StopFailure\",\"sessionId\":\"grok-session\"}").unwrap();
        assert_eq!(grok.turn_outcome, AiTurnOutcome::Failed);
        let retry = b"nebula-hook/1 source=copilot event=error\n{\"recoverable\":true,\"sessionId\":\"retry\"}";
        assert!(parse_envelope(retry).is_none());
        let failed = b"nebula-hook/1 source=copilot event=error\n{\"recoverable\":false,\"sessionId\":\"failed\"}";
        assert_eq!(parse_envelope(failed).unwrap().turn_outcome, AiTurnOutcome::Failed);
        assert!(parse_envelope(b"nebula-hook/1 source=cursor event=done\n{\"hookEventName\":\"Stop\",\"sessionId\":\"grok\"}").is_none());
    }
}
