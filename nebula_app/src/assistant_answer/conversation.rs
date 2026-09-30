//! Bounded native transcript projection. No history scan or screen-to-message guessing.

use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use base64::Engine as _;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    runtime_api::{ApiError, conversation::Identity},
    runtime_exec::PaneExecContext,
};

const HEAD_BYTES: usize = 256 * 1024;
const PAGE_BYTES: usize = 1024 * 1024;
const MESSAGE_BYTES: usize = 48 * 1024;
const OUTPUT_BYTES: usize = 128 * 1024;
const MESSAGE_COUNT: usize = 160;

mod remote;

pub(crate) enum Location {
    Local(PaneExecContext),
    Ssh(crate::ssh_session::TranscriptReader),
}

pub(crate) struct Source {
    pub identity: Identity,
    pub path: String,
    pub location: Location,
    pub cwd: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Message {
    pub id: String,
    pub role: &'static str,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub complete: bool,
    pub truncated: bool,
    #[serde(skip)]
    paired: bool,
    #[serde(skip)]
    event: bool,
}

struct Capture {
    head: Vec<u8>,
    bytes: Vec<u8>,
    start: u64,
    end: u64,
    stamp: String,
}

pub(crate) fn read(
    source: Source,
    before: Option<u64>,
    previous: Option<&str>,
) -> Result<Value, ApiError> {
    let capture = match &source.location {
        Location::Ssh(reader) => remote::capture(reader, &source.path, before)?,
        Location::Local(context) if context.wsl_distribution().is_some() => {
            capture_wsl(&source, context, before)?
        },
        Location::Local(_) => capture_file(Path::new(&source.path), before)?,
    };
    validate_source(&source.identity, &source.path, &capture.head)?;
    let mut hash = Sha256::new();
    hash.update(capture.stamp.as_bytes());
    hash.update(&capture.bytes);
    hash.update(capture.start.to_le_bytes());
    let revision = hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    if previous == Some(revision.as_str()) {
        return Ok(json!({"revision": revision, "unchanged": true}));
    }
    let (messages, cursor, truncated) = project(&source.identity, &capture)?;
    Ok(json!({
        "revision": revision, "unchanged": false, "messages": messages,
        "before": cursor, "truncated": truncated, "cwd": source.cwd,
    }))
}

fn io_failure(_: impl std::fmt::Display) -> ApiError {
    // 不把本地会话路径、原文或系统错误中的私人信息转发给手机。
    ApiError::new("conversation_unavailable", "the native conversation file is not readable")
}

fn capture_file(path: &Path, before: Option<u64>) -> Result<Capture, ApiError> {
    let mut file = File::open(path).map_err(io_failure)?;
    let meta = file.metadata().map_err(io_failure)?;
    if !meta.is_file() {
        return Err(io_failure("not a file"));
    }
    let end = before.unwrap_or(meta.len());
    if end > meta.len() {
        return Err(changed());
    }
    let start = end.saturating_sub(PAGE_BYTES as u64);
    let mut head = Vec::new();
    (&mut file).take(HEAD_BYTES as u64).read_to_end(&mut head).map_err(io_failure)?;
    file.seek(SeekFrom::Start(start.saturating_sub(1))).map_err(io_failure)?;
    let mut bytes = Vec::new();
    (&mut file).take(end - start.saturating_sub(1)).read_to_end(&mut bytes).map_err(io_failure)?;
    let after = file.metadata().map_err(io_failure)?;
    if meta.len() != after.len() || meta.modified().ok() != after.modified().ok() {
        return Err(changed());
    }
    Ok(Capture {
        head,
        bytes,
        start,
        end,
        stamp: format!("{}:{:?}", meta.len(), meta.modified().ok()),
    })
}

fn capture_wsl(
    source: &Source,
    context: &PaneExecContext,
    before: Option<u64>,
) -> Result<Capture, ApiError> {
    // 路径只作为 argv 传入，脚本固定；在所属发行版中读取，避免 9P/UNC 与用户映射差异。
    const SCRIPT: &str = r#"set -eu
p=$1
[ -f "$p" ]
size=$(stat -c %s -- "$p")
stamp=$(stat -c '%i:%s:%y:%z' -- "$p")
end=${2:-$size}
[ "$end" -le "$size" ]
start=$((end > 1048576 ? end - 1048576 : 0))
readstart=$((start > 0 ? start - 1 : 0))
printf '%s\n%s\n' "$end" "$stamp"
head -c 262144 -- "$p" | base64 -w 0
printf '\n'
tail -c +$((readstart + 1)) -- "$p" | head -c $((end - readstart)) | base64 -w 0
printf '\n%s\n' "$(stat -c '%i:%s:%y:%z' -- "$p")"
"#;
    let result = crate::runtime_exec::execute(
        context.clone(),
        source.cwd.clone(),
        vec![
            "sh".into(),
            "-c".into(),
            SCRIPT.into(),
            "pebrel-conversation".into(),
            source.path.clone(),
            before.map(|value| value.to_string()).unwrap_or_default(),
        ],
        5000,
        2 * 1024 * 1024,
    )?;
    if result["success"] != true || result["capture"]["stdout"]["truncated"] != false {
        return Err(io_failure("guest read failed"));
    }
    let mut lines = result["stdout"].as_str().unwrap_or_default().lines();
    let end =
        lines.next().and_then(|s| s.parse::<u64>().ok()).ok_or_else(|| io_failure("metadata"))?;
    let stamp = lines.next().ok_or_else(|| io_failure("metadata"))?.to_owned();
    let base64 = base64::engine::general_purpose::STANDARD;
    let head = base64.decode(lines.next().unwrap_or_default()).map_err(io_failure)?;
    let bytes = base64.decode(lines.next().unwrap_or_default()).map_err(io_failure)?;
    if lines.next() != Some(stamp.as_str()) {
        return Err(changed());
    }
    Ok(Capture { head, bytes, start: end.saturating_sub(PAGE_BYTES as u64), end, stamp })
}

fn changed() -> ApiError {
    ApiError::new(
        "conversation_changed",
        "the transcript changed while reading; refresh its current page",
    )
}

fn validate_source(identity: &Identity, path: &str, head: &[u8]) -> Result<(), ApiError> {
    let valid = match identity.kind.as_str() {
        "codex" => head
            .split(|b| *b == b'\n')
            .next()
            .and_then(|line| serde_json::from_slice::<Value>(line).ok())
            .is_some_and(|record| {
                record["type"] == "session_meta" && record["payload"]["id"] == identity.session_id
            }),
        "claude" => {
            path.rsplit(['/', '\\']).next().and_then(|name| name.strip_suffix(".jsonl"))
                == Some(identity.session_id.as_str())
        },
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ApiError::new(
            "conversation_identity_changed",
            "the native file belongs to a different conversation",
        ))
    }
}

