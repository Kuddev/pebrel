//! Native lifecycle hooks that carry their event in the helper argv.
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde_json::Value;

use super::payload::{attention_message, context_string};
use super::{AiHookKind, AiTurnOutcome};

const MAX_TRACKED_ANTIGRAVITY_CONVERSATIONS: usize = 64;
const MAX_TRANSCRIPT_LINE_BYTES: u64 = 256 * 1024;

static ANTIGRAVITY_CONVERSATIONS: LazyLock<Mutex<VecDeque<(PathBuf, bool)>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

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
        "antigravity" => antigravity_outcome(payload),
        _ => AiTurnOutcome::Unknown,
    }
}

/// Current documented reasons plus the older captured `NO_TOOL_CALL` spelling.
fn antigravity_outcome(payload: &Value) -> AiTurnOutcome {
    if payload.get("error").and_then(Value::as_str).is_some_and(|error| !error.is_empty()) {
        return AiTurnOutcome::Failed;
    }
    match payload.get("terminationReason").and_then(Value::as_str) {
        Some("NO_TOOL_CALL" | "model_stop")
            if payload.get("fullyIdle").and_then(Value::as_bool).is_some() =>
        {
            AiTurnOutcome::Succeeded
        },
        Some("max_steps_exceeded") => AiTurnOutcome::Incomplete,
        Some("error") => AiTurnOutcome::Failed,
        _ => AiTurnOutcome::Unknown,
    }
}

/// Antigravity runs subagents inside the primary CLI process, firing the same
/// global `SessionStart`, `PreInvocation` and `Stop` hooks while scrubbing
/// internal parent fields from external JSON payloads. Reject subagents before
/// ordering or pane ownership can treat a child `SessionStart` as `/new`.
pub(super) fn is_antigravity_subagent(
    payload: &Value,
    conversation_id: Option<&str>,
    local_transport: bool,
) -> bool {
    if antigravity_payload_marks_subagent(payload, conversation_id) {
        return true;
    }
    if !local_transport {
        return false;
    }
    let Some((conversation_dir, id)) =
        validated_antigravity_conversation_dir(payload, conversation_id)
    else {
        return false;
    };
    let known_in_brain = {
        let cache =
            ANTIGRAVITY_CONVERSATIONS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((_, is_subagent)) = cache.iter().find(|(dir, _)| dir == &conversation_dir) {
            return *is_subagent;
        }
        let brain_dir = conversation_dir.parent();
        cache
            .iter()
            .filter(|(dir, _)| dir.parent() == brain_dir && *dir != conversation_dir)
            .map(|(dir, _)| dir.clone())
            .collect::<Vec<_>>()
    };
    let first_source = transcript_first_step_source(&conversation_dir);
    if first_source.as_deref() == Some("SYSTEM") {
        remember_antigravity_conversation(conversation_dir, true);
        return true;
    }
    let subagent_record = format!("{id}.json");
    if known_in_brain.iter().any(|parent_dir| {
        parent_dir.join(".system_generated").join("subagents").join(&subagent_record).is_file()
    }) {
        remember_antigravity_conversation(conversation_dir, true);
        return true;
    }
    if first_source.is_some() {
        remember_antigravity_conversation(conversation_dir, false);
    }
    false
}

fn antigravity_payload_marks_subagent(payload: &Value, conversation_id: Option<&str>) -> bool {
    if context_string(payload, &["parentConversationId", "parent_conversation_id"]).is_some() {
        return true;
    }
    if context_string(payload, &["callerContext", "caller_context"])
        .is_some_and(|caller| caller.eq_ignore_ascii_case("subagent"))
    {
        return true;
    }
    match (
        conversation_id,
        context_string(payload, &["rootConversationId", "root_conversation_id"]),
    ) {
        (Some(id), Some(root)) => id != root,
        _ => false,
    }
}

