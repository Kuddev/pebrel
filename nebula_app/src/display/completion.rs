//! Legacy completion adapter; ownership rules live in completion_context.

use super::{Display, NebulaPaneState, nebula_clear_line, nebula_debug_log, suggest_engine};

impl Display {
    /// Commit the current line to history (on Enter) and reset the buffer.
    ///
    /// `screen_line` (the input read off the grid, i.e. what the shell's own
    /// editor really contained) wins over the keystroke-reconstructed
    /// `line_buf`: the latter desyncs on cursor motion / completion / history
    /// recall and used to commit spliced garbage like "laudeclaude", which the
    /// hint would then resurface as a command the user never typed.
    pub fn nebula_commit_line(&mut self, state: &mut NebulaPaneState) {
        // On Windows the grid read is the only source that sees tab
        // completions; when it failed (no prompt arrow — cmd/ssh/REPL — or a
        // mid-line edit) the keystroke buffer likely holds spliced garbage,
        // and recording that would resurface it forever as a bogus ghost hint
        // (truncated CJK paths were the visible symptom). Better no history
        // entry than a corrupted one.
        #[cfg(windows)]
        let line = state.screen_line.trim().to_owned();
        #[cfg(not(windows))]
        let line = if state.screen_line.trim().is_empty() {
            state.line_buf.trim()
        } else {
            state.screen_line.trim()
        }
        .to_owned();
        nebula_debug_log(format!(
            "input_commit cwd={:?} line={line:?} line_buf={:?} screen_line={:?}",
            state.cwd, state.line_buf, state.screen_line
        ));
        let committed =
            if line.is_empty() { state.line_buf.trim().to_owned() } else { line.clone() };
        if line.is_empty() {
            state.completion_submitted(&state.line_buf.clone());
        } else {
            state.record_completion_command(&mut self.nebula_history, &line);
        }
        // Kept for CommandStart (OSC 133;C): by the time it arrives from the
        // PTY these buffers are already cleared, so the program identity for
        // the tab icon has to be captured here. Fall back to the keystroke
        // buffer so the icon still resolves when the grid read failed. Agent
        // parsing also understands package runners such as npx/uvx.
        state.last_committed = committed;
        if let Some(agent) = crate::ai_agents::AgentKind::parse_command(&state.last_committed) {
            state.running_program = Some(agent.slug().to_owned());
            state.command_started = Some(std::time::Instant::now());
            state.agent_status = crate::ai_agents::AgentStatus::Working;
            state.agent_status_source = crate::ai_agents::AgentStatusSource::Process;
            state.agent_status_rule = None;
            state.agent_hook_seen = false;
            state.idle_screen_streak = 0;
            state.awaiting_input = false;
            state.finished_unseen = false;
            state.needs_attention = false;
        }
        nebula_clear_line(state);
    }

    /// Feed the shared directory model from an authoritative shell report.
    pub fn nebula_report_cwd(&self, state: &mut NebulaPaneState, cwd: &str) -> bool {
        if state.cwd == cwd {
            return false;
        }
        state.cwd = cwd.to_owned();
        if state.suggest_env.is_this_machine() {
            self.directory_history.record(cwd);
        }
        true
    }

    /// Recompute the inline ghost-text suggestion. `line_override` carries the
    /// grid-read input on Windows (the authoritative screen truth); when `None`
    /// the keystroke-tracked `line_buf` is used (other platforms). A whole
    /// previous command sharing the prefix wins (fish-style history hint);
    /// otherwise the final token gets path completion against the shell-reported
    /// cwd. Cached on `cwd\0buffer` so disk is only touched when the line
    /// changes — not every frame.
    pub(super) fn nebula_update_suggestion(
        &mut self,
        state: &mut NebulaPaneState,
        line_override: Option<String>,
    ) {
        suggest_engine::suggest_update(
            &suggest_engine::SuggestSources {
                history: &self.nebula_history,
                directories: &self.directory_history,
                commands: &self.nebula_commands,
                enabled: self.nebula_ghost_enabled,
                style: self.nebula_completion_style,
            },
            state,
            line_override,
        );
    }
}