fn project(
    identity: &Identity,
    capture: &Capture,
) -> Result<(Vec<Message>, Option<u64>, bool), ApiError> {
    let mut bytes = capture.bytes.as_slice();
    let mut offset = capture.start;
    if capture.start > 0 {
        if bytes.first() == Some(&b'\n') {
            bytes = &bytes[1..];
        } else if let Some(end) = bytes.iter().position(|b| *b == b'\n') {
            bytes = &bytes[end + 1..];
            offset = capture.start - 1 + end as u64 + 1;
        } else {
            return Ok((Vec::new(), Some(capture.start), true));
        }
    }
    let cursor = (offset > 0).then_some(offset);
    let mut messages = Vec::new();
    let mut calls = HashMap::new();
    let mut truncated = false;
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        let id = offset.to_string();
        offset += line.len() as u64;
        // CLI 正在追加的最后半行等待下次读取，不把不完整 JSON 当作对话内容。
        if !line.ends_with(b"\n") {
            break;
        }
        let Ok(record) = serde_json::from_slice::<Value>(line) else {
            truncated = true;
            continue;
        };
        if identity.kind == "codex" {
            codex(&record, &id, &mut messages, &mut calls);
        } else {
            claude(&record, identity, &id, &mut messages, &mut calls)?;
        }
    }
    let mut kept = 0;
    let mut first = messages.len();
    let length = messages.len();
    for (index, message) in messages.iter_mut().enumerate().rev() {
        message.truncated |= trim(&mut message.text, MESSAGE_BYTES);
        if let Some(detail) = &mut message.detail {
            message.truncated |= trim(detail, MESSAGE_BYTES);
        }
        let size = message.text.len() + message.detail.as_ref().map_or(0, String::len);
        if kept + size > OUTPUT_BYTES || length - first >= MESSAGE_COUNT {
            break;
        }
        kept += size;
        first = index;
    }
    let next = if first > 0 {
        truncated = true;
        messages.get(first).and_then(|m| m.id.split(':').next()?.parse().ok()).or(cursor)
    } else {
        cursor
    };
    messages.drain(..first);
    Ok((messages, next.filter(|before| *before < capture.end), truncated))
}

