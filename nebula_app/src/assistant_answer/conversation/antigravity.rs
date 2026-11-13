//! Deterministic Antigravity (`agy`) brain transcript resolution and projection.

use std::{
    collections::VecDeque,
    io,
    path::{Path, PathBuf},
};

use serde_json::Value;

use super::{Capture, HEAD_BYTES, Message, capture_open_file, changed, io_failure, push_message};
use crate::runtime_api::ApiError;

const BRAIN_SEGMENTS: [&str; 3] = [".gemini", "antigravity-cli", "brain"];
const LOG_SEGMENTS: [&str; 2] = [".system_generated", "logs"];
const FULL_FILE: &str = "transcript_full.jsonl";
const COMPACT_FILE: &str = "transcript.jsonl";

/// Single-segment ASCII conversation ID accepted for brain path construction.
pub(crate) fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('.')
        && !id.contains("..")
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

pub(super) fn capture_local(session_id: &str, before: Option<u64>) -> Result<Capture, ApiError> {
    let home = crate::platform::dirs::home_dir().ok_or_else(|| io_failure("home"))?;
    capture_in(&home, session_id, before)
}

fn verify_logs_dir(home: &Path, session_id: &str) -> Result<PathBuf, ApiError> {
    if !valid_session_id(session_id) {
        return Err(io_failure("session_id"));
    }
    let mut current = home.to_path_buf();
    for segment in BRAIN_SEGMENTS.into_iter().chain(std::iter::once(session_id)).chain(LOG_SEGMENTS)
    {
        current.push(segment);
        let meta = std::fs::symlink_metadata(&current).map_err(io_failure)?;
        if !meta.is_dir() || crate::platform::local_paths::is_symlink_or_reparse(&meta) {
            return Err(io_failure("unsafe transcript directory"));
        }
    }
    Ok(current)
}

pub(super) fn capture_in(
    home: &Path,
    session_id: &str,
    before: Option<u64>,
) -> Result<Capture, ApiError> {
    let logs = verify_logs_dir(home, session_id)?;
    let full = logs.join(FULL_FILE);
    let (file, chosen, used_compact) =
        match crate::platform::local_paths::open_regular_file_no_follow(&full) {
            Ok(file) => (file, full, false),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let compact = logs.join(COMPACT_FILE);
                let file = crate::platform::local_paths::open_regular_file_no_follow(&compact)
                    .map_err(io_failure)?;
                (file, compact, true)
            },
            Err(error) => return Err(io_failure(error)),
        };
    let path = chosen.to_str().ok_or_else(|| io_failure("utf8 path"))?.to_owned();
    let capture = capture_open_file(file, path, before)?;
    let after_logs = verify_logs_dir(home, session_id)?;
    if used_compact && std::fs::symlink_metadata(after_logs.join(FULL_FILE)).is_ok() {
        return Err(changed());
    }
    Ok(capture)
}

pub(super) fn valid_source(session_id: &str, path: &str, head: &[u8]) -> bool {
    valid_transcript_path(path, session_id) && valid_primary_head(head)
}

fn valid_transcript_path(path: &str, session_id: &str) -> bool {
    if !valid_session_id(session_id)
        || path.is_empty()
        || path.len() > 4096
        || path.chars().any(char::is_control)
    {
        return false;
    }
    let parts = path.split(['/', '\\']).filter(|part| !part.is_empty()).collect::<Vec<_>>();
    parts.len() >= 4
        && parts.iter().all(|part| *part != "..")
        && parts[parts.len() - 4] == session_id
        && parts[parts.len() - 3] == LOG_SEGMENTS[0]
        && parts[parts.len() - 2] == LOG_SEGMENTS[1]
        && matches!(parts[parts.len() - 1], FULL_FILE | COMPACT_FILE)
}

fn valid_primary_head(head: &[u8]) -> bool {
    if head.is_empty() {
        return true;
    }
    if let Some(newline) = head.iter().position(|&b| b == b'\n') {
        return serde_json::from_slice::<Value>(&head[..newline]).ok().is_some_and(|record| {
            record.get("step_index").and_then(Value::as_u64).is_some()
                && record["source"].as_str().is_some_and(|source| source != "SYSTEM")
        });
    }
    let prefix_len = head.len().min(256);
    let mut end = prefix_len;
    while end > 0 && std::str::from_utf8(&head[..end]).is_err() {
        end -= 1;
    }
    let prefix = std::str::from_utf8(&head[..end]).unwrap_or_default();
    if prefix.contains("\"source\":\"SYSTEM\"") {
        return false;
    }
    if head.len() < HEAD_BYTES {
        prefix.starts_with('{')
    } else {
        prefix.starts_with('{')
            && prefix.contains("\"step_index\":")
            && prefix.contains("\"source\":\"")
    }
}

