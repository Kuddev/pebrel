//! One pane's Agent lifecycle, shared by both UI shells.
//!
//! Hook events are facts; screen matches are observations with limited authority.
//! Command ownership ends at the shell boundary, never because output goes quiet.

use crate::ai_agents::{AgentStatus, AgentStatusSource, Detection};
use std::collections::HashSet;

use super::{AiHookEvent, AiHookKind};

/// A stop from these sources proves nothing about the answer unless it carries a result, so
/// an unclassified one returns the pane to idle instead of announcing completion.
const UNPROVEN_STOP: [&str; 6] = ["pi", "omp", "cursor", "copilot", "antigravity", "opencode"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum HookCoverage {
    #[default]
    None,
    Completion,
    Turns,
    Lifecycle,
}

#[derive(Debug, Clone)]
struct Owner {
    source: String,
    session: Option<String>,
    pid: Option<u32>,
    remote_process: Option<String>,
    bridge_instance: Option<String>,
}

/// Owns status and arbitration. Adapters may submit facts but cannot set fields.
#[derive(Debug, Clone)]
pub(crate) struct AgentActivity {
    status: AgentStatus,
    source: AgentStatusSource,
    rule: Option<String>,
    coverage: HookCoverage,
    owner: Option<Owner>,
    idle_samples: u8,
    unmatched_samples: u8,
    /// Only a preceding Working state may survive a screen form's expiry.
    screen_blocked_fallback: Option<(AgentStatusSource, Option<String>)>,
    turn_observed: bool,
    pending_submit: bool,
    /// A Claude prompt was typed while the previous turn was still Working.
    /// The current turn's completion must still be recorded, but an automatic
    /// retry would submit one prompt too many. Cleared when the provider
    /// accepts a new prompt, a new turn is observed, or the session ends.
    queued_prompt: bool,
    /// A completion-only hook latches its result until an idle screen or an
    /// explicit submission separates the old frame from a new turn.
    screen_armed: bool,
    command_ended: bool,
    native_codex: bool,
    /// Fresh provider title evidence is separate from the underlying lifecycle.
    /// Removing a title marker cannot manufacture a completion or approval.
    native_title: Option<AgentStatus>,
    turn_id: Option<String>,
    /// Keep every prompt identity for this Claude session so delayed hooks
    /// cannot become valid again after a bounded history evicts them.
    claude_prompt_ids: HashSet<String>,
    /// Reject Claude events whose ingress sequence predates the last input,
    /// including raw remote envelopes parsed only after leaving the mailbox.
    submit_receive_sequence: u64,
    last_remote_sequence: Option<u64>,
    /// Reject title snapshots captured before a newer submission or Hook.
    last_activity_at: Option<std::time::Instant>,
}

impl Default for AgentActivity {
    fn default() -> Self {
        Self {
            status: AgentStatus::Unknown,
            source: AgentStatusSource::Unknown,
            rule: None,
            coverage: HookCoverage::None,
            owner: None,
            idle_samples: 0,
            unmatched_samples: 0,
            screen_blocked_fallback: None,
            turn_observed: false,
            pending_submit: false,
            queued_prompt: false,
            screen_armed: true,
            command_ended: false,
            native_codex: false,
            native_title: None,
            turn_id: None,
            claude_prompt_ids: HashSet::new(),
            submit_receive_sequence: 0,
            last_remote_sequence: None,
            last_activity_at: None,
        }
    }
}

impl AgentActivity {
    pub fn status(&self) -> AgentStatus {
        self.native_title.unwrap_or(self.status)
    }

    pub fn is_current_turn(&self, turn_id: &str) -> bool {
        self.turn_id.as_deref() == Some(turn_id)
    }

    /// Whether a prompt typed during the current turn is still waiting for its
    /// provider-confirmed identity. Automatic retry must not add another one.
    pub fn queued_prompt_pending(&self) -> bool {
        self.queued_prompt
    }

    pub fn source(&self) -> AgentStatusSource {
        if self.native_title.is_some() { AgentStatusSource::Title } else { self.source }
    }

    pub fn rule(&self) -> Option<&str> {
        match self.native_title {
            Some(AgentStatus::Blocked) => Some("codex.title.action_required"),
            Some(AgentStatus::Working) => Some("codex.title.working"),
            _ => self.rule.as_deref(),
        }
    }

    pub fn hook_seen(&self) -> bool {
        self.coverage != HookCoverage::None
    }

    pub fn primary_pid(&self) -> Option<u32> {
        self.owner.as_ref().and_then(|owner| owner.pid)
    }

    pub fn allows_screen(&self) -> bool {
        !self.command_ended
            && self.native_title.is_none()
            && self.coverage != HookCoverage::Lifecycle
    }

    /// The adapter supplies a real OSC update or a fresh authenticated remote pane
    /// sample from the already identified foreground Agent. Old snapshots must
    /// not override a newer Hook/submission. Title evidence never owns requests,
    /// session identity, results, or notifications.
    pub fn observe_native_title(
        &mut self,
        program: Option<&str>,
        title: &str,
        observed_at: std::time::Instant,
    ) -> bool {
        if self.command_ended
            || self.last_activity_at.is_some_and(|at| observed_at <= at)
            || self.owner.as_ref().is_some_and(|owner| {
                crate::ai_agents::AgentKind::parse(&owner.source)
                    != program.and_then(crate::ai_agents::AgentKind::parse)
            })
        {
            return false;
        }
        let signal = crate::ai_agents::native_title::activity(program, title);
        if signal == self.native_title {
            return false;
        }
        self.native_title = signal;
        self.reset_screen_samples();
        if signal.is_some() {
            self.pending_submit = false;
        }
        if signal == Some(AgentStatus::Working) {
            self.turn_observed = true;
            self.screen_armed = true;
            self.queued_prompt = false;
        }
        true
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// An explicit shell boundary also rejects delayed events from the old CLI.
    pub fn command_finished(&mut self) {
        self.reset();
        self.command_ended = true;
    }

    /// Launching an Agent is not evidence that it has answered a prompt.
    pub fn begin_command(&mut self, agent: bool) {
        self.reset();
        if agent {
            self.status = AgentStatus::Working;
            self.source = AgentStatusSource::Process;
        }
    }

    pub fn identify(&mut self, source: AgentStatusSource) {
        if !self.hook_seen() && self.status == AgentStatus::Unknown {
            self.source = source;
        }
    }

    /// A concrete runtime submission or accepted confirmation starts work. The
    /// previous input frame must not manufacture a completion before work starts.
    pub fn submitted(&mut self) {
        self.submitted_at(super::current_receive_sequence());
    }

    pub fn submitted_at(&mut self, received_sequence: u64) {
        self.native_title = None;
        self.status = AgentStatus::Working;
        self.source = AgentStatusSource::Process;
        self.rule = None;
        self.screen_blocked_fallback = None;
        self.turn_observed = true;
        self.pending_submit = true;
        self.queued_prompt = false;
        self.last_activity_at = Some(std::time::Instant::now());
        self.input_sent_at(received_sequence);
    }

    /// Input sent while Claude is still working may queue the next prompt.
    /// Record the current turn's completion normally, but do not auto-retry
    /// over the prompt the user already queued.
    pub fn queued_submit(&mut self) {
        self.pending_submit = true;
        self.queued_prompt = true;
        self.last_activity_at = Some(std::time::Instant::now());
        self.input_sent_at(super::current_receive_sequence());
    }

    pub fn submitted_text(&mut self, program: &str, text: &str) {
        self.submitted_text_at(program, text, super::current_receive_sequence());
    }

    pub fn submitted_text_at(&mut self, program: &str, text: &str, received_sequence: u64) {
        // Codex /plan 只切换交互模式，不触发任务 hook；若先制造 Running，
        // 对话输入会一直等一个不存在的完成事件。普通任务仍保留提交屏障。
        if program == "codex" && text.trim() == "/plan" {
            self.input_sent_at(received_sequence);
        } else {
            self.submitted_at(received_sequence);
        }
    }

    /// Physical Enter alone can also dismiss a menu. It only permits observing a
    /// new turn; it is not itself a successful submission or a completion.
    pub fn input_sent(&mut self) {
        self.input_sent_at(super::current_receive_sequence());
    }

    fn input_sent_at(&mut self, received_sequence: u64) {
        self.submit_receive_sequence = received_sequence;
        self.screen_armed = true;
        self.reset_screen_samples();
    }

    /// An unavailable or unidentified snapshot interrupts consecutive evidence
    /// without revoking a state. Only a known agent's real snapshot can do that.
    pub fn reset_screen_samples(&mut self) {
        self.idle_samples = 0;
        self.unmatched_samples = 0;
    }

    /// Check ownership before the ordering gate records this event. An ignored
    /// legacy notification must not reopen the gate's already completed stream.
    pub fn accepts_hook(&self, event: &AiHookEvent) -> bool {
        // Claude has no provider sequence. The ingress fence prevents old local
        // or queued remote events from crossing new input.
        if event.source == "claude" && event.received_sequence < self.submit_receive_sequence {
            return false;
        }
        if self.command_ended && event.kind != AiHookKind::SessionStart {
            return false;
        }
        if event.legacy_attention {
            return false;
        }
        // Both bridges stay installed for old clients and pending hook trust.
        // Once this session proves its native bridge is active, notify cannot
        // downgrade its coverage or emit a second completion notification.
        if self.native_codex && event.source == "codex" && event.codex_hooks.is_none() {
            return false;
        }
        if event.source == "claude"
            && event.kind == AiHookKind::PromptSubmit
            && event
                .turn_id
                .as_deref()
                .is_some_and(|turn_id| self.claude_prompt_ids.contains(turn_id))
        {
            return false;
        }
        // Claude provides no source sequence, so the provider-confirmed prompt
        // identity is the authority. A distinct unseen ID may start the next
        // turn even when a keybinding submitted it without a local Enter;
        // replayed or still-current IDs cannot consume the submit guard.
        if event.source == "claude"
            && event.kind == AiHookKind::PromptSubmit
            && self.turn_id.is_some()
            && (event.turn_id.is_none() || event.turn_id.as_deref() == self.turn_id.as_deref())
        {
            return false;
        }
        // An explicit local submission is waiting for its provider identity;
        // the previous turn's completion must not close the new turn. A plain
        // Enter during Working may have queued nothing, so its completion is
        // recorded normally and only the automatic retry is suppressed.
        if event.source == "claude"
            && self.pending_submit
            && !self.queued_prompt
            && event.kind != AiHookKind::SessionStart
            && event.kind != AiHookKind::PromptSubmit
            && self.turn_id.is_some()
            && event.turn_id.as_deref() == self.turn_id.as_deref()
        {
            return false;
        }
        if (event.codex_hooks.is_some()
            || event.source == "claude"
                && matches!(
                    event.kind,
                    AiHookKind::TurnDone | AiHookKind::NeedsAttention | AiHookKind::ToolComplete
                ))
            && self.turn_id.is_some()
            && event.turn_id.is_some()
            && self.turn_id != event.turn_id
            && !matches!(event.kind, AiHookKind::SessionStart | AiHookKind::PromptSubmit)
        {
            return false;
        }
        if event.kind == AiHookKind::TurnDone
            && (self.status == AgentStatus::Done
                || event.source == "claude" && self.status == AgentStatus::Idle)
            && event.turn_id.is_some()
            && event.turn_id == self.turn_id
        {
            return false;
        }
        if let Some(owner) = &self.owner {
            if owner.source != event.source {
                return false;
            }
            if owner.remote_process.is_some()
                && event.remote_process.is_some()
                && owner.remote_process != event.remote_process
            {
                return false;
            }
            if owner.remote_process.is_some()
                && owner.remote_process == event.remote_process
                && event
                    .bridge_sequence
                    .zip(self.last_remote_sequence)
                    .is_some_and(|(next, previous)| next <= previous)
            {
                return false;
            }
            let same_process = owner.pid.is_some() && owner.pid == event.agent_pid
                || owner.remote_process.is_some() && owner.remote_process == event.remote_process
                || matches!(event.source.as_str(), "pi" | "omp")
                    && owner.bridge_instance.is_some()
                    && owner.bridge_instance == event.bridge_instance;
            match (owner.pid, event.agent_pid) {
                (Some(primary), Some(incoming)) if primary != incoming => return false,
                _ if owner.session.is_some()
                    && event.session_id.is_some()
                    && owner.session != event.session_id
                    && !(same_process && event.kind == AiHookKind::SessionStart) =>
                {
                    return false;
                },
                _ => {},
            }
        }
        true
    }

    fn record_owner(&mut self, event: &AiHookEvent) {
        if let Some(owner) = &mut self.owner {
            owner.pid = owner.pid.or(event.agent_pid);
            if owner.remote_process.is_none() {
                owner.remote_process = event.remote_process.clone();
            }
            if owner.bridge_instance.is_none() {
                owner.bridge_instance = event.bridge_instance.clone();
            }
            if event.session_id.is_some() {
                owner.session = event.session_id.clone();
            }
        } else {
            self.owner = Some(Owner {
                source: event.source.clone(),
                session: event.session_id.clone(),
                pid: event.agent_pid,
                remote_process: event.remote_process.clone(),
                bridge_instance: event.bridge_instance.clone(),
            });
        }
    }

    /// Apply an already ordered event. A nested/foreign session has no authority
    /// over the primary pane, including its identity, attention and notifications.
    pub fn apply_hook(&mut self, event: &AiHookEvent) -> bool {
        if !self.accepts_hook(event) {
            return false;
        }
        if event.source == "claude"
            && event.kind == AiHookKind::SessionStart
            && !event.session_compacted
            && event.session_id.as_ref().is_some_and(|session_id| {
                self.owner.as_ref().and_then(|owner| owner.session.as_ref()) != Some(session_id)
            })
        {
            self.claude_prompt_ids.clear();
        }
        if event.source == "claude"
            && event.kind == AiHookKind::SessionStart
            && !event.session_compacted
        {
            self.queued_prompt = false;
        }
        if event.source == "claude" && event.kind == AiHookKind::PromptSubmit {
            self.queued_prompt = false;
        }
        if event.source == "claude"
            && event.kind == AiHookKind::PromptSubmit
            && let Some(turn_id) = event.turn_id.as_ref()
        {
            self.claude_prompt_ids.insert(turn_id.clone());
        }
        self.record_owner(event);
        self.native_title = None;
        self.native_codex |= event.codex_hooks.is_some();
        if event.remote_process.is_some() && event.bridge_sequence.is_some() {
            self.last_remote_sequence = event.bridge_sequence;
        }
        if event.turn_id.is_some()
            || (event.kind == AiHookKind::SessionStart && !event.session_compacted)
        {
            self.turn_id = event.turn_id.clone();
        }
        self.command_ended = false;
        if event.kind == AiHookKind::SessionEnd {
            self.command_finished();
            return true;
        }
        let capabilities = event.capabilities();
        let coverage = if capabilities.lifecycle {
            if capabilities.attention_events {
                HookCoverage::Lifecycle
            } else {
                HookCoverage::Turns
            }
        } else {
            HookCoverage::Completion
        };
        self.coverage = coverage;
        self.source = AgentStatusSource::Hook;
        self.rule = None;
        self.screen_blocked_fallback = None;
        self.reset_screen_samples();
        if event.kind != AiHookKind::SessionStart {
            self.pending_submit = false;
        }
        self.status = match event.kind {
            AiHookKind::SessionStart if event.session_compacted => self.status,
            AiHookKind::SessionStart => {
                self.turn_observed = false;
                AgentStatus::Idle
            },
            AiHookKind::PromptSubmit | AiHookKind::ToolComplete => {
                self.turn_observed = true;
                AgentStatus::Working
            },
            AiHookKind::TurnDone if event.active_background_tasks() > 0 => {
                self.turn_observed = true;
                self.rule = Some(format!(
                    "hook.background_tasks.active={}",
                    event.active_background_tasks()
                ));
                AgentStatus::Working
            },
            AiHookKind::TurnDone => {
                self.screen_armed = false;
                if matches!(
                    event.turn_outcome,
                    super::AiTurnOutcome::Failed
                        | super::AiTurnOutcome::Incomplete
                        | super::AiTurnOutcome::Cancelled
                ) || (UNPROVEN_STOP.contains(&event.source.as_str())
                    && event.turn_outcome == super::AiTurnOutcome::Unknown)
                {
                    AgentStatus::Idle
                } else {
                    AgentStatus::Done
                }
            },
            AiHookKind::NeedsAttention => AgentStatus::Blocked,
            AiHookKind::SessionEnd => unreachable!("handled above"),
        };
        self.last_activity_at = Some(std::time::Instant::now());
        true
    }

    /// Observe a real snapshot of a known agent. No match can revoke stale
    /// screen attention, but cannot establish idle or completion. Full lifecycle
    /// hooks never consult screen rules, even if a frame contains an old form.
    pub fn observe_screen(&mut self, detection: Option<Detection>) -> bool {
        if !self.allows_screen() {
            return false;
        }
        let Some(detection) = detection else {
            self.idle_samples = 0;
            if self.source != AgentStatusSource::Screen || self.status != AgentStatus::Blocked {
                return false;
            }
            self.unmatched_samples = self.unmatched_samples.saturating_add(1);
            if self.unmatched_samples < 2 {
                return false;
            }
            self.unmatched_samples = 0;
            if let Some((source, rule)) = self.screen_blocked_fallback.take() {
                self.status = AgentStatus::Working;
                self.source = source;
                self.rule = rule;
            } else {
                self.status = AgentStatus::Unknown;
                self.source = AgentStatusSource::Unknown;
                self.rule = None;
            }
            return true;
        };
        self.unmatched_samples = 0;
        let next = match detection.status {
            AgentStatus::Idle => {
                if self.pending_submit {
                    self.idle_samples = 0;
                    return false;
                }
                self.idle_samples = self.idle_samples.saturating_add(1);
                if self.idle_samples < 2 {
                    return false;
                }
                self.screen_armed = true;
                // Completion hooks own completion, even when start/permission
                // evidence has to come from the screen on older clients.
                if matches!(self.coverage, HookCoverage::Completion | HookCoverage::Turns) {
                    return false;
                }
                if self.turn_observed { AgentStatus::Done } else { AgentStatus::Idle }
            },
            status @ (AgentStatus::Working | AgentStatus::Blocked) => {
                self.idle_samples = 0;
                // Pi reports starts and completions, but has no attention hook.
                // Screen evidence may fill that gap only during a hook-owned
                // turn. It cannot start a turn or override the hook's result.
                if self.coverage == HookCoverage::Turns
                    && (status != AgentStatus::Blocked
                        || !matches!(self.status, AgentStatus::Working | AgentStatus::Blocked))
                {
                    return false;
                }
                if !self.screen_armed {
                    return false;
                }
                self.pending_submit = false;
                self.queued_prompt = false;
                self.turn_observed |= status == AgentStatus::Working;
                status
            },
            AgentStatus::Unknown | AgentStatus::Done => {
                self.idle_samples = 0;
                return false;
            },
        };
        if next == self.status {
            return false;
        }
        self.screen_blocked_fallback =
            if next == AgentStatus::Blocked && self.status == AgentStatus::Working {
                Some((self.source, self.rule.take()))
            } else {
                None
            };
        self.status = next;
        self.source = AgentStatusSource::Screen;
        self.rule = Some(detection.rule_id);
        true
    }

    /// Backdate the latest submission/hook boundary without sleeping in tests.
    #[cfg(test)]
    pub(crate) fn backdate_activity_for_test(&mut self, back: std::time::Duration) {
        if let Some(at) = &mut self.last_activity_at {
            *at = std::time::Instant::now().checked_sub(back).unwrap_or(std::time::Instant::now());
        }
    }
}

/// Notification policy is an edge, not the number of polling samples.
pub(crate) fn screen_notification(
    previous: AgentStatus,
    next: AgentStatus,
    hooks: bool,
) -> Option<bool> {
    match next {
        AgentStatus::Blocked if previous != AgentStatus::Blocked => Some(true),
        AgentStatus::Done if matches!(previous, AgentStatus::Working | AgentStatus::Blocked) => {
            (!hooks).then_some(false)
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests;
