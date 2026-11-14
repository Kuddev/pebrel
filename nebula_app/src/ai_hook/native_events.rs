//! Native lifecycle hooks that carry their event in the helper argv.
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, ErrorKind, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::payload::{attention_message, context_string};
use super::{AiHookKind, AiTurnOutcome};
use crate::platform::regular_file::{
    RegularProbe, is_dir_and_ancestors_no_symlink, is_dir_no_symlink, open_regular_under_root,
    paths_equal_lexically, probe_regular_under_root,
};

const MAX_TRACKED_ANTIGRAVITY_CONVERSATIONS: usize = 64;
const MAX_TRANSCRIPT_LINE_BYTES: u64 = 256 * 1024;
const MAX_BRAIN_SCAN_DIRS: usize = 4_096;
const SUBAGENT_PROBE_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AntigravityVerdict {
    Primary,
    Subagent,
    Unverified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CachedConversation {
    Subagent,
    NonSystemStepZero,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrainScan {
    RegisteredSubagent,
    NotFoundComplete,
    Incomplete,
}

static ANTIGRAVITY_CONVERSATIONS: LazyLock<Mutex<VecDeque<(PathBuf, CachedConversation)>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));
static ACTIVE_ANTIGRAVITY_FOREGROUND: LazyLock<Mutex<HashMap<String, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static PROBE_WORKER: LazyLock<ProbeWorker> = LazyLock::new(ProbeWorker::new);

#[derive(Debug)]
pub(in crate::ai_hook) struct AntigravityForegroundLease {
    session_id: String,
}

impl AntigravityForegroundLease {
    pub(in crate::ai_hook) fn acquire(session_id: &str) -> Self {
        let mut active =
            ACTIVE_ANTIGRAVITY_FOREGROUND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *active.entry(session_id.to_owned()).or_insert(0) += 1;
        Self { session_id: session_id.to_owned() }
    }
}

impl Clone for AntigravityForegroundLease {
    fn clone(&self) -> Self {
        Self::acquire(&self.session_id)
    }
}

impl Drop for AntigravityForegroundLease {
    fn drop(&mut self) {
        let mut active =
            ACTIVE_ANTIGRAVITY_FOREGROUND.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(count) = active.get_mut(&self.session_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                active.remove(&self.session_id);
            }
        }
    }
}

fn is_confirmed_antigravity_foreground(session_id: &str) -> bool {
    ACTIVE_ANTIGRAVITY_FOREGROUND
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(session_id)
        .copied()
        .unwrap_or(0)
        > 0
}

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
/// internal parent fields from external JSON payloads. Require positive primary
/// verification when local path metadata is supplied so subagents and unverified
/// envelopes (timeout, full queue, symlinked roots, incomplete scans) never
/// replace foreground pane ownership.
pub(super) fn classify_antigravity_envelope(
    payload: &Value,
    conversation_id: Option<&str>,
    local_transport: bool,
) -> AntigravityVerdict {
    classify_antigravity_envelope_with_worker(
        payload,
        conversation_id,
        local_transport,
        &trusted_antigravity_brain_roots(),
        &PROBE_WORKER,
        SUBAGENT_PROBE_TIMEOUT,
    )
}

