//! Program records share the PTY lifecycle, never the active grid's lifetime.
use super::Term;
use crate::event::{Event, EventListener};
use crate::program_status::{FEATURE_REPLY, Records, Report};

impl<T> Term<T> {
    pub fn program_statuses(&self) -> &Records {
        &self.program_statuses
    }

    /// StreamProcessor owns the wire order of status reports and RIS. Keep VT
    /// buffer replay from clearing records reported after a buffered reset.
    pub(crate) fn with_program_status_preserved<R>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let records = std::mem::take(&mut self.program_statuses);
        let result = operation(self);
        self.program_statuses = records;
        result
    }
}

impl<T: EventListener> Term<T> {
    pub(crate) fn report_program_status(&mut self, report: Report) {
        if report == Report::Query {
            self.event_proxy.send_event(Event::PtyWrite(FEATURE_REPLY.into()));
        } else if self.program_statuses.apply(report) {
            self.event_proxy.send_event(Event::ProgramStatusChanged);
        }
    }

    /// Drop live records at a shell prompt or transport end, retaining results.
    pub fn end_program_status_activity(&mut self) {
        if self.program_statuses.command_ended() {
            self.event_proxy.send_event(Event::ProgramStatusChanged);
        }
    }

    /// A user input or new shell command acknowledges retained results.
    /// Live records and inherited identity remain available.
    pub fn acknowledge_program_status_results(&mut self) -> bool {
        let changed = self.program_statuses.acknowledge_results();
        if changed {
            self.event_proxy.send_event(Event::ProgramStatusChanged);
        }
        changed
    }

    pub(crate) fn reset_program_status(&mut self) {
        if self.program_statuses.reset() {
            self.event_proxy.send_event(Event::ProgramStatusChanged);
        }
    }

    pub fn exit(&mut self) {
        self.end_program_status_activity();
        self.event_proxy.send_event(Event::Exit);
    }
}
