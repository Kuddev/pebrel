//! Native lifecycle hooks that carry their event in the helper argv.
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::payload::{attention_message, context_string};
use super::{AiHookKind, AiTurnOutcome};
use crate::platform::regular_file::{
    is_dir_no_symlink, is_regular_no_symlink, open_regular_nonblocking, paths_equal_lexically,
};

const MAX_TRACKED_ANTIGRAVITY_CONVERSATIONS: usize = 64;
const MAX_TRANSCRIPT_LINE_BYTES: u64 = 256 * 1024;
const MAX_BRAIN_SCAN_DIRS: usize = 4_096;
const SUBAGENT_PROBE_TIMEOUT: Duration = Duration::from_millis(200);

static ANTIGRAVITY_CONVERSATIONS: LazyLock<Mutex<VecDeque<(PathBuf, bool)>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));
static PROBE_WORKER: LazyLock<ProbeWorker> = LazyLock::new(ProbeWorker::new);

#[cfg(test)]
std::thread_local! {
    static TEST_TRUSTED_BRAIN_ROOT: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

type ProbeJob = Box<dyn FnOnce() + Send>;

struct ProbeWorker {
    tx: Option<mpsc::SyncSender<ProbeJob>>,
}

impl ProbeWorker {
    fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel::<ProbeJob>(8);
        let spawned = std::thread::Builder::new()
            .name("pebrel-agy-probe".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    job();
                }
            })
            .is_ok();
        Self { tx: spawned.then_some(tx) }
    }

    fn run<T: Send + 'static>(
        &self,
        timeout: Duration,
        job: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        let tx = self.tx.as_ref()?;
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        tx.try_send(Box::new(move || {
            let _ = reply_tx.send(job());
        }))
        .ok()?;
        reply_rx.recv_timeout(timeout).ok()
    }
}

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
    is_antigravity_subagent_with_worker(
        payload,
        conversation_id,
        local_transport,
        &trusted_antigravity_brain_roots(),
        &PROBE_WORKER,
        SUBAGENT_PROBE_TIMEOUT,
    )
}

fn is_antigravity_subagent_with_worker(
    payload: &Value,
    conversation_id: Option<&str>,
    local_transport: bool,
    trusted_roots: &[PathBuf],
    worker: &ProbeWorker,
    timeout: Duration,
) -> bool {
    if antigravity_payload_marks_subagent(payload, conversation_id) {
        return true;
    }
    if !local_transport {
        return false;
    }
    let Some((brain_root, conversation_dir, id)) =
        validated_antigravity_conversation_dir(payload, conversation_id, trusted_roots)
    else {
        return false;
    };
    let known_in_brain = {
        let cache =
            ANTIGRAVITY_CONVERSATIONS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((_, is_subagent)) = cache.iter().find(|(dir, _)| dir == &conversation_dir) {
            return *is_subagent;
        }
        cache
            .iter()
            .filter(|(dir, _)| {
                dir.parent() == Some(brain_root.as_path()) && *dir != conversation_dir
            })
            .map(|(dir, _)| dir.clone())
            .collect::<Vec<_>>()
    };
    let deadline = Instant::now() + timeout;
    worker
        .run(timeout, move || {
            probe_antigravity_conversation_on_disk(
                &brain_root,
                conversation_dir,
                &id,
                &known_in_brain,
                deadline,
            )
        })
        .unwrap_or(false)
}

fn probe_antigravity_conversation_on_disk(
    brain_root: &Path,
    conversation_dir: PathBuf,
    id: &str,
    known_in_brain: &[PathBuf],
    deadline: Instant,
) -> bool {
    if !is_dir_no_symlink(brain_root) || !is_dir_no_symlink(&conversation_dir) {
        return false;
    }
    let first_source = transcript_first_step_source(&conversation_dir);
    if first_source.as_deref() == Some("SYSTEM") {
        remember_antigravity_conversation(conversation_dir, true);
        return true;
    }
    if Instant::now() >= deadline {
        return false;
    }
    let (registered_subagent, scan_complete) =
        subagent_registered_in_brain(brain_root, &conversation_dir, id, known_in_brain, deadline);
    if registered_subagent {
        remember_antigravity_conversation(conversation_dir, true);
        return true;
    }
    if first_source.is_some() && scan_complete {
        remember_antigravity_conversation(conversation_dir, false);
    }
    false
}