fn trim(text: &mut String, maximum: usize) -> bool {
    if text.len() <= maximum {
        return false;
    }
    let mut end = maximum;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    true
}

fn text_content(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_owned();
    }
    value
        .as_array()
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| {
                    matches!(
                        part["type"].as_str(),
                        Some("text" | "Text" | "input_text" | "output_text")
                    )
                    .then(|| part["text"].as_str())
                    .flatten()
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn push_message(
    messages: &mut Vec<Message>,
    id: String,
    role: &'static str,
    text: String,
    event: bool,
) {
    if text.trim().is_empty() {
        return;
    }
    // Codex 同时落 response_item 与 event_msg；只合并互为副本的两种来源，保留用户重复发送。
    if let Some(previous) = messages
        .iter_mut()
        .rev()
        .find(|m| m.role == role && m.text == text && m.event != event && !m.paired)
    {
        previous.paired = true;
        return;
    }
    messages.push(Message {
        id,
        role,
        text,
        name: None,
        detail: None,
        complete: true,
        truncated: false,
        paired: false,
        event,
    });
}

fn tool(
    messages: &mut Vec<Message>,
    calls: &mut HashMap<String, usize>,
    id: String,
    call: &str,
    name: &str,
    input: String,
) {
    calls.insert(call.to_owned(), messages.len());
    messages.push(Message {
        id,
        role: "tool",
        text: input,
        name: Some(name.to_owned()),
        detail: None,
        complete: false,
        truncated: false,
        paired: false,
        event: false,
    });
}

fn result(
    messages: &mut Vec<Message>,
    calls: &HashMap<String, usize>,
    id: String,
    call: &str,
    output: String,
) {
    if let Some(message) = calls.get(call).and_then(|index| messages.get_mut(*index)) {
        message.detail = Some(output);
        message.complete = true;
    } else {
        messages.push(Message {
            id,
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
}

fn codex(
    record: &Value,
    id: &str,
    messages: &mut Vec<Message>,
    calls: &mut HashMap<String, usize>,
) {
    let p = &record["payload"];
    match (record["type"].as_str(), p["type"].as_str()) {
        (Some("response_item"), Some("message"))
            if p["role"] == "assistant" && p["channel"] != "analysis" =>
        {
            // 原始 user 记录还包含环境和 AGENTS 上下文；真实输入以用户消息事件为准，
            // 不按正文前缀过滤，避免误删用户主动发送的 XML 或 Markdown。
            push_message(messages, id.into(), "assistant", text_content(&p["content"]), false);
        },
        (Some("event_msg"), Some("item_completed")) => {
            let item = &p["item"];
            let role = match item["type"].as_str() {
                Some("UserMessage") => "user",
                Some("AgentMessage") if item["phase"] != "analysis" => "assistant",
                _ => return,
            };
            push_message(messages, id.into(), role, text_content(&item["content"]), true);
        },
        (Some("event_msg"), Some("user_message")) => push_message(
            messages,
            id.into(),
            "user",
            p["message"].as_str().unwrap_or_default().into(),
            true,
        ),
        (Some("event_msg"), Some("agent_message")) => push_message(
            messages,
            id.into(),
            "assistant",
            p["message"].as_str().unwrap_or_default().into(),
            true,
        ),
        (Some("response_item"), Some("function_call" | "custom_tool_call")) => {
            tool(
                messages,
                calls,
                id.into(),
                p["call_id"].as_str().unwrap_or(id),
                p["name"].as_str().unwrap_or("tool"),
                p["arguments"].as_str().or_else(|| p["input"].as_str()).unwrap_or_default().into(),
            );
        },
        (Some("response_item"), Some("function_call_output" | "custom_tool_call_output")) => {
            result(
                messages,
                calls,
                id.into(),
                p["call_id"].as_str().unwrap_or_default(),
                text_content(&p["output"]),
            );
        },
        _ => {},
    }
}

fn claude(
    record: &Value,
    identity: &Identity,
    id: &str,
    messages: &mut Vec<Message>,
    calls: &mut HashMap<String, usize>,
) -> Result<(), ApiError> {
    let role = match record["type"].as_str() {
        Some("user") => "user",
        Some("assistant") => "assistant",
        _ => return Ok(()),
    };
    if record["isSidechain"] == true {
        return Ok(());
    }
    if record["sessionId"] != identity.session_id {
        return Err(ApiError::new(
            "conversation_identity_changed",
            "a transcript record belongs to another session",
        ));
    }
    let content = &record["message"]["content"];
    if let Some(text) = content.as_str() {
        push_message(messages, id.into(), role, text.into(), false);
    }
    if let Some(parts) = content.as_array() {
        for (index, part) in parts.iter().enumerate() {
            let block_id = format!("{id}:{index}");
            match part["type"].as_str() {
                Some("text") => push_message(
                    messages,
                    block_id,
                    role,
                    part["text"].as_str().unwrap_or_default().into(),
                    false,
                ),
                Some("tool_use") => tool(
                    messages,
                    calls,
                    block_id,
                    part["id"].as_str().unwrap_or(id),
                    part["name"].as_str().unwrap_or("tool"),
                    serde_json::to_string_pretty(&part["input"]).unwrap_or_default(),
                ),
                Some("tool_result") => result(
                    messages,
                    calls,
                    block_id,
                    part["tool_use_id"].as_str().unwrap_or_default(),
                    text_content(&part["content"]),
                ),
                _ => {},
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn identity(kind: &str) -> Identity {
        Identity {
            kind: kind.into(),
            session_id: "0199a213-c2a4-7cf5-8f6b-d746fbb6e86c".into(),
            epoch: Some(7),
        }
    }

    fn fixture(records: &[Value]) -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(format!("{}.jsonl", identity("claude").session_id));
        let mut file = File::create(&path).unwrap();
        for record in records {
            writeln!(file, "{record}").unwrap();
        }
        (directory, path)
    }

    #[test]
    fn native_codex_messages_deduplicate_events_and_keep_tools_without_reasoning() {
        let identity = identity("codex");
        let (_dir, path) = fixture(&[
            json!({"type":"session_meta","payload":{"id":identity.session_id}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"中文测试🙂"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"中文测试🙂"}]}}),
            json!({"type":"response_item","payload":{"type":"reasoning","summary":[{"text":"private reasoning"}]}}),
            json!({"type":"response_item","payload":{"type":"function_call","name":"exec","call_id":"tool-1","arguments":"{\"cmd\":\"pwd\"}"}}),
            json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"tool-1","output":"/project"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"## 回答\n`done`"}]}}),
            json!({"type":"event_msg","payload":{"type":"agent_message","message":"## 回答\n`done`"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"中文测试🙂"}}),
        ]);
        let capture = capture_file(&path, None).unwrap();
        validate_source(&identity, path.to_str().unwrap(), &capture.head).unwrap();
        let (messages, before, _) = project(&identity, &capture).unwrap();
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].text, "中文测试🙂");
        assert_eq!(messages[1].detail.as_deref(), Some("/project"));
        assert!(messages[1].complete);
        assert_eq!(messages[2].role, "assistant");
        assert!(before.is_none());
        assert!(!serde_json::to_string(&messages).unwrap().contains("private reasoning"));
        let mut other = identity.clone();
        other.session_id = "another-thread".into();
        assert!(validate_source(&other, path.to_str().unwrap(), &capture.head).is_err());
    }

    #[test]
    fn codex_completed_items_hide_injected_context_but_preserve_real_markup_and_repeats() {
        let identity = identity("codex");
        let input = "# AGENTS.md instructions\n<environment_context>用户正文</environment_context>";
        let (_dir, path) = fixture(&[
            json!({"type":"session_meta","payload":{"id":identity.session_id}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>internal fixture context</environment_context>"}]}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":input}]}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","content":[{"type":"text","text":input}]}}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"Reasoning","raw_content":["private reasoning"]}}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","phase":"final_answer","content":[{"type":"Text","text":"## 回答"}]}}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"## 回答"}]}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":input}]}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","content":[{"type":"text","text":input}]}}}),
        ]);
        let capture = capture_file(&path, None).unwrap();
        let (messages, _, truncated) = project(&identity, &capture).unwrap();
        assert!(!truncated);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].text, input);
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].text, "## 回答");
        assert_eq!(messages[2].text, input);
        assert_ne!(messages[0].id, messages[2].id);
    }

    #[test]
    fn codex_legacy_user_events_do_not_promote_raw_context_or_analysis() {
        let identity = identity("codex");
        let (_dir, path) = fixture(&[
            json!({"type":"session_meta","payload":{"id":identity.session_id}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"internal context"}]}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"real question"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","channel":"analysis","content":[{"type":"output_text","text":"private reasoning"}]}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","phase":"analysis","content":[{"type":"Text","text":"private reasoning"}]}}}),
            json!({"type":"event_msg","payload":{"type":"agent_message","message":"answer"}}),
        ]);
        let capture = capture_file(&path, None).unwrap();
        let (messages, _, _) = project(&identity, &capture).unwrap();
        assert_eq!(
            messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(),
            ["real question", "answer"]
        );
    }

    #[test]
    fn claude_records_require_the_owner_and_partial_writes_wait_for_a_newline() {
        let identity = identity("claude");
        let (_dir, path) = fixture(&[
            json!({"type":"user","sessionId":identity.session_id,"message":{"content":"question"}}),
            json!({"type":"assistant","sessionId":identity.session_id,"message":{"content":[{"type":"text","text":"answer"},{"type":"tool_use","id":"x","name":"Read","input":{"path":"src/main.rs"}}]}}),
            json!({"type":"user","sessionId":identity.session_id,"message":{"content":[{"type":"tool_result","tool_use_id":"x","content":[{"type":"text","text":"file text"}]}]}}),
        ]);
        File::options()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{\"type\":\"assistant\"")
            .unwrap();
        let capture = capture_file(&path, None).unwrap();
        let (messages, _, _) = project(&identity, &capture).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[2].detail.as_deref(), Some("file text"));
        let mut other = identity.clone();
        other.session_id = "other".into();
        assert!(project(&other, &capture).is_err());
        assert!(validate_source(&other, path.to_str().unwrap(), &capture.head).is_err());
    }

    #[test]
    fn native_pages_have_utf8_and_history_budgets_and_epoch_checks_are_exact() {
        let identity = identity("codex");
        let text = "中🙂".repeat(100_000);
        let (_dir, path) = fixture(&[
            json!({"type":"session_meta","payload":{"id":identity.session_id}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}}),
        ]);
        let capture = capture_file(&path, None).unwrap();
        let (messages, _, _) = project(&identity, &capture).unwrap();
        assert!(messages[0].truncated);
        assert!(messages[0].text.len() <= MESSAGE_BYTES);
        assert!(text.starts_with(&messages[0].text));
        assert!(identity.matches(&identity));
        assert!(!identity.matches(&Identity { epoch: Some(8), ..identity.clone() }));
        assert!(Identity { epoch: None, ..identity.clone() }.matches(&identity));
        assert!(capture_file(&path, Some(capture.end + 1)).is_err());
    }
}
