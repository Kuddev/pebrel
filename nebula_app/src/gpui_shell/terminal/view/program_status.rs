//! Presentation of the core's latest OSC 7501 records. No screen-text inference.
use gpui::{Context, EventEmitter as _};
use nebula_terminal::program_status::{Records, State};

use super::{TerminalView, TerminalViewEvent};
use crate::runtime_api::RuntimeTaskState;
use crate::taskbar::TaskProgress;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProgramStatus {
    state: State,
    progress: Option<u8>,
    pub app: Option<String>,
}

impl ProgramStatus {
    fn select(records: &Records) -> Option<Self> {
        // A blocked child must remain visible under a working/idle root.
        // Equal priorities use the latest update, matching the core's LRU order.
        let record =
            records.iter().filter(|record| !record.result_acknowledged).max_by_key(|record| {
                match record.state {
                    State::Blocked => 5,
                    State::Error => 4,
                    State::Working => 3,
                    State::Done => 2,
                    State::Idle => 1,
                }
            })?;
        Some(Self {
            app: records.app_for(record).map(str::to_owned),
            state: record.state,
            progress: record.progress,
        })
    }

    pub fn task_state(&self) -> RuntimeTaskState {
        match self.state {
            State::Idle => RuntimeTaskState::Idle,
            State::Working => RuntimeTaskState::Running,
            State::Done => RuntimeTaskState::Finished,
            State::Blocked => RuntimeTaskState::Attention,
            State::Error => RuntimeTaskState::Failed,
        }
    }

    fn progress(&self) -> TaskProgress {
        match self.state {
            State::Working => {
                self.progress.map_or(TaskProgress::Indeterminate, TaskProgress::Value)
            },
            State::Blocked => TaskProgress::Paused(self.progress),
            State::Error => TaskProgress::Error(None),
            State::Idle | State::Done => TaskProgress::None,
        }
    }

    fn is_result(&self) -> bool {
        matches!(self.state, State::Done | State::Error)
    }
}

impl TerminalView {
    pub(super) fn effective_program_status(&self) -> Option<&ProgramStatus> {
        // Explicit program state wins over Hook and screen/process fallback.
        // Hook ownership still supplies session identity, answers and requests.
        if self.error.is_some()
            || self.pty_failed
            || matches!(self.ssh_stage, Some(crate::ssh_session::SshStage::Failed(_)))
        {
            return None;
        }
        self.program_status.as_ref().filter(|status| self.exited.is_none() || status.is_result())
    }

    pub(super) fn refresh_program_status(&mut self, cx: &mut Context<Self>) {
        let (seen, status) = self.session.as_ref().map_or((false, None), |session| {
            let term = session.term.lock();
            (term.program_statuses().seen(), ProgramStatus::select(term.program_statuses()))
        });
        if self.program_status_seen == seen && self.program_status == status {
            return;
        }
        self.program_status_seen = seen;
        self.program_status = status;
        // A reset must clear the old projection even after releasing ownership.
        self.apply_program_progress(cx);
        // State badges and Runtime API read the same projection. Free text is
        // retained in the core only; it is never interpreted or used as markup.
        // Do not add desktop notifications: hooks already own those edges.
        cx.emit(TerminalViewEvent::TitleChanged);
        cx.notify();
    }

    pub(super) fn sync_program_progress(&mut self, cx: &mut Context<Self>) {
        if self.program_status_seen {
            self.apply_program_progress(cx);
        }
    }

    fn apply_program_progress(&mut self, cx: &mut Context<Self>) {
        let progress =
            self.effective_program_status().map_or(TaskProgress::None, ProgramStatus::progress);
        if self.progress != progress {
            self.progress = progress;
            cx.emit(TerminalViewEvent::ProgressChanged(progress));
        }
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;
