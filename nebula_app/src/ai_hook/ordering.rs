//! Per-stream ordering and bounded deduplication, after pane routing.

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};

use super::{AiHookEvent, AiHookKind};

const MAX_TRACKED_STREAMS: usize = 512;
const MAX_EVENT_IDS_PER_STREAM: usize = 64;
const DUPLICATE_WINDOW_MS: u64 = 1_500;
static EVENT_GATE: LazyLock<Mutex<AiHookEventGate>> =
    LazyLock::new(|| Mutex::new(AiHookEventGate::default()));

impl AiHookEvent {
    fn stream_key(&self, pane: Option<u64>) -> AiHookStreamKey {
        AiHookStreamKey {
            source: self.source.clone(),
            session_id: if self.source == "pi" && self.bridge_instance.is_some() {
                self.bridge_instance.clone()
            } else {
                self.session_id.clone()
            },
            pane,
            agent_pid: self.agent_pid,
            remote_process: self.remote_process.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AiHookStreamKey {
    source: String,
    session_id: Option<String>,
    pane: Option<u64>,
    /// agent 的进程身份。同一个 pane 里主 agent 与它 spawn 的子代理各占一条
    /// 流：两者的 session id 也不同，但那个值可能缺失，pid 不会。
    agent_pid: Option<u32>,
    remote_process: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamLifecycle {
    Active,
    Blocked,
    Done,
    Ended,
}

#[derive(Debug)]
struct AiHookStreamState {
    last_bridge_sequence: Option<u64>,
    last_occurred_at_ms: Option<u64>,
    last_received_sequence: u64,
    lifecycle: StreamLifecycle,
    seen_event_ids: VecDeque<String>,
    last_fingerprint: Option<(u64, u64)>,
}

impl AiHookStreamState {
    fn new(event: &AiHookEvent) -> Self {
        Self {
            last_bridge_sequence: None,
            last_occurred_at_ms: None,
            last_received_sequence: event.received_sequence,
            lifecycle: StreamLifecycle::Active,
            seen_event_ids: VecDeque::new(),
            last_fingerprint: None,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct AiHookEventGate {
    streams: HashMap<AiHookStreamKey, AiHookStreamState>,
}

/// 事件门的判定结果。带原因，而不只是一个 bool——「通知没出现」这类问题事后
/// 唯一的线索就是这个原因，日志里必须说得出是哪一条规则拦的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    Accepted,
    /// 同一个 `event_id` 已经处理过。
    DuplicateEventId,
    /// bridge 序号不比上一次大（重放或迟到）。
    StaleSequence,
    /// 没有序号，但 provider 时间戳比上一次早。
    StaleTime,
    /// 完全没有身份元数据的终态事件，在短窗口内重复抵达。
    DuplicateFingerprint,
    /// 该 session 已经 SessionEnd，只有 SessionStart 能复活。
    AfterSessionEnd,
    /// Done 之后抵达的 ToolComplete，且没有任何证据证明它更新。
    UnorderedAfterDone,
}

impl GateVerdict {
    pub fn accepted(self) -> bool {
        self == Self::Accepted
    }
}

impl AiHookEventGate {
    #[cfg(test)]
    pub(super) fn accept(&mut self, event: &AiHookEvent, pane_id: u64) -> bool {
        self.verdict(event, pane_id).accepted()
    }

    pub(super) fn verdict(&mut self, event: &AiHookEvent, pane_id: u64) -> GateVerdict {
        let key = event.stream_key(Some(pane_id));
        if !self.streams.contains_key(&key) && self.streams.len() >= MAX_TRACKED_STREAMS {
            if let Some(oldest) = self
                .streams
                .iter()
                .min_by_key(|(_, state)| state.last_received_sequence)
                .map(|(key, _)| key.clone())
            {
                self.streams.remove(&oldest);
            }
        }
        let state = self.streams.entry(key).or_insert_with(|| AiHookStreamState::new(event));

        if let Some(event_id) = event.event_id.as_deref()
            && state.seen_event_ids.iter().any(|seen| seen == event_id)
        {
            return GateVerdict::DuplicateEventId;
        }

        let provider_order = event
            .bridge_sequence
            .zip(state.last_bridge_sequence)
            .map(|(current, previous)| current.cmp(&previous));
        let time_order = event
            .occurred_at_ms
            .zip(state.last_occurred_at_ms)
            .map(|(current, previous)| current.cmp(&previous));
        if provider_order.is_some_and(|order| order != std::cmp::Ordering::Greater) {
            return GateVerdict::StaleSequence;
        }
        if provider_order.is_none() && time_order == Some(std::cmp::Ordering::Less) {
            return GateVerdict::StaleTime;
        }
        let strictly_newer = provider_order == Some(std::cmp::Ordering::Greater)
            || (provider_order.is_none() && time_order == Some(std::cmp::Ordering::Greater));

        let fingerprint = event_fingerprint(event);
        let use_fingerprint = event.event_id.is_none()
            && event.bridge_sequence.is_none()
            && event.occurred_at_ms.is_none()
            && matches!(
                event.kind,
                AiHookKind::TurnDone | AiHookKind::NeedsAttention | AiHookKind::SessionEnd
            );
        if use_fingerprint
            && let Some((previous, at)) = state.last_fingerprint
            && previous == fingerprint
            && event.received_at_ms.saturating_sub(at) <= DUPLICATE_WINDOW_MS
        {
            return GateVerdict::DuplicateFingerprint;
        }

        match state.lifecycle {
            StreamLifecycle::Ended if event.kind != AiHookKind::SessionStart => {
                return GateVerdict::AfterSessionEnd;
            },
            // Permission granted 后 PostToolUse 合法地把 Blocked 拉回 Working。
            StreamLifecycle::Blocked => {},
            // Done 后的无序 ToolComplete 最常见于迟到 Hook。只有 bridge
            // sequence/time 明确证明更新，或发送端保证串行时才允许恢复。
            StreamLifecycle::Done
                if event.kind == AiHookKind::ToolComplete
                    && !strictly_newer
                    && !(event.capabilities().serialized_delivery
                        && event.bridge_sequence.is_some()) =>
            {
                return GateVerdict::UnorderedAfterDone;
            },
            _ => {},
        }

        if event.kind == AiHookKind::SessionStart {
            state.seen_event_ids.clear();
        }
        if let Some(event_id) = event.event_id.as_deref() {
            if state.seen_event_ids.len() == MAX_EVENT_IDS_PER_STREAM {
                state.seen_event_ids.pop_front();
            }
            state.seen_event_ids.push_back(event_id.to_owned());
        }
        if let Some(sequence) = event.bridge_sequence {
            state.last_bridge_sequence = Some(sequence);
        }
        if let Some(occurred_at_ms) = event.occurred_at_ms {
            state.last_occurred_at_ms = Some(occurred_at_ms);
        }
        state.last_received_sequence = state.last_received_sequence.max(event.received_sequence);
        state.last_fingerprint = use_fingerprint.then_some((fingerprint, event.received_at_ms));
        state.lifecycle = match event.kind {
            AiHookKind::SessionStart | AiHookKind::PromptSubmit | AiHookKind::ToolComplete => {
                StreamLifecycle::Active
            },
            AiHookKind::TurnDone if event.active_background_tasks() > 0 => StreamLifecycle::Active,
            AiHookKind::TurnDone => StreamLifecycle::Done,
            AiHookKind::NeedsAttention => StreamLifecycle::Blocked,
            AiHookKind::SessionEnd => StreamLifecycle::Ended,
        };
        GateVerdict::Accepted
    }
}

fn event_fingerprint(event: &AiHookEvent) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    event.kind.hash(&mut hasher);
    event.message.hash(&mut hasher);
    event.answer.hash(&mut hasher);
    event.background_tasks.hash(&mut hasher);
    if let Some(attention) = event.attention.as_ref() {
        attention.cwd.hash(&mut hasher);
        attention.project.hash(&mut hasher);
        attention.permission_or_tool.hash(&mut hasher);
        attention.raw_context.hash(&mut hasher);
    }
    hasher.finish()
}

/// 在最终 Pane 已解析后调用。全进程共用一扇门，关闭/跨窗口移动期间不会为
/// 每个 view 留下互相矛盾的事件缓存；pane id 在本进程生命周期内不复用。
///
/// 返回带原因的判定：调用方必须把原因记进日志。事件被静默丢掉是这套链路里最
/// 难查的一类故障——用户看到的只是「通知没出现」。
pub(crate) fn accept_for_pane(event: &AiHookEvent, pane_id: u64) -> GateVerdict {
    EVENT_GATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).verdict(event, pane_id)
}

/// 同一 pump 批次内，只有一组事件全部带 bridge sequence 时才按该序号
/// 重排；不同会话仍占据原来的交错槽位。跨批次的旧序号由事件门拒绝。
pub(crate) fn reorder_batch(events: Vec<AiHookEvent>) -> Vec<AiHookEvent> {
    let keys = events.iter().map(|event| event.stream_key(event.pane)).collect::<Vec<_>>();
    let mut groups: HashMap<AiHookStreamKey, VecDeque<AiHookEvent>> = HashMap::new();
    for (key, event) in keys.iter().cloned().zip(events) {
        groups.entry(key).or_default().push_back(event);
    }
    for group in groups.values_mut() {
        if group.len() > 1 && group.iter().all(|event| event.bridge_sequence.is_some()) {
            let mut ordered = group.drain(..).collect::<Vec<_>>();
            ordered.sort_by_key(|event| event.bridge_sequence);
            group.extend(ordered);
        }
    }
    keys.into_iter().filter_map(|key| groups.get_mut(&key).and_then(VecDeque::pop_front)).collect()
}
