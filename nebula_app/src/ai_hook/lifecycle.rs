//! One pane's Agent lifecycle, shared by both UI shells.
//!
//! Hook events are facts; screen matches are observations with limited authority.
//! Command ownership ends at the shell boundary, never because output goes quiet.

use crate::ai_agents::{AgentStatus, AgentStatusSource, Detection};
use std::collections::HashSet;

use super::{AiHookEvent, AiHookKind};

/// A stop from these sources proves nothing about the answer unless it carries a result, so
/// an unclassified one returns the pane to idle instead of announcing completion.
const UNPROVEN_STOP: [&str; 5] = ["pi", "omp", "cursor", "copilot", "antigravity"];

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
    /// 停滞对账的连续形态计数：idle 拍（收敛）与 working 拍（点火）各一份，
    /// 任一拍出现对方形态即归零——交替出现的证据不构成「稳定形态」。
    idle_samples: u8,
    working_samples: u8,
    unmatched_samples: u8,
    /// Only a preceding Working state may survive a screen form's expiry.
    screen_blocked_fallback: Option<(AgentStatusSource, Option<String>)>,
    turn_observed: bool,
    pending_submit: bool,
    /// 屏幕停滞对账推断的完成（Done/Idle）标记。推断不是权威：同 turn 的
    /// 迟到真 TurnDone 仍要被采纳（补真实的成败与通知），去重护栏只拦
    /// 权威完成的重放。任何被采纳的 hook 事件与命令边界都会清掉它。
    inferred_done: bool,
    /// A completion-only hook latches its result until an idle screen or an
    /// explicit submission separates the old frame from a new turn.
    screen_armed: bool,
    command_ended: bool,
    native_codex: bool,
    turn_id: Option<String>,
    /// Keep every prompt identity for this Claude session so delayed hooks
    /// cannot become valid again after a bounded history evicts them.
    claude_prompt_ids: HashSet<String>,
    /// Reject Claude events whose ingress sequence predates the last input,
    /// including raw remote envelopes parsed only after leaving the mailbox.
    submit_receive_sequence: u64,
    last_remote_sequence: Option<u64>,
    /// 最近一次明确提交或被采纳的 hook 事件时刻。远端停滞对账必须给
    /// 每次新提交完整等待期限，不沿用上一回合的空闲时间。
    /// （见 [`AgentActivity::reconcile_stalled_turn`]）。
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
            working_samples: 0,
            unmatched_samples: 0,
            screen_blocked_fallback: None,
            turn_observed: false,
            pending_submit: false,
            inferred_done: false,
            screen_armed: true,
            command_ended: false,
            native_codex: false,
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
        self.status
    }

    pub fn is_current_turn(&self, turn_id: &str) -> bool {
        self.turn_id.as_deref() == Some(turn_id)
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
        self.submitted_at(super::current_receive_sequence());
    }

    pub fn submitted_at(&mut self, received_sequence: u64) {
        self.status = AgentStatus::Working;
        self.source = AgentStatusSource::Process;
        self.rule = None;
        self.screen_blocked_fallback = None;
        self.turn_observed = true;
        self.pending_submit = true;
        self.last_activity_at = Some(std::time::Instant::now());
        self.input_sent_at(received_sequence);
    }

    /// Input sent while Claude is still working may queue the next prompt.
    /// Keep the current turn active while waiting for its distinct prompt ID.
    pub fn queued_submit(&mut self) {
        self.pending_submit = true;
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
        self.working_samples = 0;
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
        if event.source == "claude"
            && event.kind == AiHookKind::PromptSubmit
            && event
                .turn_id
                .as_deref()
                .is_some_and(|turn_id| self.claude_prompt_ids.contains(turn_id))
        {
            return false;
        }
        // Claude provides no source sequence. A new prompt must have a new
        // identity; a delayed hook from a previously observed turn cannot
        // consume the submit guard.
        if event.source == "claude"
            && event.kind == AiHookKind::PromptSubmit
            && self.turn_id.is_some()
            && (!self.pending_submit
                || event.turn_id.is_none()
                || event.turn_id.as_deref() == self.turn_id.as_deref())
        {
            return false;
        }
        if event.source == "claude"
            && self.pending_submit
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
            && !self.inferred_done
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
            && event.kind == AiHookKind::PromptSubmit
            && let Some(turn_id) = event.turn_id.as_ref()
        {
            self.claude_prompt_ids.insert(turn_id.clone());
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
        self.inferred_done = false;
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

    /// Whether the latest explicit submission or accepted hook has been quiet
    /// for `threshold`. Screen samples, plain Enter and rejected hooks do not
    /// move this boundary. The caller separately requires hook ownership.
    pub fn turn_quiet_for(&self, threshold: std::time::Duration) -> bool {
        self.last_activity_at.is_none_or(|at| at.elapsed() >= threshold)
    }

    /// 远端回合停滞对账（双向）：屏幕证据的第二重用途，语义对齐本地进程树对账
    /// （`apply_activity_evidence`）的「进程消失 ≠ 失败，只收边」，以及它的镜像
    /// ——「屏幕持续转圈 ≠ 回合没开始」。前置 `!allows_screen()`——正常路径（无
    /// hook 权威）仍走 [`Self::observe_screen`]。SSH/WSL 的 hook 有真实丢失路径
    /// （远端 bridge 通道锁 0.3s busy 即丢、hook 超时、envelope 超限）：丢 Stop
    /// 把回合永久钉在 Working；丢 UserPromptSubmit/权限答复则把它钉在上一回合的
    /// Done/Blocked。这里在「hook 已静默超阈值 + 屏幕连续两拍同形态」同时成立时
    /// 双向收敛：idle 提示符两拍 → Done/Idle（`turn_observed` 语义与
    /// `observe_screen` 的 idle 分支一致）；working 转圈两拍 → 重新点火 Working
    /// 并补记 `turn_observed`（迟到的 Stop 能正确收敛）。只改状态，不清身份：
    /// 远端 CLI 还活着，`running_program`/会话身份归命令边界（133;D /
    /// SessionEnd）管。
    ///
    /// 停滞未到、刚提交（`pending_submit`）、无匹配或两拍中途形态翻面都会归零
    /// 该方向的连续计数——误判的防线是「agent 自家的稳定屏幕形态」，不是纯计时。
    pub fn reconcile_stalled_turn(&mut self, detection: Option<&Detection>, stalled: bool) -> bool {
        if self.allows_screen() {
            return false;
        }
        if !stalled {
            self.idle_samples = 0;
            self.working_samples = 0;
            return false;
        }
        if self.pending_submit {
            // 提交屏障防的是「上一帧的 idle 屏幕把刚提交的回合立刻判完成」，
            // 它只活到停滞阈值：提交后 hook 全程静默超过阈值（PromptSubmit
            // 与后续事件在 bridge 上全部丢失）时屏障过期，屏幕证据恢复话语
            // 权。本拍只过期屏障并把两向计数归零——收敛/点火仍需其后连续
            // 两拍稳定形态，不因过期而即时定论。
            self.pending_submit = false;
            self.idle_samples = 0;
            self.working_samples = 0;
            return false;
        }
        let Some(detection) = detection else {
            self.idle_samples = 0;
            self.working_samples = 0;
            return false;
        };
        let next = match detection.status {
            AgentStatus::Idle => {
                self.working_samples = 0;
                self.idle_samples = self.idle_samples.saturating_add(1);
                if self.idle_samples < 2 {
                    return false;
                }
                if self.turn_observed { AgentStatus::Done } else { AgentStatus::Idle }
            },
            AgentStatus::Working => {
                self.idle_samples = 0;
                self.working_samples = self.working_samples.saturating_add(1);
                if self.working_samples < 2 {
                    return false;
                }
                // 点火即观测到回合：迟到的 Stop 依此收敛为 Done 而非 Idle。
                self.turn_observed = true;
                AgentStatus::Working
            },
            // 权限等待（Blocked）是 attention 语义，归 Notification hook 与
            // `observe_screen` 的权威判定管；Done/Unknown 无形态可依。
            AgentStatus::Blocked | AgentStatus::Done | AgentStatus::Unknown => {
                self.idle_samples = 0;
                self.working_samples = 0;
                return false;
            },
        };
        if next == self.status {
            return false;
        }
        self.status = next;
        self.source = AgentStatusSource::Screen;
        self.rule = Some(detection.rule_id.clone());
        // 收敛为 Done/Idle 是**推断**完成（点火 Working 则推翻旧推断）：
        // 迟到的同 turn 真 TurnDone 不受去重护栏拦截（见 `accepts_hook`）。
        self.inferred_done = matches!(next, AgentStatus::Done | AgentStatus::Idle);
        self.idle_samples = 0;
        self.working_samples = 0;
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