fn valid_antigravity_conversation_segment(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id != "."
        && id != ".."
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn clean_absolute_path(raw: &str) -> Option<&Path> {
    if raw.len() > 4096 || raw.chars().any(char::is_control) {
        return None;
    }
    let path = Path::new(raw);
    (path.is_absolute() && !path.components().any(|c| matches!(c, Component::ParentDir)))
        .then_some(path)
}

fn validated_brain_dir(dir: &Path, expected_id: Option<&str>) -> Option<(PathBuf, String)> {
    let segment = dir.file_name()?.to_str()?;
    if !valid_antigravity_conversation_segment(segment)
        || expected_id.is_some_and(|id| id != segment)
        || dir.parent()?.file_name()?.to_str()? != "brain"
    {
        return None;
    }
    Some((dir.to_path_buf(), segment.to_owned()))
}

fn validated_antigravity_conversation_dir(
    payload: &Value,
    conversation_id: Option<&str>,
) -> Option<(PathBuf, String)> {
    if let Some(found) = payload
        .get("artifactDirectoryPath")
        .and_then(Value::as_str)
        .and_then(clean_absolute_path)
        .and_then(|dir| validated_brain_dir(dir, conversation_id))
    {
        return Some(found);
    }
    let transcript =
        payload.get("transcriptPath").and_then(Value::as_str).and_then(clean_absolute_path)?;
    if !matches!(
        transcript.file_name().and_then(|name| name.to_str()),
        Some("transcript.jsonl" | "transcript_full.jsonl")
    ) {
        return None;
    }
    let logs = transcript.parent()?;
    if logs.file_name().and_then(|name| name.to_str()) != Some("logs") {
        return None;
    }
    let generated = logs.parent()?;
    if generated.file_name().and_then(|name| name.to_str()) != Some(".system_generated") {
        return None;
    }
    validated_brain_dir(generated.parent()?, conversation_id)
}

fn transcript_first_step_source(conversation_dir: &Path) -> Option<String> {
    let logs = conversation_dir.join(".system_generated").join("logs");
    let file = std::fs::File::open(logs.join("transcript.jsonl"))
        .or_else(|_| std::fs::File::open(logs.join("transcript_full.jsonl")))
        .ok()?;
    let mut reader = BufReader::new(file.take(MAX_TRANSCRIPT_LINE_BYTES));
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let step: Value = serde_json::from_str(line.trim()).ok()?;
    step.get("source").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned)
}

