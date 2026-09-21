//! One pane's Agent lifecycle, shared by both UI shells.
//!
//! Hook events are facts; screen matches are observations with limited authority.
//! Command ownership ends at the shell boundary, never because output goes quiet.

use crate::ai_agents::{AgentStatus, AgentStatusSource, Detection};

use super::{AiHookEvent, AiHookKind};

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
    turn_observed: bool,
    pending_submit: bool,
    /// A completion-only hook latches its result until an idle screen or an
    /// explicit submission separates the old frame from a new turn.
    screen_armed: bool,
    command_ended: bool,
    native_codex: bool,
    turn_id: Option<String>,
    last_remote_sequence: Option<u64>,
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
            turn_observed: false,
            pending_submit: false,
            screen_armed: true,
            command_ended: false,
            native_codex: false,
            turn_id: None,
            last_remote_sequence: None,
        }
    }
}

impl AgentActivity {
    pub fn status(&self) -> AgentStatus {
        self.status
    }
    pub fn source(&self) -> AgentStatusSource {
        self.source
    }
    pub fn rule(&self) -> Option<&str> {
        self.rule.as_deref()
    }
    pub fn hook_seen(&self) -> bool {
        self.coverage != HookCoverage::None
    }
    pub fn primary_pid(&self) -> Option<u32> {
        self.owner.as_ref().and_then(|owner| owner.pid)
    }
    pub fn allows_screen(&self) -> bool {
        !self.command_ended && self.coverage != HookCoverage::Lifecycle
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
        self.status = AgentStatus::Working;
        self.source = AgentStatusSource::Process;
        self.rule = None;
        self.turn_observed = true;
        self.pending_submit = true;
        self.input_sent();
    }

    /// Physical Enter alone can also dismiss a menu. It only permits observing a
    /// new turn; it is not itself a successful submission or a completion.
    pub fn input_sent(&mut self) {
        self.screen_armed = true;
        self.idle_samples = 0;
    }

    /// Check ownership before the ordering gate records this event. An ignored
    /// legacy notification must not reopen the gate's already completed stream.
    pub fn accepts_hook(&self, event: &AiHookEvent) -> bool {
        if self.command_ended && event.kind != AiHookKind::SessionStart {
            return false;
        }
        if event.legacy_attention
            && !matches!(self.status, AgentStatus::Working | AgentStatus::Blocked)
        {
            return false;
        }
        // Both bridges stay installed for old clients and pending hook trust.
        // Once this session proves its native bridge is active, notify cannot
        // downgrade its coverage or emit a second completion notification.
        if self.native_codex && event.source == "codex" && event.codex_hooks.is_none() {
            return false;
        }
        if event.codex_hooks.is_some()
            && self.turn_id.is_some()
            && event.turn_id.is_some()
            && self.turn_id != event.turn_id
            && !matches!(event.kind, AiHookKind::SessionStart | AiHookKind::PromptSubmit)
        {
            return false;
        }
        if event.kind == AiHookKind::TurnDone
            && self.status == AgentStatus::Done
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
                || event.source == "pi"
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
        self.record_owner(event);
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
        self.idle_samples = 0;
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
                ) || (event.source == "pi"
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
        true
    }

    /// No match resets consecutive-idle evidence. Full lifecycle hooks never
    /// consult screen rules, even if the current frame contains a real old form.
    pub fn observe_screen(&mut self, detection: Option<Detection>) -> bool {
        if !self.allows_screen() {
            return false;
        }
        let Some(detection) = detection else {
            self.idle_samples = 0;
            return false;
        };
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
        self.status = next;
        self.source = AgentStatusSource::Screen;
        self.rule = Some(detection.rule_id);
        true
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