pub(super) fn unwrap_user_input(raw: &str) -> &str {
    let Some(rest) = raw.strip_prefix("<USER_REQUEST>") else {
        return raw;
    };
    let rest = rest.strip_prefix("\r\n").or_else(|| rest.strip_prefix('\n')).unwrap_or(rest);
    let end = rest
        .rfind("</USER_REQUEST>\n<ADDITIONAL_METADATA>")
        .or_else(|| rest.rfind("</USER_REQUEST>\r\n<ADDITIONAL_METADATA>"))
        .or_else(|| rest.rfind("</USER_REQUEST>"));
    let Some(end) = end else {
        return raw;
    };
    let inner = &rest[..end];
    inner.strip_suffix("\r\n").or_else(|| inner.strip_suffix('\n')).unwrap_or(inner)
}

fn finish_pending(messages: &mut [Message], pending_tools: &mut VecDeque<usize>) {
    while let Some(index) = pending_tools.pop_front() {
        if let Some(message) = messages.get_mut(index) {
            message.complete = true;
        }
    }
}

pub(super) fn project_step(
    record: &Value,
    id: &str,
    messages: &mut Vec<Message>,
    pending_tools: &mut VecDeque<usize>,
) {
    if record["source"] == "SYSTEM" {
        return;
    }
    match (record["source"].as_str(), record["type"].as_str()) {
        (Some("USER_EXPLICIT"), Some("USER_INPUT")) => {
            finish_pending(messages, pending_tools);
            if let Some(raw) = record["content"].as_str() {
                push_message(messages, id.into(), "user", unwrap_user_input(raw).to_owned(), false);
            }
        },
        (Some("MODEL"), Some("PLANNER_RESPONSE")) => {
            finish_pending(messages, pending_tools);
            if let Some(text) = record["content"].as_str() {
                push_message(messages, format!("{id}:text"), "assistant", text.to_owned(), false);
            }
            if let Some(calls) = record["tool_calls"].as_array() {
                for (index, call) in calls.iter().enumerate() {
                    let name = call["name"].as_str().filter(|s| !s.is_empty()).unwrap_or("tool");
                    let args = call
                        .get("args")
                        .map(|value| serde_json::to_string_pretty(value).unwrap_or_default())
                        .unwrap_or_default();
                    pending_tools.push_back(messages.len());
                    messages.push(Message {
                        id: format!("{id}:call:{index}"),
                        role: "tool",
                        text: args,
                        name: Some(name.to_owned()),
                        detail: None,
                        complete: false,
                        truncated: false,
                        paired: false,
                        event: false,
                    });
                }
            }
        },
        (Some("MODEL"), Some("GENERIC")) => {
            let output = record["content"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or_else(|| record["error"].as_str())
                .unwrap_or_default()
                .to_owned();
            if let Some(index) = pending_tools.pop_front()
                && let Some(message) = messages.get_mut(index)
            {
                message.detail = Some(output);
                message.complete = true;
            } else {
                messages.push(Message {
                    id: id.into(),
                    role: "tool",
                    text: String::new(),
                    name: None,
                    detail: Some(output),
                    complete: true,
                    truncated: false,
                    paired: false,
                    event: false,
                });
            }
        },
        _ => {},
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use serde_json::json;

    use super::*;
    use crate::{assistant_answer::conversation::project, runtime_api::conversation::Identity};

    fn antigravity_identity(id: &str) -> Identity {
        Identity { kind: "antigravity".into(), session_id: id.into(), epoch: Some(3) }
    }

    fn write_brain_logs(home: &Path, id: &str, file: &str, rows: &[Value]) -> PathBuf {
        let logs =
            home.join(".gemini/antigravity-cli/brain").join(id).join(".system_generated/logs");
        fs::create_dir_all(&logs).unwrap();
        let path = logs.join(file);
        let mut handle = fs::File::create(&path).unwrap();
        for row in rows {
            writeln!(handle, "{row}").unwrap();
        }
        path
    }

    #[test]
    fn antigravity_projection_unwraps_requests_hides_thinking_and_pairs_tool_steps() {
        let home = tempfile::tempdir().unwrap();
        let id = "0199a213-c2a4-7cf5-8f6b-d746fbb6e86c";
        let identity = antigravity_identity(id);
        let nested = "检查配置\n<USER_REQUEST>保留字面标签</USER_REQUEST>";
        write_brain_logs(
            home.path(),
            id,
            FULL_FILE,
            &[
                json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","content":format!("<USER_REQUEST>\n{nested}\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nlocal time\n</ADDITIONAL_METADATA>")}),
                json!({"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","thinking":"private chain of thought","content":"先读取配置。","tool_calls":[{"name":"read_file","args":{"Path":"Cargo.toml"}},{"name":"run_command","args":{"CommandLine":"cargo test"}}]}),
                json!({"step_index":2,"source":"MODEL","type":"GENERIC","status":"DONE","content":"[package]\nname = \"demo\""}),
                json!({"step_index":3,"source":"MODEL","type":"GENERIC","status":"RUNNING","content":"Tool is running as a background task"}),
                json!({"step_index":4,"source":"SYSTEM","type":"SYSTEM_MESSAGE","status":"DONE","content":"internal wakeup"}),
                json!({"step_index":5,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","content":format!("<USER_REQUEST>\n{nested}\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nlocal time\n</ADDITIONAL_METADATA>")}),
                json!({"step_index":6,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","thinking":"more secret thought","tool_calls":[{"name":"read_file","args":{"Path":"README.md"}}]}),
            ],
        );
        // Prefer transcript_full.jsonl even when compact transcript.jsonl is also present.
        write_brain_logs(
            home.path(),
            id,
            COMPACT_FILE,
            &[
                json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","content":"<USER_REQUEST>\ncompact\n</USER_REQUEST>"}),
            ],
        );

        let capture = capture_in(home.path(), id, None).unwrap();
        assert!(valid_source(id, &capture.path, &capture.head));
        let (messages, before, truncated) = project(&identity, &capture).unwrap();
        assert!(!truncated);
        assert!(before.is_none());
        assert_eq!(messages.len(), 6);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].text, nested);
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].text, "先读取配置。");
        assert_eq!(messages[2].role, "tool");
        assert_eq!(messages[2].name.as_deref(), Some("read_file"));
        assert_eq!(messages[2].detail.as_deref(), Some("[package]\nname = \"demo\""));
        assert!(messages[2].complete);
        assert_eq!(messages[3].detail.as_deref(), Some("Tool is running as a background task"));
        assert!(messages[3].complete, "detached background launch step has already returned");
        assert_eq!(messages[4].role, "user");
        assert_eq!(messages[4].text, nested);
        assert_ne!(messages[0].id, messages[4].id);
        assert_eq!(messages[5].role, "tool");
        assert!(!messages[5].complete, "trailing unreturned tool call stays in progress");
        let serialized = serde_json::to_string(&messages).unwrap();
        assert!(!serialized.contains("private chain of thought"));
        assert!(!serialized.contains("more secret thought"));
        assert!(!serialized.contains("internal wakeup"));
        assert!(!serialized.contains("ADDITIONAL_METADATA"));
    }

    #[test]
    fn antigravity_validation_rejects_subagents_traversal_and_symlinks() {
        assert!(!valid_session_id(""));
        assert!(!valid_session_id("."));
        assert!(!valid_session_id(".."));
        assert!(!valid_session_id(".hidden"));
        assert!(!valid_session_id("a..b"));
        assert!(!valid_session_id("nested/id"));

        let home = tempfile::tempdir().unwrap();
        let subagent = "subagent-01";
        let sub_path = write_brain_logs(
            home.path(),
            subagent,
            COMPACT_FILE,
            &[
                json!({"step_index":0,"source":"SYSTEM","type":"SYSTEM_MESSAGE","status":"DONE","content":"subagent prompt"}),
            ],
        );
        let sub_capture = capture_in(home.path(), subagent, None).unwrap();
        assert!(!valid_source(subagent, &sub_capture.path, &sub_capture.head));
        assert!(!valid_source("other-session", sub_path.to_str().unwrap(), b""));

        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.jsonl");
        fs::write(
            &secret,
            "{\"step_index\":0,\"source\":\"USER_EXPLICIT\",\"type\":\"USER_INPUT\",\"content\":\"ok\"}\n",
        )
        .unwrap();
        let linked_session = "linked-01";
        let logs = home
            .path()
            .join(".gemini/antigravity-cli/brain")
            .join(linked_session)
            .join(".system_generated/logs");
        fs::create_dir_all(&logs).unwrap();
        if let Err(error) =
            crate::platform::local_paths::symlink_file_for_test(&secret, &logs.join(FULL_FILE))
        {
            if crate::platform::Platform::current() == crate::platform::Platform::Windows
                && error.raw_os_error() == Some(1314)
            {
                return;
            }
            panic!("symlink fixture: {error}");
        }
        write_brain_logs(
            home.path(),
            linked_session,
            COMPACT_FILE,
            &[
                json!({"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"fallback"}),
            ],
        );
        assert!(
            capture_in(home.path(), linked_session, None).is_err(),
            "a symlinked transcript_full.jsonl must fail closed instead of falling back"
        );
    }
}