fn remember_antigravity_conversation(conversation_dir: PathBuf, is_subagent: bool) {
    let mut cache =
        ANTIGRAVITY_CONVERSATIONS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((_, cached)) = cache.iter_mut().find(|(dir, _)| dir == &conversation_dir) {
        *cached |= is_subagent;
        return;
    }
    if cache.len() >= MAX_TRACKED_ANTIGRAVITY_CONVERSATIONS {
        cache.pop_front();
    }
    cache.push_back((conversation_dir, is_subagent));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_hook::{AiHookEvent, parse_envelope};

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

    #[test]
    fn antigravity_uses_its_conversation_id_and_only_classifies_known_stops() {
        let parse = |event: &str, body: &str| {
            let raw = format!("nebula-hook/1 source=antigravity pane=5 event={event}\n{body}");
            parse_envelope(raw.as_bytes())
        };
        for (event, kind) in [
            ("session-start", AiHookKind::SessionStart),
            ("tool-complete", AiHookKind::ToolComplete),
            ("done", AiHookKind::TurnDone),
        ] {
            let parsed = parse(event, r#"{"conversationId":"conv-1","workspacePaths":["/w"]}"#);
            let parsed = parsed.unwrap();
            assert_eq!((parsed.kind, parsed.session_id.as_deref()), (kind, Some("conv-1")));
        }
        let foreign = parse("done", r#"{"sessionId":"x","session_id":"y"}"#).unwrap();
        assert_eq!(foreign.session_id, None);
        for (body, outcome) in [
            (r#"{"terminationReason":"NO_TOOL_CALL","fullyIdle":true}"#, AiTurnOutcome::Succeeded),
            (r#"{"terminationReason":"model_stop","fullyIdle":true}"#, AiTurnOutcome::Succeeded),
            (r#"{"terminationReason":"model_stop"}"#, AiTurnOutcome::Unknown),
            (r#"{"terminationReason":"max_steps_exceeded"}"#, AiTurnOutcome::Incomplete),
            (r#"{"terminationReason":"model_stop","error":"quota"}"#, AiTurnOutcome::Failed),
            (r#"{"terminationReason":"user_cancelled"}"#, AiTurnOutcome::Unknown),
        ] {
            assert_eq!(parse("done", body).unwrap().turn_outcome, outcome, "{body}");
        }
    }

    #[test]
    fn antigravity_hook_identity_resumes_after_session_round_trip() {
        let event = parse_envelope(
            b"nebula-hook/1 source=antigravity pane=5 event=session-start\n{\"conversationId\":\"b5f6c1c2-1111-2222-3333-444455556666\"}",
        )
        .unwrap();
        let saved = crate::session::AgentSession {
            source: event.source,
            session_id: event.session_id,
            session_file: event.session_file,
        };
        let json = serde_json::to_string(&saved).unwrap();
        let restored: crate::session::AgentSession = serde_json::from_str(&json).unwrap();
        let expected = "agy --conversation b5f6c1c2-1111-2222-3333-444455556666";
        assert_eq!(restored.resume_command().as_deref(), Some(expected));

        let manual = crate::ai_sessions::AiSession::test_session(
            crate::ai_agents::AgentKind::Antigravity,
            restored.session_id.as_deref().unwrap(),
            "Saved conversation",
        );
        assert_eq!(manual.resume_command().as_deref(), Some(expected));
        assert_eq!(manual.fork_command(), None);
    }

    #[test]
    fn antigravity_resume_requires_a_safe_explicit_conversation_id() {
        for id in [None, Some(""), Some("x; calc"), Some("$(id)"), Some("two words"), Some("x\ny")]
        {
            let saved = crate::session::AgentSession {
                source: "antigravity".into(),
                session_id: id.map(str::to_owned),
                session_file: None,
            };
            assert_eq!(saved.resume_command(), None, "{id:?}");
        }
        assert_eq!(crate::ai_agents::AgentKind::Antigravity.resume_command(&"a".repeat(65)), None);
    }

    #[test]
    fn antigravity_background_work_defers_completion_until_fully_idle() {
        use crate::ai_agents::AgentStatus;
        use crate::ai_hook::lifecycle::AgentActivity;
        let mut activity = AgentActivity::default();
        for (idle, expected) in [(false, AgentStatus::Working), (true, AgentStatus::Done)] {
            let wire = format!(
                "nebula-hook/1 source=antigravity pane=5 event=done\n{{\"conversationId\":\"conv-1\",\"terminationReason\":\"model_stop\",\"fullyIdle\":{idle}}}"
            );
            let event = parse_envelope(wire.as_bytes()).unwrap();
            assert!(event.capabilities().background_tasks);
            assert_eq!(event.active_background_tasks(), u32::from(!idle));
            assert!(activity.apply_hook(&event));
            assert_eq!(activity.status(), expected);
            assert_eq!(
                crate::notify::Notification::from_ai_hook(&event, None, false).is_some(),
                idle
            );
        }
        let event = parse_envelope(
            b"nebula-hook/1 source=antigravity event=done\n{\"terminationReason\":\"model_stop\"}",
        )
        .unwrap();
        assert!(crate::notify::Notification::from_ai_hook(&event, None, false).is_none());
    }

    #[test]
    fn antigravity_in_process_subagents_never_hijack_primary_conversation_or_turn() {
        use crate::ai_agents::AgentStatus;
        use crate::ai_hook::lifecycle::AgentActivity;
        use serde_json::json;

        let temp = tempfile::tempdir().unwrap();
        let brain = temp.path().join("brain");
        let write_transcript = |id: &str, source: &str, step_type: &str| {
            let logs = brain.join(id).join(".system_generated").join("logs");
            std::fs::create_dir_all(&logs).unwrap();
            let step = json!({"step_index": 0, "source": source, "type": step_type}).to_string();
            let content = format!("{step}\n");
            std::fs::write(logs.join("transcript.jsonl"), &content).unwrap();
            let full = logs.join("transcript_full.jsonl");
            std::fs::write(&full, &content).unwrap();
            full
        };

        let root_id = "11111111-1111-4111-8111-111111111111";
        let sub_id = "22222222-2222-4222-8222-222222222222";
        let fork_id = "33333333-3333-4333-8333-333333333333";
        let next_root_id = "44444444-4444-4444-8444-444444444444";

        let root_transcript = write_transcript(root_id, "USER_EXPLICIT", "USER_INPUT");
        let sub_transcript = write_transcript(sub_id, "SYSTEM", "SYSTEM_MESSAGE");
        let fork_transcript = write_transcript(fork_id, "USER_EXPLICIT", "USER_INPUT");
        let next_root_transcript = write_transcript(next_root_id, "USER_EXPLICIT", "USER_INPUT");
        let subagents_dir = brain.join(root_id).join(".system_generated").join("subagents");
        std::fs::create_dir_all(&subagents_dir).unwrap();
        std::fs::write(
            subagents_dir.join(format!("{fork_id}.json")),
            json!({"conversationId": fork_id, "state": "SUBAGENT_STATE_ALIVE"}).to_string(),
        )
        .unwrap();

        let send = |event: &str, id: &str, transcript: &Path, extra: Value| {
            let mut body = json!({
                "conversationId": id,
                "transcriptPath": transcript.to_str().unwrap(),
            });
            if let Some(extra_obj) = extra.as_object() {
                for (k, v) in extra_obj {
                    body[k] = v.clone();
                }
            }
            let wire = format!("nebula-hook/1 source=antigravity pane=5 event={event}\n{body}");
            parse_envelope(wire.as_bytes()).map(|mut parsed| {
                parsed.agent_pid = Some(4242);
                parsed
            })
        };

        let mut activity = AgentActivity::default();
        let mut active_session = None;
        fn accept(
            activity: &mut AgentActivity,
            active_session: &mut Option<String>,
            event: AiHookEvent,
        ) {
            assert!(activity.apply_hook(&event));
            if let Some(id) = event.session_id {
                *active_session = Some(id);
            }
        }

        accept(
            &mut activity,
            &mut active_session,
            send("session-start", root_id, &root_transcript, json!({})).unwrap(),
        );
        assert_eq!(activity.status(), AgentStatus::Idle);
        accept(
            &mut activity,
            &mut active_session,
            send("tool-complete", root_id, &root_transcript, json!({})).unwrap(),
        );
        assert_eq!(activity.status(), AgentStatus::Working);
        assert_eq!(active_session.as_deref(), Some(root_id));

        for (child_id, child_transcript) in [(sub_id, &sub_transcript), (fork_id, &fork_transcript)]
        {
            for (event, extra) in [
                ("session-start", json!({})),
                ("tool-complete", json!({})),
                ("done", json!({"terminationReason": "model_stop", "fullyIdle": true})),
            ] {
                assert!(
                    send(event, child_id, child_transcript, extra).is_none(),
                    "subagent {child_id} event {event} must be filtered before pane routing"
                );
            }
        }
        assert_eq!(activity.status(), AgentStatus::Working);
        assert_eq!(active_session.as_deref(), Some(root_id));

        accept(
            &mut activity,
            &mut active_session,
            send("tool-complete", root_id, &root_transcript, json!({})).unwrap(),
        );
        accept(
            &mut activity,
            &mut active_session,
            send(
                "done",
                root_id,
                &root_transcript,
                json!({"terminationReason": "model_stop", "fullyIdle": true}),
            )
            .unwrap(),
        );
        assert_eq!(activity.status(), AgentStatus::Done);
        assert_eq!(active_session.as_deref(), Some(root_id));

        let saved = crate::session::AgentSession {
            source: "antigravity".into(),
            session_id: active_session.clone(),
            session_file: None,
        };
        assert_eq!(
            saved.resume_command().as_deref(),
            Some("agy --conversation 11111111-1111-4111-8111-111111111111")
        );

        accept(
            &mut activity,
            &mut active_session,
            send("session-start", next_root_id, &next_root_transcript, json!({})).unwrap(),
        );
        assert_eq!(activity.status(), AgentStatus::Idle);
        assert_eq!(active_session.as_deref(), Some(next_root_id));
    }

    #[test]
    fn antigravity_explicit_subagent_markers_and_untrusted_paths_are_handled_safely() {
        use crate::ai_hook::parse_remote_envelope;
        use serde_json::json;

        let parse_both = |body: Value| {
            let wire =
                format!("nebula-hook/1 source=antigravity pane=5 event=session-start\n{body}");
            (parse_envelope(wire.as_bytes()), parse_remote_envelope(wire.as_bytes(), Some(5)))
        };

        for subagent_body in [
            json!({
                "conversationId": "22222222-2222-4222-8222-222222222222",
                "parentConversationId": "11111111-1111-4111-8111-111111111111",
            }),
            json!({
                "conversationId": "22222222-2222-4222-8222-222222222222",
                "callerContext": "subagent",
            }),
            json!({
                "conversationId": "22222222-2222-4222-8222-222222222222",
                "rootConversationId": "11111111-1111-4111-8111-111111111111",
            }),
        ] {
            let (local, remote) = parse_both(subagent_body.clone());
            assert!(local.is_none() && remote.is_none(), "{subagent_body}");
        }

        for root_body in [
            json!({
                "conversationId": "11111111-1111-4111-8111-111111111111",
                "parentConversationId": "",
                "callerContext": "",
                "rootConversationId": "11111111-1111-4111-8111-111111111111",
                "agentName": "custom-agent",
            }),
            json!({
                "conversationId": "11111111-1111-4111-8111-111111111111",
                "transcriptPath": "/tmp/brain/../outside/transcript.jsonl",
            }),
        ] {
            let (local, remote) = parse_both(root_body.clone());
            assert!(local.is_some() && remote.is_some(), "{root_body}");
        }

        let temp = tempfile::tempdir().unwrap();
        let sub_id = "22222222-2222-4222-8222-222222222222";
        let logs = temp.path().join("brain").join(sub_id).join(".system_generated").join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        let transcript = logs.join("transcript_full.jsonl");
        std::fs::write(
            &transcript,
            "{\"step_index\":0,\"source\":\"SYSTEM\",\"type\":\"SYSTEM_MESSAGE\"}\n",
        )
        .unwrap();
        let wire = format!(
            "nebula-hook/1 source=antigravity pane=5 event=session-start\n{}",
            json!({"conversationId": sub_id, "transcriptPath": transcript.to_str().unwrap()})
        );
        assert!(parse_envelope(wire.as_bytes()).is_none());
        assert!(
            parse_remote_envelope(wire.as_bytes(), Some(5)).is_some(),
            "remote envelopes must never inspect local host transcript files"
        );
    }
}