fn classify_antigravity_envelope_with_worker(
    payload: &Value,
    conversation_id: Option<&str>,
    local_transport: bool,
    trusted_roots: &[PathBuf],
    worker: &ProbeWorker,
    timeout: Duration,
) -> AntigravityVerdict {
    if antigravity_payload_marks_subagent(payload, conversation_id) {
        return AntigravityVerdict::Subagent;
    }
    if !local_transport {
        return AntigravityVerdict::Primary;
    }
    let has_artifact = payload.get("artifactDirectoryPath").is_some_and(|v| !v.is_null());
    let has_transcript = payload.get("transcriptPath").is_some_and(|v| !v.is_null());
    if !has_artifact && !has_transcript {
        return AntigravityVerdict::Primary;
    }
    let Some((brain_root, conversation_dir, id)) =
        validated_antigravity_conversation_dir(payload, conversation_id, trusted_roots)
    else {
        return AntigravityVerdict::Unverified;
    };
    let (step_zero_non_system, known_parent_ids) = {
        let cache =
            ANTIGRAVITY_CONVERSATIONS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut step_zero_ok = false;
        if let Some((_, state)) = cache.iter().find(|(dir, _)| dir == &conversation_dir) {
            match state {
                CachedConversation::Subagent => return AntigravityVerdict::Subagent,
                CachedConversation::NonSystemStepZero => step_zero_ok = true,
            }
        }
        let parents = cache
            .iter()
            .filter(|(dir, _)| {
                dir.parent() == Some(brain_root.as_path()) && *dir != conversation_dir
            })
            .filter_map(|(dir, _)| dir.file_name().and_then(|n| n.to_str()).map(str::to_owned))
            .collect::<Vec<_>>();
        (step_zero_ok, parents)
    };
    let confirmed_foreground = is_confirmed_antigravity_foreground(&id);
    let deadline = Instant::now() + timeout;
    worker
        .run(timeout, move || {
            probe_antigravity_conversation_on_disk(
                &brain_root,
                conversation_dir,
                &id,
                step_zero_non_system,
                &known_parent_ids,
                confirmed_foreground,
                deadline,
            )
        })
        .unwrap_or(AntigravityVerdict::Unverified)
}

fn probe_antigravity_conversation_on_disk(
    brain_root: &Path,
    conversation_dir: PathBuf,
    id: &str,
    step_zero_non_system: bool,
    known_parent_ids: &[String],
    confirmed_foreground: bool,
    deadline: Instant,
) -> AntigravityVerdict {
    if !is_dir_and_ancestors_no_symlink(brain_root) || !is_dir_no_symlink(&conversation_dir) {
        return AntigravityVerdict::Unverified;
    }
    if !step_zero_non_system {
        let Some(first_source) = transcript_first_step_source(brain_root, id) else {
            return AntigravityVerdict::Unverified;
        };
        if first_source == "SYSTEM" {
            remember_antigravity_conversation(conversation_dir, CachedConversation::Subagent);
            return AntigravityVerdict::Subagent;
        }
        remember_antigravity_conversation(
            conversation_dir.clone(),
            CachedConversation::NonSystemStepZero,
        );
    }
    if Instant::now() >= deadline {
        return AntigravityVerdict::Unverified;
    }
    match subagent_registered_in_brain(
        brain_root,
        id,
        known_parent_ids,
        confirmed_foreground,
        deadline,
    ) {
        BrainScan::RegisteredSubagent => {
            remember_antigravity_conversation(conversation_dir, CachedConversation::Subagent);
            AntigravityVerdict::Subagent
        },
        BrainScan::NotFoundComplete => AntigravityVerdict::Primary,
        BrainScan::Incomplete => AntigravityVerdict::Unverified,
    }
}

fn subagent_registered_in_brain(
    brain_root: &Path,
    id: &str,
    known_parent_ids: &[String],
    confirmed_foreground: bool,
    deadline: Instant,
) -> BrainScan {
    let record_name = format!("{id}.json");
    let mut saw_indeterminate = false;
    for parent_id in known_parent_ids {
        match probe_regular_under_root(
            brain_root,
            &[parent_id, ".system_generated", "subagents", &record_name],
        ) {
            RegularProbe::RegularFile => return BrainScan::RegisteredSubagent,
            RegularProbe::NotFound => {},
            RegularProbe::InvalidOrError => saw_indeterminate = true,
        }
    }
    if confirmed_foreground {
        return if saw_indeterminate { BrainScan::Incomplete } else { BrainScan::NotFoundComplete };
    }
    let Ok(entries) = std::fs::read_dir(brain_root) else {
        return BrainScan::Incomplete;
    };
    let mut scanned = 0;
    for entry in entries {
        if Instant::now() >= deadline || scanned >= MAX_BRAIN_SCAN_DIRS {
            return BrainScan::Incomplete;
        }
        let Ok(entry) = entry else {
            return BrainScan::Incomplete;
        };
        let Ok(file_type) = entry.file_type() else {
            return BrainScan::Incomplete;
        };
        let file_name = entry.file_name();
        let Some(segment) = file_name.to_str() else {
            continue;
        };
        if segment == id || !valid_antigravity_conversation_segment(segment) {
            continue;
        }
        if file_type.is_symlink() {
            saw_indeterminate = true;
            continue;
        }
        if !file_type.is_dir() || known_parent_ids.iter().any(|known| known == segment) {
            continue;
        }
        scanned += 1;
        match probe_regular_under_root(
            brain_root,
            &[segment, ".system_generated", "subagents", &record_name],
        ) {
            RegularProbe::RegularFile => return BrainScan::RegisteredSubagent,
            RegularProbe::NotFound => {},
            RegularProbe::InvalidOrError => saw_indeterminate = true,
        }
    }
    if saw_indeterminate { BrainScan::Incomplete } else { BrainScan::NotFoundComplete }
}