fn subagent_registered_in_brain(
    brain_root: &Path,
    conversation_dir: &Path,
    id: &str,
    known_in_brain: &[PathBuf],
    deadline: Instant,
) -> (bool, bool) {
    let record_name = format!("{id}.json");
    if known_in_brain.iter().any(|parent_dir| has_regular_subagent_record(parent_dir, &record_name))
    {
        return (true, true);
    }
    let Ok(entries) = std::fs::read_dir(brain_root) else {
        return (false, false);
    };
    let mut scanned = 0;
    for entry in entries {
        if Instant::now() >= deadline || scanned >= MAX_BRAIN_SCAN_DIRS {
            return (false, false);
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_ok_and(|ft| ft.is_dir()) {
            continue;
        }
        let file_name = entry.file_name();
        let Some(segment) = file_name.to_str() else { continue };
        if segment == id || !valid_antigravity_conversation_segment(segment) {
            continue;
        }
        let path = entry.path();
        if path == conversation_dir || known_in_brain.iter().any(|known| known == &path) {
            continue;
        }
        scanned += 1;
        if has_regular_subagent_record(&path, &record_name) {
            return (true, true);
        }
    }
    (false, true)
}

fn has_regular_subagent_record(parent_dir: &Path, record_name: &str) -> bool {
    if !is_dir_no_symlink(parent_dir) {
        return false;
    }
    let generated = parent_dir.join(".system_generated");
    if !is_dir_no_symlink(&generated) {
        return false;
    }
    let subagents = generated.join("subagents");
    is_dir_no_symlink(&subagents) && is_regular_no_symlink(&subagents.join(record_name))
}

fn trusted_antigravity_brain_roots() -> Vec<PathBuf> {
    let mut roots = Vec::with_capacity(3);
    #[cfg(test)]
    TEST_TRUSTED_BRAIN_ROOT.with(|slot| {
        if let Some(root) = slot.borrow().as_ref() {
            roots.push(root.clone());
        }
    });
    if let Some(home) = crate::platform::dirs::home_dir() {
        let gemini = home.join(".gemini");
        roots.push(gemini.join("antigravity-cli").join("brain"));
        roots.push(gemini.join("antigravity").join("brain"));
    }
    roots
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

fn validated_brain_dir(
    dir: &Path,
    expected_id: Option<&str>,
    trusted_roots: &[PathBuf],
) -> Option<(PathBuf, PathBuf, String)> {
    let segment = dir.file_name()?.to_str()?;
    let brain_dir = dir.parent()?;
    if !valid_antigravity_conversation_segment(segment)
        || expected_id.is_some_and(|id| id != segment)
        || brain_dir.file_name()?.to_str()? != "brain"
        || !trusted_roots.iter().any(|root| paths_equal_lexically(brain_dir, root))
    {
        return None;
    }
    Some((brain_dir.to_path_buf(), dir.to_path_buf(), segment.to_owned()))
}

fn validated_antigravity_conversation_dir(
    payload: &Value,
    conversation_id: Option<&str>,
    trusted_roots: &[PathBuf],
) -> Option<(PathBuf, PathBuf, String)> {
    if let Some(found) = payload
        .get("artifactDirectoryPath")
        .and_then(Value::as_str)
        .and_then(clean_absolute_path)
        .and_then(|dir| validated_brain_dir(dir, conversation_id, trusted_roots))
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
    validated_brain_dir(generated.parent()?, conversation_id, trusted_roots)
}

fn transcript_first_step_source(conversation_dir: &Path) -> Option<String> {
    let generated = conversation_dir.join(".system_generated");
    if !is_dir_no_symlink(&generated) {
        return None;
    }
    let logs = generated.join("logs");
    if !is_dir_no_symlink(&logs) {
        return None;
    }
    let file = open_regular_nonblocking(&logs.join("transcript.jsonl"))
        .or_else(|_| open_regular_nonblocking(&logs.join("transcript_full.jsonl")))
        .ok()?;
    let mut reader = BufReader::new(file.take(MAX_TRANSCRIPT_LINE_BYTES + 1));
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    if line.len() as u64 > MAX_TRANSCRIPT_LINE_BYTES {
        return None;
    }
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
mod tests;
