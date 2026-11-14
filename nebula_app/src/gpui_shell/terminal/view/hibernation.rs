use std::path::PathBuf;
use std::sync::Arc;

use nebula_terminal::Term;
use nebula_terminal::sync::FairMutex;
use nebula_terminal::term::grid_content::Scrollback;

use super::TerminalView;
use crate::gpui_shell::terminal::session::EventProxy;
use crate::platform::shell;
use crate::session::LaunchSession;

const MAX_SLEEP_BYTES: usize = 4 * 1024 * 1024;

pub(in crate::gpui_shell) struct SleepCandidate {
    pub cwd: PathBuf,
    pub launch: LaunchSession,
    shell_pid: u32,
    term: Arc<FairMutex<Term<EventProxy>>>,
    input_epoch: u64,
}

impl SleepCandidate {
    pub fn capture(&self) -> Option<Scrollback> {
        if !self.cwd.is_dir() || !no_child_processes(self.shell_pid) {
            return None;
        }
        let history = self.term.lock().capture_scrollback(MAX_SLEEP_BYTES)?;
        no_child_processes(self.shell_pid).then_some(history)
    }
}

fn no_child_processes(shell_pid: u32) -> bool {
    crate::process_tree::descendants(shell_pid)
        .is_ok_and(|processes| matches!(processes.as_slice(), [root] if root.pid == shell_pid))
}

impl TerminalView {
    pub(in crate::gpui_shell) fn has_been_idle_for(&self, timeout: std::time::Duration) -> bool {
        !self.output_visible && self.sleep_idle_since.elapsed() >= timeout
    }

    pub(in crate::gpui_shell) fn supports_sleep(&self) -> bool {
        self.exec_context.as_ref().is_some_and(|context| context.wsl_distribution().is_none())
            && matches!(self.session_launch, LaunchSession::Shell { .. } | LaunchSession::Default)
            && self.session.as_ref().is_some_and(|session| session.shell_pid != 0)
    }

    pub(super) fn observe_sleep_readiness(&mut self, name: &str, value: &str) {
        if !shell::POWERSHELL_SLEEP_READINESS {
            return;
        }
        if name == crate::completion_context::SHELL_VAR {
            self.power_shell_sleep_epoch = None;
        } else if name == "pebrel_sleep_ready"
            && self.suggest.completion_context.owns_current(value)
        {
            self.power_shell_sleep_epoch = Some(self.prompt_input_epoch);
        }
    }

    pub(in crate::gpui_shell) fn sleep_candidate(&self) -> Option<SleepCandidate> {
        if !self.supports_sleep()
            || self.error.is_some()
            || self.exited.is_some()
            || self.command_running
            || self.running_program.is_some()
            || self.pending_runtime_submit.is_some()
            || self.pending_shell_command.is_some()
            || self.recovery.target.is_some()
            || self.marked_text.is_some()
            || !self.suggest.line_buf.is_empty()
            || !self.port_forwards.is_empty()
        {
            return None;
        }
        let session = self.session.as_ref()?;
        let launch = match &self.session_launch {
            LaunchSession::Shell { .. } => self.session_launch.clone(),
            LaunchSession::Default => {
                let (name, shell) = shell::hibernation_default_shell()?;
                LaunchSession::Shell {
                    name,
                    program: shell.program().to_owned(),
                    args: shell.args().to_vec(),
                }
            },
            _ => return None,
        };
        if let LaunchSession::Shell { program, .. } = &launch
            && shell::requires_sleep_readiness_report(program)
            && self.power_shell_sleep_epoch != Some(self.prompt_input_epoch)
        {
            return None;
        }
        if !session.term.lock().at_unedited_prompt() {
            return None;
        }
        Some(SleepCandidate {
            cwd: self.working_directory()?.into(),
            launch,
            shell_pid: session.shell_pid,
            term: Arc::clone(&session.term),
            input_epoch: self.prompt_input_epoch,
        })
    }

    pub(in crate::gpui_shell) fn matches_sleep_candidate(
        &self,
        candidate: &SleepCandidate,
    ) -> bool {
        self.prompt_input_epoch == candidate.input_epoch
            && self.sleep_candidate().is_some_and(|current| {
                current.cwd == candidate.cwd
                    && current.launch == candidate.launch
                    && current.shell_pid == candidate.shell_pid
                    && Arc::ptr_eq(&current.term, &candidate.term)
            })
    }
}