fn trusted_antigravity_brain_roots() -> Vec<PathBuf> {
    let mut roots = Vec::with_capacity(2);
    #[cfg(test)]
    TEST_TRUSTED_BRAIN_ROOT.with(|slot| {
        if let Some(root) = slot.borrow().as_ref() {
            roots.push(root.clone());
        }
    });
    if let Some(cli_dir) = super::local::antigravity::directory() {
        roots.push(cli_dir.join("brain"));
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
    (path.is_absolute()
        && !path.components().any(|c| matches!(c, Component::CurDir | Component::ParentDir)))
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

fn validated_transcript_dir(
    raw: &str,
    expected_id: Option<&str>,
    trusted_roots: &[PathBuf],
) -> Option<(PathBuf, PathBuf, String)> {
    let transcript = clean_absolute_path(raw)?;
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
    validated_brain_dir(generated.parent()?, expected_id, trusted_roots)
}

fn validated_antigravity_conversation_dir(
    payload: &Value,
    conversation_id: Option<&str>,
    trusted_roots: &[PathBuf],
) -> Option<(PathBuf, PathBuf, String)> {
    let artifact = match payload.get("artifactDirectoryPath") {
        None | Some(Value::Null) => None,
        Some(val) => Some(validated_brain_dir(
            clean_absolute_path(val.as_str()?)?,
            conversation_id,
            trusted_roots,
        )?),
    };
    let transcript = match payload.get("transcriptPath") {
        None | Some(Value::Null) => None,
        Some(val) => Some(validated_transcript_dir(val.as_str()?, conversation_id, trusted_roots)?),
    };
    match (artifact, transcript) {
        (Some(a), Some(t)) => (a == t).then_some(a),
        (Some(a), None) => Some(a),
        (None, Some(t)) => Some(t),
        (None, None) => None,
    }
}

fn transcript_first_step_source(brain_root: &Path, id: &str) -> Option<String> {
    let file = match open_regular_under_root(
        brain_root,
        &[id, ".system_generated", "logs", "transcript.jsonl"],
    ) {
        Ok(file) => file,
        Err(err) if err.kind() == ErrorKind::NotFound => open_regular_under_root(
            brain_root,
            &[id, ".system_generated", "logs", "transcript_full.jsonl"],
        )
        .ok()?,
        Err(_) => return None,
    };
    let mut reader = BufReader::new(file.take(MAX_TRANSCRIPT_LINE_BYTES + 1));
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    if line.len() as u64 > MAX_TRANSCRIPT_LINE_BYTES {
        return None;
    }
    let step: Value = serde_json::from_str(line.trim()).ok()?;
    step.get("source").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned)
}

fn remember_antigravity_conversation(conversation_dir: PathBuf, state: CachedConversation) {
    let mut cache =
        ANTIGRAVITY_CONVERSATIONS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((_, cached)) = cache.iter_mut().find(|(dir, _)| dir == &conversation_dir) {
        if state == CachedConversation::Subagent {
            *cached = CachedConversation::Subagent;
        }
        return;
    }
    if cache.len() >= MAX_TRACKED_ANTIGRAVITY_CONVERSATIONS {
        cache.pop_front();
    }
    cache.push_back((conversation_dir, state));
}

#[cfg(test)]
mod subagent_tests;

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
}
